//! The R2007 LZ77 variant (ODA spec §5.10).
//!
//! Ported from ACadSharp's `DwgLZ77AC21Decompressor` (MIT, © DomCR), with every
//! index bounds-checked. The literal byte shuffle of §5.10.1 is precomputed into
//! [`LITERAL_ORDER`] instead of 32 hand-written copy functions.

use cadkit_core::{Error, Result};

/// Copy primitives of §5.10.1: (kind, source offset, destination offset).
/// Kind 1–4 and 8 copy that many bytes; 2 and 3 copy in reverse order;
/// 16 copies two 8-byte halves swapped.
type CopyOp = (u8, u8, u8);

/// Per literal length 1..=32, the primitive copies that make up the shuffle.
const COPY_PLANS: [&[CopyOp]; 33] = [
    &[],
    &[(1, 0, 0)],
    &[(2, 0, 0)],
    &[(3, 0, 0)],
    &[(4, 0, 0)],
    &[(1, 4, 0), (4, 0, 1)],
    &[(1, 5, 0), (4, 1, 1), (1, 0, 5)],
    &[(2, 5, 0), (4, 1, 2), (1, 0, 6)],
    &[(8, 0, 0)],
    &[(1, 8, 0), (8, 0, 1)],
    &[(1, 9, 0), (8, 1, 1), (1, 0, 9)],
    &[(2, 9, 0), (8, 1, 2), (1, 0, 10)],
    &[(4, 8, 0), (8, 0, 4)],
    &[(1, 12, 0), (4, 8, 1), (8, 0, 5)],
    &[(1, 13, 0), (4, 9, 1), (8, 1, 5), (1, 0, 13)],
    &[(2, 13, 0), (4, 9, 2), (8, 1, 6), (1, 0, 14)],
    &[(16, 0, 0)],
    &[(8, 9, 0), (1, 8, 8), (8, 0, 9)],
    &[(1, 17, 0), (16, 1, 1), (1, 0, 17)],
    &[(3, 16, 0), (16, 0, 3)],
    &[(4, 16, 0), (8, 8, 4), (8, 0, 12)],
    &[(1, 20, 0), (4, 16, 1), (8, 8, 5), (8, 0, 13)],
    &[(2, 20, 0), (4, 16, 2), (8, 8, 6), (8, 0, 14)],
    &[(3, 20, 0), (4, 16, 3), (8, 8, 7), (8, 0, 15)],
    &[(8, 16, 0), (16, 0, 8)],
    &[(8, 17, 0), (1, 16, 8), (16, 0, 9)],
    &[(1, 25, 0), (8, 17, 1), (1, 16, 9), (16, 0, 10)],
    &[(2, 25, 0), (8, 17, 2), (1, 16, 10), (16, 0, 11)],
    &[(4, 24, 0), (8, 16, 4), (8, 8, 12), (8, 0, 20)],
    &[(1, 28, 0), (4, 24, 1), (8, 16, 5), (8, 8, 13), (8, 0, 21)],
    &[(2, 28, 0), (4, 24, 2), (8, 16, 6), (8, 8, 14), (8, 0, 22)],
    &[
        (1, 30, 0),
        (4, 26, 1),
        (8, 18, 5),
        (8, 10, 13),
        (8, 2, 21),
        (2, 0, 29),
    ],
    &[(16, 16, 0), (16, 0, 16)],
];

/// `LITERAL_ORDER[n][i]` is the source index of destination byte `i` when copying a
/// literal run of `n` (1..=32) bytes.
pub(crate) const LITERAL_ORDER: [[u8; 32]; 33] = build_literal_order();

// Evaluated at compile time: an out-of-range index would fail the build, not panic.
#[allow(clippy::indexing_slicing)]
const fn build_literal_order() -> [[u8; 32]; 33] {
    let mut table = [[0u8; 32]; 33];
    let mut n = 1;
    while n <= 32 {
        let plan = COPY_PLANS[n];
        let mut k = 0;
        while k < plan.len() {
            let (kind, s, d) = plan[k];
            let (s, d) = (s as usize, d as usize);
            let row = &mut table[n];
            match kind {
                1 => row[d] = s as u8,
                2 => {
                    row[d] = (s + 1) as u8;
                    row[d + 1] = s as u8;
                }
                3 => {
                    row[d] = (s + 2) as u8;
                    row[d + 1] = (s + 1) as u8;
                    row[d + 2] = s as u8;
                }
                4 | 8 => {
                    let mut i = 0;
                    while i < kind as usize {
                        row[d + i] = (s + i) as u8;
                        i += 1;
                    }
                }
                _ => {
                    let mut i = 0;
                    while i < 8 {
                        row[d + i] = (s + 8 + i) as u8;
                        row[d + 8 + i] = (s + i) as u8;
                        i += 1;
                    }
                }
            }
            k += 1;
        }
        n += 1;
    }
    table
}

struct State<'a> {
    src: &'a [u8],
    pos: usize,
    end: usize,
    /// Output so far; grows with the data, never past `limit`.
    out: Vec<u8>,
    limit: usize,
    base: u64,
}

impl State<'_> {
    fn err(&self, message: &str) -> Error {
        Error::invalid(
            self.base.saturating_add(self.pos as u64),
            format!("R2007 LZ77: {message}"),
        )
    }

    fn next(&mut self) -> Result<u8> {
        if self.pos >= self.end {
            return Err(Error::Truncated {
                offset: self.base.saturating_add(self.pos as u64),
                needed: 1,
            });
        }
        let byte = self
            .src
            .get(self.pos)
            .copied()
            .ok_or_else(|| self.err("input overrun"))?;
        self.pos += 1;
        Ok(byte)
    }

    fn literal_length(&mut self, opcode: u8) -> Result<usize> {
        let mut length = usize::from(opcode) + 8;
        if length == 0x17 {
            let n = self.next()?;
            length += usize::from(n);
            if n == 0xFF {
                loop {
                    let lo = self.next()?;
                    let hi = self.next()?;
                    let n = u16::from_le_bytes([lo, hi]);
                    length = length.saturating_add(usize::from(n));
                    if n != 0xFFFF {
                        break;
                    }
                }
            }
        }
        Ok(length)
    }

    /// Copies `length` literal bytes with the §5.10.1 byte shuffle.
    fn copy_literal(&mut self, length: usize) -> Result<()> {
        let src_end = self
            .pos
            .checked_add(length)
            .filter(|&e| e <= self.end)
            .ok_or_else(|| self.err("literal run past input"))?;
        if self
            .out
            .len()
            .checked_add(length)
            .is_none_or(|e| e > self.limit)
        {
            return Err(self.err("literal run past output"));
        }
        let mut s = self.pos;
        while s < src_end {
            let n = (src_end - s).min(32);
            let order = LITERAL_ORDER
                .get(n)
                .ok_or_else(|| self.err("literal order"))?;
            for i in 0..n {
                let from = s + usize::from(order.get(i).copied().unwrap_or(0));
                let byte = self
                    .src
                    .get(from)
                    .copied()
                    .ok_or_else(|| self.err("literal source"))?;
                self.out.push(byte);
            }
            s += n;
        }
        self.pos = src_end;
        Ok(())
    }

    /// Copies `length` bytes from `offset` bytes back in the output (may overlap).
    fn copy_back(&mut self, length: usize, offset: usize) -> Result<()> {
        let start = self
            .out
            .len()
            .checked_sub(offset)
            .ok_or_else(|| self.err("back-reference before start of output"))?;
        if self
            .out
            .len()
            .checked_add(length)
            .is_none_or(|e| e > self.limit)
        {
            return Err(self.err("match past output"));
        }
        // Byte by byte: the source may overlap the bytes being written (runs).
        for i in 0..length {
            let byte = self.out.get(start + i).copied().unwrap_or(0);
            self.out.push(byte);
        }
        Ok(())
    }

    /// Decodes one match instruction; updates `opcode` to the byte holding the next
    /// literal count. Returns (length, offset).
    fn instruction(&mut self, opcode: &mut u8) -> Result<(usize, usize)> {
        Ok(match *opcode >> 4 {
            0 => {
                let mut length = usize::from(*opcode & 0x0F) + 0x13;
                let low = usize::from(self.next()?);
                *opcode = self.next()?;
                length += usize::from((*opcode >> 3) & 0x10);
                let offset = (usize::from(*opcode & 0x78) << 5) + 1 + low;
                (length, offset)
            }
            1 => {
                let length = usize::from(*opcode & 0x0F) + 3;
                let low = usize::from(self.next()?);
                *opcode = self.next()?;
                let offset = (usize::from(*opcode & 0xF8) << 5) + 1 + low;
                (length, offset)
            }
            2 => {
                let lo = usize::from(self.next()?);
                let hi = usize::from(self.next()?);
                let mut offset = (hi << 8) | lo;
                let mut length = usize::from(*opcode & 7);
                if *opcode & 8 == 0 {
                    *opcode = self.next()?;
                    length += usize::from(*opcode & 0xF8);
                } else {
                    offset += 1;
                    length += usize::from(self.next()?) << 3;
                    *opcode = self.next()?;
                    length += (usize::from(*opcode & 0xF8) << 8) + 0x100;
                }
                (length, offset)
            }
            _ => {
                let length = usize::from(*opcode >> 4);
                let low = usize::from(*opcode & 0x0F);
                *opcode = self.next()?;
                let offset = (usize::from(*opcode & 0xF8) << 1) + low + 1;
                (length, offset)
            }
        })
    }
}

/// Decompresses `src` into at most `max_out` bytes. The output grows with the data
/// (a declared size from the file never allocates up front); it is shorter than
/// `max_out` when the stream ends early, and callers treat the rest as zeros.
pub fn decompress(src: &[u8], max_out: usize, base: u64) -> Result<Vec<u8>> {
    let out = Vec::with_capacity(max_out.min(1 << 20));
    let mut st = State {
        src,
        pos: 0,
        end: src.len(),
        out,
        limit: max_out,
        base,
    };
    if st.end == 0 {
        return Ok(st.out);
    }
    let mut opcode = st.next()?;
    let mut length = 0usize;
    if opcode & 0xF0 == 0x20 {
        st.pos = st.pos.checked_add(3).ok_or_else(|| st.err("header"))?;
        let byte = st
            .src
            .get(st.pos.wrapping_sub(1))
            .copied()
            .ok_or_else(|| st.err("truncated header"))?;
        length = usize::from(byte & 7);
    }
    while st.pos < st.end {
        if length == 0 {
            length = st.literal_length(opcode)?;
        }
        st.copy_literal(length)?;
        if st.pos >= st.end {
            break;
        }
        opcode = st.next()?;
        let (mut len, mut offset) = st.instruction(&mut opcode)?;
        loop {
            st.copy_back(len, offset)?;
            length = usize::from(opcode & 7);
            if length != 0 || st.pos >= st.end {
                break;
            }
            opcode = st.next()?;
            if opcode >> 4 == 0 {
                break;
            }
            if opcode >> 4 == 0x0F {
                opcode &= 0x0F;
            }
            (len, offset) = st.instruction(&mut opcode)?;
        }
    }
    Ok(st.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_orders_are_permutations() {
        for n in 1..=32usize {
            let mut seen = vec![false; n];
            for &s in &LITERAL_ORDER[n][..n] {
                assert!(!seen[s as usize], "length {n} repeats source {s}");
                seen[s as usize] = true;
            }
            assert!(seen.iter().all(|&b| b), "length {n} misses a byte");
        }
        // Spec §5.10.1 table rows.
        assert_eq!(&LITERAL_ORDER[2][..2], &[1, 0]);
        assert_eq!(&LITERAL_ORDER[5][..5], &[4, 0, 1, 2, 3]);
        assert_eq!(&LITERAL_ORDER[32][..8], &[24, 25, 26, 27, 28, 29, 30, 31]);
    }

    #[test]
    fn single_literal_run() {
        // Opcode 0x02 -> literal length 10, shuffled as 1[9], 8[1], 1[0].
        let src = [0x02, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        assert_eq!(
            decompress(&src, 10, 0).unwrap(),
            vec![9, 1, 2, 3, 4, 5, 6, 7, 8, 0]
        );
    }

    #[test]
    fn literal_then_back_reference() {
        // 8 literals (opcode 0x00), then opcode 0x41 0x00: length 4, offset
        // ((0 & 0xF8) << 1) + 1 + 1 = 2, literal count (0 & 7) = 0, end of input.
        let src = [0x00, 1, 2, 3, 4, 5, 6, 7, 8, 0x41, 0x00];
        let out = decompress(&src, 12, 0).unwrap();
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8, 7, 8, 7, 8]);
    }

    #[test]
    fn corrupt_input_is_an_error() {
        // Literal run longer than the input.
        assert!(decompress(&[0x05, 1, 2], 100, 0).is_err());
        // Output too small.
        assert!(decompress(&[0x00, 1, 2, 3, 4, 5, 6, 7, 8], 4, 0).is_err());
        // Back-reference before start.
        assert!(decompress(&[0x00, 1, 2, 3, 4, 5, 6, 7, 8, 0x41, 0xF8], 64, 0).is_err());
        assert!(decompress(&[], 3, 0).unwrap().is_empty());
    }

    #[test]
    fn huge_declared_size_allocates_only_what_is_produced() {
        // A 10-byte literal run with a declared size of 4 GiB.
        let src = [0x02, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let out = decompress(&src, u32::MAX as usize, 0).unwrap();
        assert_eq!(out.len(), 10);
        assert!(out.capacity() <= 1 << 20);
    }
}
