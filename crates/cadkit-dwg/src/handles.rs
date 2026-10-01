//! Object map / AcDb:Handles section (ODA spec chapter 23).
//!
//! A sequence of chunks: a big-endian `RS` chunk size (the size field plus the pairs;
//! the spec's 2032-byte cap is not honoured by all writers), then pairs of unsigned modular-char handle
//! delta and signed modular-char offset delta, then a big-endian CRC. A chunk of size
//! 2 ends the map. The running handle and offset restart from 0 in every chunk
//! (the spec implies one running total; the files and ACadSharp reset per chunk).

use std::collections::{BTreeMap, HashSet};

use cadkit_core::{Error, Result};

use crate::bits::BitReader;
use crate::crc::crc8;

/// Result of parsing the object map.
#[derive(Debug, Default)]
pub struct ObjectMap {
    /// handle → object offset.
    pub map: BTreeMap<u64, u64>,
    /// Number of chunks whose CRC did not match.
    pub crc_failures: usize,
    /// Pairs dropped (zero handle delta or negative offset).
    pub dropped: usize,
    /// Where the map ended early (section data exhausted before the end chunk).
    pub truncated_at: Option<u64>,
    /// Pairs ignored because their handle or their offset was already mapped (the
    /// first entry wins; one offset holds one object).
    pub duplicates: usize,
    /// Pairs ignored beyond `max_entries`: more objects than the object data can hold.
    pub excess: usize,
}

/// Parses the object map. More than `max_objects` entries is a limit error; entries
/// beyond `max_entries` (a plausibility bound from the object data size) are counted
/// in [`ObjectMap::excess`] and ignored.
pub fn parse(data: &[u8], base: u64, max_objects: u64, max_entries: usize) -> Result<ObjectMap> {
    let mut out = ObjectMap::default();
    let mut offsets = HashSet::new();
    let mut pos = 0usize;
    loop {
        let size = match data.get(pos..pos + 2) {
            Some(&[hi, lo]) => usize::from(u16::from_be_bytes([hi, lo])),
            _ => {
                out.truncated_at = Some(base + pos as u64);
                break;
            }
        };
        if size <= 2 {
            break;
        }
        let body_start = pos + 2;
        // The spec says chunks are cut off at 2032 bytes; AutoCAD writes larger ones
        // (a 2033-byte body in a public R2010 sample), so the declared size is used.
        let body_end = body_start + (size - 2);
        // A short last chunk keeps the pairs that are there.
        let Some(body) = data.get(body_start..body_end.min(data.len())) else {
            out.truncated_at = Some(base + pos as u64);
            break;
        };
        if body_end > data.len() {
            out.truncated_at = Some(base + pos as u64);
        }
        let crc_bytes = data.get(body_end..body_end + 2);
        if let (Some(&[hi, lo]), Some(chunk)) = (crc_bytes, data.get(pos..body_end)) {
            if crc8(0xC0C1, chunk) != u16::from_be_bytes([hi, lo]) {
                out.crc_failures += 1;
            }
        }
        let mut r = BitReader::with_base(body, base + body_start as u64);
        let mut handle: u64 = 0;
        let mut offset: i64 = 0;
        while r.remaining_bits() >= 16 {
            let (Ok(dh), Ok(dl)) = (r.umc(), r.mc()) else {
                out.truncated_at.get_or_insert(base + pos as u64);
                break;
            };
            handle = handle
                .checked_add(dh)
                .ok_or_else(|| r.invalid("handle overflow"))?;
            offset = offset
                .checked_add(dl)
                .ok_or_else(|| r.invalid("offset overflow"))?;
            match u64::try_from(offset) {
                Ok(o) if dh > 0 => {
                    if out.map.contains_key(&handle) || offsets.contains(&o) {
                        out.duplicates += 1;
                    } else if out.map.len() >= max_entries {
                        out.excess += 1;
                    } else {
                        offsets.insert(o);
                        out.map.insert(handle, o);
                        if out.map.len() as u64 > max_objects {
                            return Err(Error::LimitExceeded(format!(
                                "more than {max_objects} objects"
                            )));
                        }
                    }
                }
                _ => out.dropped += 1,
            }
        }
        if out.truncated_at.is_some() {
            break;
        }
        pos = body_end + 2;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_chunks_reset_running_values() {
        // Chunk 1: pairs (+1, +100), (+2, +50) -> handles 1@100, 3@150.
        let body1 = [0x01, 0x80 | (100 & 0x7F), 100 >> 7, 0x02, 0x32];
        let mut data = Vec::new();
        data.extend(((body1.len() + 2) as u16).to_be_bytes());
        data.extend(body1);
        let crc = crc8(0xC0C1, &data);
        data.extend(crc.to_be_bytes());
        // Chunk 2: (+5, +10) -> handle 5@10 (restarted from 0).
        let start = data.len();
        let body2 = [0x05, 0x0A];
        data.extend(((body2.len() + 2) as u16).to_be_bytes());
        data.extend(body2);
        let crc = crc8(0xC0C1, &data[start..]);
        data.extend(crc.to_be_bytes());
        // End chunk.
        data.extend([0x00, 0x02, 0x00, 0x00]);
        let m = parse(&data, 0, 100, 100).unwrap();
        assert_eq!(
            m.map.into_iter().collect::<Vec<_>>(),
            vec![(1, 100), (3, 150), (5, 10)]
        );
        assert_eq!(m.crc_failures, 0);
    }

    /// One chunk of `(handle delta, offset delta)` pairs (single-byte MC values),
    /// without the end chunk.
    fn chunk(pairs: &[(u8, u8)]) -> Vec<u8> {
        let body: Vec<u8> = pairs.iter().flat_map(|&(h, o)| [h, o]).collect();
        let mut data = ((body.len() + 2) as u16).to_be_bytes().to_vec();
        data.extend(body);
        let crc = crc8(0xC0C1, &data);
        data.extend(crc.to_be_bytes());
        data
    }

    const END: [u8; 4] = [0x00, 0x02, 0x00, 0x00];

    #[test]
    fn repeated_offsets_and_handles_are_dropped() {
        // Handles 1, 2, 3 all at offset 0x10 (offset delta 0 after the first); a second
        // chunk repeats handle 1 at another offset, then maps handle 5 at 0x30.
        let mut data = chunk(&[(1, 0x10), (1, 0), (1, 0)]);
        data.extend(chunk(&[(1, 0x20), (4, 0x10)]));
        data.extend(END);
        let m = parse(&data, 0, 100, 100).unwrap();
        assert_eq!(
            m.map.into_iter().collect::<Vec<_>>(),
            vec![(1, 0x10), (5, 0x30)]
        );
        assert_eq!(m.duplicates, 3);
    }

    #[test]
    fn entries_beyond_the_plausible_count_are_ignored() {
        // A thousand distinct objects cannot fit in 40 bytes of object data.
        let pairs: Vec<(u8, u8)> = (0..1000).map(|_| (1, 1)).collect();
        let mut data = chunk(&pairs);
        data.extend(END);
        let m = parse(&data, 0, 100_000, 10).unwrap();
        assert_eq!((m.map.len(), m.excess), (10, 990));
    }

    #[test]
    fn truncated_map_keeps_what_was_read() {
        // Chunk of 6 bytes declared, only one pair (+1, +16) present.
        let m = parse(&[0x00, 0x08, 0x01, 0x10], 0, 100, 100).unwrap();
        assert_eq!(m.map.into_iter().collect::<Vec<_>>(), vec![(1, 16)]);
        let m = parse(&[], 0, 100, 100).unwrap();
        assert!(m.map.is_empty() && m.truncated_at == Some(0));
    }
}
