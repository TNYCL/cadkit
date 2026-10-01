//! Reed-Solomon block de-interleaving for R2007 pages (ODA spec §5.13).
//!
//! R2007 stores system pages as RS(255,239) and data pages as RS(255,251) code
//! words, interleaved byte by byte across blocks. cadkit does not correct errors:
//! it only gathers the data bytes of each block (the parity bytes follow them in
//! every code word) and relies on the CRCs/structure checks downstream, as
//! ACadSharp does.

/// Code word size.
pub const CODEWORD: usize = 255;
/// Data bytes per system page block, RS(255,239).
pub const SYSTEM_K: usize = 239;
/// Data bytes per data page block, RS(255,251).
pub const DATA_K: usize = 251;

/// De-interleaves `blocks` code words from `encoded`, returning `out_len` data bytes
/// (at most `blocks * k`). Byte `j` of block `i` is stored at `encoded[i + j * blocks]`.
/// Missing input bytes are treated as zero; the caller checks sizes beforehand.
pub fn deinterleave(encoded: &[u8], blocks: usize, k: usize, out_len: usize) -> Vec<u8> {
    let out_len = out_len.min(blocks.saturating_mul(k));
    let mut out = Vec::with_capacity(out_len);
    'outer: for block in 0..blocks {
        for j in 0..k {
            if out.len() >= out_len {
                break 'outer;
            }
            let index = j.checked_mul(blocks).and_then(|v| v.checked_add(block));
            out.push(index.and_then(|i| encoded.get(i)).copied().unwrap_or(0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gathers_data_bytes_of_each_block() {
        // Two blocks of k=3 interleaved: positions 0,2,4 belong to block 0.
        let encoded = [10, 20, 11, 21, 12, 22, 99, 98];
        assert_eq!(
            deinterleave(&encoded, 2, 3, 6),
            vec![10, 11, 12, 20, 21, 22]
        );
        assert_eq!(deinterleave(&encoded, 2, 3, 4), vec![10, 11, 12, 20]);
        // Short input never panics.
        assert_eq!(deinterleave(&[1], 2, 2, 4), vec![1, 0, 0, 0]);
        assert!(deinterleave(&[], usize::MAX, usize::MAX, 0).is_empty());
    }
}
