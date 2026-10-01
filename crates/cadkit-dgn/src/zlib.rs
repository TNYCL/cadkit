//! Bounded zlib inflation for V8 streams and pages.

use flate2::{Decompress, FlushDecompress, Status};

/// Result of inflating one zlib member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Inflated {
    /// Decompressed bytes (possibly partial, see `complete`).
    pub(crate) data: Vec<u8>,
    /// The stream reached its end marker and checksum.
    pub(crate) complete: bool,
    /// Why inflation stopped early, when it did.
    pub(crate) problem: Option<String>,
}

/// Inflates a zlib stream, never producing more than `max_out` bytes.
///
/// Corrupt or truncated input yields the bytes decoded so far with `complete = false`
/// instead of an error, so callers can still walk the records that survived. Exceeding
/// `max_out` is reported through `problem` and stops output at the limit.
pub(crate) fn inflate(input: &[u8], max_out: usize) -> Inflated {
    let mut dec = Decompress::new(true);
    let mut data: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let consumed = usize::try_from(dec.total_in()).unwrap_or(usize::MAX);
        let rest = input.get(consumed..).unwrap_or(&[]);
        let room = max_out.saturating_sub(data.len());
        let window = room.saturating_add(1).min(chunk.len()).max(1);
        let Some(out) = chunk.get_mut(..window) else {
            return done(data, false, Some("internal buffer error".into()));
        };
        let before_out = dec.total_out();
        let before_in = dec.total_in();
        let status = match dec.decompress(rest, out, FlushDecompress::None) {
            Ok(s) => s,
            Err(e) => return done(data, false, Some(format!("zlib: {e}"))),
        };
        let produced = usize::try_from(dec.total_out() - before_out).unwrap_or(usize::MAX);
        if produced > room {
            let keep = out.get(..room).unwrap_or(&[]);
            data.extend_from_slice(keep);
            return done(
                data,
                false,
                Some(format!("inflated size exceeds {max_out} bytes")),
            );
        }
        data.extend_from_slice(out.get(..produced).unwrap_or(&[]));
        match status {
            Status::StreamEnd => return done(data, true, None),
            _ if produced == 0 && dec.total_in() == before_in => {
                return done(data, false, Some("zlib stream is truncated".into()));
            }
            _ => {}
        }
    }
}

fn done(data: Vec<u8>, complete: bool, problem: Option<String>) -> Inflated {
    Inflated {
        data,
        complete,
        problem,
    }
}

#[cfg(test)]
pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflates_complete_streams() {
        let z = compress(b"hello hello hello");
        let r = inflate(&z, 100);
        assert!(r.complete);
        assert_eq!(r.data, b"hello hello hello");
    }

    #[test]
    fn output_is_bounded() {
        let big = vec![7u8; 100_000];
        let r = inflate(&compress(&big), 1000);
        assert!(!r.complete);
        assert_eq!(r.data.len(), 1000);
        assert!(r.problem.unwrap().contains("exceeds"));
    }

    #[test]
    fn truncated_and_garbage_input_do_not_panic() {
        let z = compress(&[1u8; 5000]);
        let r = inflate(&z[..z.len() / 2], 10_000);
        assert!(!r.complete);
        let r = inflate(&[0xde, 0xad, 0xbe, 0xef], 10);
        assert!(!r.complete);
        let r = inflate(&[], 10);
        assert!(!r.complete);
    }
}
