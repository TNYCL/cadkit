//! The R2004 LZ77 variant (ODA spec §4.7), also used by R2010+.
//!
//! Two details differ from the spec text; ACadSharp's `DwgLZ77AC18Decompressor` (MIT)
//! reads them the same way and the sample files confirm them (see
//! `docs/dwg/FORMAT_NOTES.md`):
//! - every back-reference distance is the encoded offset **plus one**;
//! - for opcodes 0x12–0x1F only the low three bits give the match length, bit 3 is
//!   bit 14 of the distance (spec erratum: it says `(opcode1 & 0x0F) + 2`).
//!
//! Every input read and every output write is bounds-checked; the output never
//! grows beyond the caller's declared size.

use cadkit_core::{Error, Result};

struct Input<'a> {
    data: &'a [u8],
    pos: usize,
    base: u64,
}

impl Input<'_> {
    fn next(&mut self) -> Result<u8> {
        let byte = self.data.get(self.pos).copied().ok_or(Error::Truncated {
            offset: self.base.saturating_add(self.pos as u64),
            needed: 1,
        })?;
        self.pos += 1;
        Ok(byte)
    }

    fn offset(&self) -> u64 {
        self.base.saturating_add(self.pos as u64)
    }

    /// Literal length after a byte with a zero high nibble (§4.7 "Literal Length").
    fn literal_length(&mut self, code: u8) -> Result<usize> {
        let mut total = usize::from(code & 0x0F);
        if total == 0 {
            loop {
                let byte = self.next()?;
                if byte == 0 {
                    total = total.saturating_add(0xFF);
                } else {
                    total = total.saturating_add(0x0F + usize::from(byte));
                    break;
                }
            }
        }
        Ok(total.saturating_add(3))
    }

    /// Match length of opcodes 0x10–0x3F, where `mask` selects the length bits.
    fn match_length(&mut self, opcode: u8, mask: u8) -> Result<usize> {
        let mut total = usize::from(opcode & mask);
        if total == 0 {
            loop {
                let byte = self.next()?;
                if byte == 0 {
                    total = total.saturating_add(0xFF);
                } else {
                    total = total.saturating_add(usize::from(byte) + usize::from(mask));
                    break;
                }
            }
        }
        Ok(total.saturating_add(2))
    }

    /// "Two Byte Offset": returns (offset bits, first byte whose low 2 bits are the literal count).
    fn two_byte_offset(&mut self) -> Result<(usize, u8)> {
        let first = self.next()?;
        let second = self.next()?;
        Ok((
            (usize::from(first) >> 2) | (usize::from(second) << 6),
            first,
        ))
    }
}

struct Output {
    data: Vec<u8>,
    limit: usize,
}

impl Output {
    fn literal(&mut self, input: &mut Input<'_>, count: usize) -> Result<()> {
        let end = input
            .pos
            .checked_add(count)
            .filter(|&e| e <= input.data.len())
            .ok_or(Error::Truncated {
                offset: input.offset(),
                needed: count.saturating_sub(input.data.len().saturating_sub(input.pos)) as u64,
            })?;
        if self.data.len().saturating_add(count) > self.limit {
            return Err(Error::invalid(
                input.offset(),
                "LZ77 output exceeds the declared size",
            ));
        }
        let slice = input
            .data
            .get(input.pos..end)
            .ok_or_else(|| Error::invalid(input.offset(), "literal run"))?;
        self.data.extend_from_slice(slice);
        input.pos = end;
        Ok(())
    }

    fn copy_back(&mut self, at: u64, distance: usize, count: usize) -> Result<()> {
        if distance == 0 || distance > self.data.len() {
            return Err(Error::invalid(
                at,
                format!("LZ77 back-reference {distance} before start of output"),
            ));
        }
        if self.data.len().saturating_add(count) > self.limit {
            return Err(Error::invalid(at, "LZ77 output exceeds the declared size"));
        }
        // Byte by byte: the source may overlap the bytes being written (runs).
        let start = self.data.len() - distance;
        for src in start..start + count {
            let byte = self.data.get(src).copied().unwrap_or(0);
            self.data.push(byte);
        }
        Ok(())
    }
}

/// Decompresses `src`, producing at most `max_out` bytes. `base` is the absolute
/// offset of `src` in the file, used in error messages.
pub fn decompress(src: &[u8], max_out: usize, base: u64) -> Result<Vec<u8>> {
    let mut input = Input {
        data: src,
        pos: 0,
        base,
    };
    // Cap the initial reservation: `max_out` comes from the file.
    let mut out = Output {
        data: Vec::with_capacity(max_out.min(1 << 20)),
        limit: max_out,
    };

    let mut opcode = input.next()?;
    if opcode > 0x11 {
        // A first byte above 0x11 starts with a literal run of `opcode - 17` bytes. Not
        // in the spec; taken from ACadSharp's `DwgLZ77AC18Decompressor.DecompressToDest`
        // (MIT). No sample page starts this way; files starting with a regular literal
        // length or match are unaffected.
        out.literal(&mut input, usize::from(opcode - 0x11))?;
        opcode = input.next()?;
    }
    if opcode & 0xF0 == 0 {
        let count = input.literal_length(opcode)?;
        out.literal(&mut input, count)?;
        opcode = input.next()?;
    }

    while opcode != 0x11 {
        let at = input.offset();
        let (count, distance, lit_source) = match opcode {
            0x00..=0x0F => {
                return Err(Error::invalid(
                    at,
                    format!("LZ77 opcode {opcode:#04x} is not used"),
                ));
            }
            0x10..=0x1F => {
                let count = input.match_length(opcode, 0x07)?;
                let (bits, first) = input.two_byte_offset()?;
                let distance = (usize::from(opcode & 0x08) << 11 | bits) + 0x4000;
                (count, distance, first)
            }
            0x20..=0x3F => {
                let count = input.match_length(opcode, 0x1F)?;
                let (bits, first) = input.two_byte_offset()?;
                (count, bits + 1, first)
            }
            _ => {
                let count = usize::from(opcode >> 4) - 1;
                let second = input.next()?;
                let distance = (usize::from((opcode >> 2) & 3) | (usize::from(second) << 2)) + 1;
                (count, distance, opcode)
            }
        };
        out.copy_back(at, distance, count)?;

        let mut literals = usize::from(lit_source & 3);
        if literals == 0 {
            opcode = input.next()?;
            if opcode & 0xF0 == 0 {
                literals = input.literal_length(opcode)?;
            }
        }
        if literals > 0 {
            out.literal(&mut input, literals)?;
            opcode = input.next()?;
        }
    }
    Ok(out.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_then_short_match() {
        // 0x01: literal length 4; 0x5C 0x00: copy 4 bytes from distance 4, no literals;
        // 0x11: end.
        let src = [0x01, b'a', b'b', b'c', b'd', 0x5C, 0x00, 0x11];
        assert_eq!(decompress(&src, 100, 0).unwrap(), b"abcdabcd");
    }

    #[test]
    fn long_literal_length() {
        // 0x00 then 0x01: running total 0x0F + 1, plus 3 -> 19 literal bytes.
        let mut src = vec![0x00, 0x01];
        src.extend(1..=19u8);
        src.push(0x11);
        assert_eq!(
            decompress(&src, 100, 0).unwrap(),
            (1..=19u8).collect::<Vec<_>>()
        );
        // Each extra 0x00 adds 0xFF.
        let mut src = vec![0x00, 0x00, 0x01];
        src.extend(std::iter::repeat_n(7u8, 0x0F + 0xFF + 1 + 3));
        src.push(0x11);
        assert_eq!(
            decompress(&src, 1000, 0).unwrap().len(),
            0x0F + 0xFF + 1 + 3
        );
    }

    #[test]
    fn match_with_literal_bits_in_opcode() {
        // "abcd" (literal length 4); opcode 0x71: (0x7 - 1) = 6 bytes, offset bits
        // ((0x71 >> 2) & 3) = 0 and second byte 0 -> distance 1, literal count
        // (0x71 & 3) = 1 -> "z"; then 0x11.
        let src = [0x01, b'a', b'b', b'c', b'd', 0x71, 0x00, b'z', 0x11];
        assert_eq!(decompress(&src, 100, 0).unwrap(), b"abcdddddddz");
    }

    #[test]
    fn two_byte_offset_opcodes() {
        // 0x22: 0x22 - 0x1E = 4 bytes; two-byte offset (first = 3 << 2 = 0x0C, second 0)
        // -> distance 4; literal bits of first byte = 0 -> next opcode 0x11.
        let src = [0x01, b'w', b'x', b'y', b'z', 0x22, 0x0C, 0x00, 0x11];
        assert_eq!(decompress(&src, 100, 0).unwrap(), b"wxyzwxyz");
    }

    #[test]
    fn corrupt_streams_are_errors_not_panics() {
        // Back-reference before the start.
        assert!(decompress(&[0x01, 1, 2, 3, 4, 0xF0, 0xFF, 0x11], 100, 0).is_err());
        // Output larger than declared.
        assert!(decompress(&[0x01, 1, 2, 3, 4, 0x11], 3, 0).is_err());
        // Truncated input.
        assert!(decompress(&[0x05, 1, 2], 100, 0).is_err());
        assert!(decompress(&[], 100, 0).is_err());
        // Unused opcode inside the stream.
        assert!(decompress(&[0x01, 1, 2, 3, 4, 0x41, 0x00, 0x05], 100, 0).is_err());
        // Endless zero run in a literal length must stop at the end of input.
        let mut src = vec![0x00];
        src.extend(std::iter::repeat_n(0u8, 10_000));
        assert!(decompress(&src, 100, 0).is_err());
    }
}
