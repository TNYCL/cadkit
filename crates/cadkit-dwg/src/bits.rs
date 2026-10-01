//! Bounds-checked bit stream reader for the DWG bit codes (ODA spec chapter 2).
//!
//! DWG object, header and class data is a big-endian *bit* stream: bit 7 of the
//! first byte is read first. Multi-byte raw values embedded in the bit stream
//! (RS, RL, RD, the payload of BS/BL/BD) keep little-endian byte order, but each
//! of their bytes may straddle two file bytes.
//!
//! Every read either succeeds or returns [`Error::Truncated`] / [`Error::Invalid`]
//! with an absolute byte offset; nothing here can panic. A reader is confined to
//! the bit range `[start, end)` it was created with, so a corrupt length can never
//! make it read into a neighbouring object.

use cadkit_core::{Error, Result};

/// A handle reference as stored in the stream: `|CODE (4 bits)|COUNTER (4 bits)|BYTES|`
/// (spec §2.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HandleRef {
    /// Reference code: 2..=5 are absolute (soft/hard owner/pointer); 6, 8, 0xA, 0xC
    /// are offsets relative to a reference handle (usually the object's own handle).
    pub code: u8,
    /// Value bytes as stored (big-endian), zero-extended. For codes 6 and 8 this is 0.
    pub value: u64,
}

impl HandleRef {
    /// Resolves the reference against `reference` (the handle of the object that
    /// contains it). Absolute codes return the stored value. Returns `None` for
    /// codes the spec does not define.
    pub fn resolve(self, reference: u64) -> Option<u64> {
        match self.code {
            0..=5 => Some(self.value),
            6 => reference.checked_add(1),
            8 => reference.checked_sub(1),
            0xA => reference.checked_add(self.value),
            0xC => reference.checked_sub(self.value),
            _ => None,
        }
    }
}

/// Cursor over a bit range of a byte slice.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Current bit position, counted from bit 7 of `data[0]`.
    pos: u64,
    /// Exclusive bit limit.
    end: u64,
    /// Absolute byte offset of `data[0]`, for error messages.
    base: u64,
}

/// Number of bits in `len` bytes, saturating (slices never get that large).
fn bits_of(len: usize) -> u64 {
    (len as u64).saturating_mul(8)
}

impl<'a> BitReader<'a> {
    /// Reader over all bits of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            end: bits_of(data.len()),
            base: 0,
        }
    }

    /// Reader over all bits of `data`, reporting offsets relative to `base`.
    pub fn with_base(data: &'a [u8], base: u64) -> Self {
        Self {
            data,
            pos: 0,
            end: bits_of(data.len()),
            base,
        }
    }

    /// A reader over the same data restricted to bits `[start, end)`.
    /// The range is clamped to the data.
    pub fn range(&self, start: u64, end: u64) -> Self {
        let limit = bits_of(self.data.len());
        let end = end.min(limit);
        let start = start.min(end);
        Self {
            data: self.data,
            pos: start,
            end,
            base: self.base,
        }
    }

    /// The underlying bytes.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Current bit position.
    pub fn bit_pos(&self) -> u64 {
        self.pos
    }

    /// Exclusive bit limit of this reader.
    pub fn end_bit(&self) -> u64 {
        self.end
    }

    /// Bits left before the limit.
    pub fn remaining_bits(&self) -> u64 {
        self.end.saturating_sub(self.pos)
    }

    /// Absolute byte offset of the current position, for diagnostics.
    pub fn offset(&self) -> u64 {
        self.base.saturating_add(self.pos / 8)
    }

    /// Moves to an absolute bit position (must not exceed the limit).
    pub fn seek_bit(&mut self, pos: u64) -> Result<()> {
        if pos > self.end {
            return Err(Error::Truncated {
                offset: self.base.saturating_add(self.end / 8),
                needed: (pos - self.end).div_ceil(8),
            });
        }
        self.pos = pos;
        Ok(())
    }

    /// Lowers the bit limit (it can never be raised).
    pub fn set_end_bit(&mut self, end: u64) {
        self.end = end.min(self.end).max(self.pos.min(self.end));
    }

    /// Skips to the next byte boundary.
    pub fn align_byte(&mut self) {
        let aligned = self.pos.div_ceil(8).saturating_mul(8);
        self.pos = aligned.min(self.end);
    }

    fn need(&self, bits: u64) -> Result<()> {
        if self.remaining_bits() < bits {
            return Err(Error::Truncated {
                offset: self.offset(),
                needed: (bits - self.remaining_bits()).div_ceil(8),
            });
        }
        Ok(())
    }

    /// Error for invalid data at the current position.
    pub fn invalid(&self, message: impl Into<String>) -> Error {
        Error::invalid(self.offset(), message)
    }

    /// Reads up to 64 bits, most significant first. Bounds are checked by the caller.
    fn take(&mut self, n: u32) -> u64 {
        let mut out = 0u64;
        let mut left = n;
        while left > 0 {
            let byte_index = usize::try_from(self.pos / 8).unwrap_or(usize::MAX);
            let byte = self.data.get(byte_index).copied().unwrap_or(0);
            let bit_in_byte = (self.pos % 8) as u32;
            let avail = 8 - bit_in_byte;
            let count = avail.min(left);
            let shifted = u64::from(byte) >> (avail - count);
            let mask = (1u64 << count) - 1;
            out = (out << count) | (shifted & mask);
            self.pos += u64::from(count);
            left -= count;
        }
        out
    }

    /// Reads `n` (≤ 64) bits as an unsigned number, first bit most significant.
    pub fn bits(&mut self, n: u32) -> Result<u64> {
        if n > 64 {
            return Err(self.invalid("bit count above 64"));
        }
        self.need(u64::from(n))?;
        Ok(self.take(n))
    }

    /// `B`: one bit.
    pub fn b(&mut self) -> Result<bool> {
        Ok(self.bits(1)? == 1)
    }

    /// `BB`: two bits.
    pub fn bb(&mut self) -> Result<u8> {
        Ok(self.bits(2)? as u8)
    }

    /// `3B`: one to three bits, stopping after the first zero bit (spec §2.1).
    pub fn b3(&mut self) -> Result<u8> {
        let mut value = 0u8;
        for _ in 0..3 {
            let bit = self.b()?;
            value = (value << 1) | u8::from(bit);
            if !bit {
                break;
            }
        }
        Ok(value)
    }

    /// `RC`: a raw byte (not necessarily byte aligned).
    pub fn rc(&mut self) -> Result<u8> {
        Ok(self.bits(8)? as u8)
    }

    /// `RS`: raw little-endian 16-bit value.
    pub fn rs(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array::<2>()?))
    }

    /// `RL`: raw little-endian 32-bit value.
    pub fn rl(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }

    /// Raw little-endian 64-bit value.
    pub fn rll(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array::<8>()?))
    }

    /// `RD`: raw little-endian IEEE double.
    pub fn rd(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.array::<8>()?))
    }

    /// `2RD`.
    pub fn rd2(&mut self) -> Result<[f64; 2]> {
        Ok([self.rd()?, self.rd()?])
    }

    /// `3RD`.
    pub fn rd3(&mut self) -> Result<[f64; 3]> {
        Ok([self.rd()?, self.rd()?, self.rd()?])
    }

    /// Reads `N` raw bytes.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.need(bits_of(N))?;
        let mut out = [0u8; N];
        for byte in &mut out {
            *byte = self.take(8) as u8;
        }
        Ok(out)
    }

    /// Reads `n` raw bytes into a new vector. `n` is checked against the remaining
    /// bits before anything is allocated.
    pub fn bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        self.need(bits_of(n))?;
        let mut out = Vec::with_capacity(n);
        if self.pos % 8 == 0 {
            let start =
                usize::try_from(self.pos / 8).map_err(|_| self.invalid("position overflow"))?;
            let slice = start
                .checked_add(n)
                .and_then(|end| self.data.get(start..end))
                .ok_or_else(|| self.invalid("byte range outside data"))?;
            out.extend_from_slice(slice);
            self.pos += bits_of(n);
        } else {
            for _ in 0..n {
                out.push(self.take(8) as u8);
            }
        }
        Ok(out)
    }

    /// Skips `n` bits.
    pub fn skip_bits(&mut self, n: u64) -> Result<()> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }

    /// `BS`: bit short (spec §2.2). `00` short follows, `01` unsigned char, `10` 0, `11` 256.
    pub fn bs(&mut self) -> Result<i16> {
        Ok(match self.bb()? {
            0 => self.rs()? as i16,
            1 => i16::from(self.rc()?),
            2 => 0,
            _ => 256,
        })
    }

    /// `BL`: bit long (spec §2.3). The `11` code is unused and rejected.
    pub fn bl(&mut self) -> Result<i32> {
        match self.bb()? {
            0 => Ok(self.rl()? as i32),
            1 => Ok(i32::from(self.rc()?)),
            2 => Ok(0),
            _ => Err(self.invalid("BL with unused code 11")),
        }
    }

    /// `BLL`: bit long long (spec §2.4): a byte count, then that many bytes LSB first.
    /// The count is a plain 3-bit field; the spec's reference to the variable-length
    /// `3B` code (§2.1) is an erratum (R2013+ header and R2010+ proxy graphics sizes
    /// only decode with 3 fixed bits).
    pub fn bll(&mut self) -> Result<u64> {
        let len = self.bits(3)? as u8;
        let mut value = 0u64;
        for i in 0..u32::from(len) {
            value |= u64::from(self.rc()?) << (8 * i);
        }
        Ok(value)
    }

    /// `BD`: bit double (spec §2.5). The `11` code is unused and rejected.
    pub fn bd(&mut self) -> Result<f64> {
        match self.bb()? {
            0 => self.rd(),
            1 => Ok(1.0),
            2 => Ok(0.0),
            _ => Err(self.invalid("BD with unused code 11")),
        }
    }

    /// `2BD`.
    pub fn bd2(&mut self) -> Result<[f64; 2]> {
        Ok([self.bd()?, self.bd()?])
    }

    /// `3BD`.
    pub fn bd3(&mut self) -> Result<[f64; 3]> {
        Ok([self.bd()?, self.bd()?, self.bd()?])
    }

    /// `DD`: bit double with default (spec §2.9). The patched bytes replace parts of
    /// the little-endian representation of `default`.
    pub fn dd(&mut self, default: f64) -> Result<f64> {
        let mut bytes = default.to_le_bytes();
        match self.bb()? {
            0 => return Ok(default),
            1 => {
                let patch = self.array::<4>()?;
                bytes[..4].copy_from_slice(&patch);
            }
            2 => {
                let patch = self.array::<6>()?;
                bytes[4] = patch[0];
                bytes[5] = patch[1];
                bytes[..4].copy_from_slice(&patch[2..]);
            }
            _ => return self.rd(),
        }
        Ok(f64::from_le_bytes(bytes))
    }

    /// `2DD` with per-axis defaults.
    pub fn dd2(&mut self, default: [f64; 2]) -> Result<[f64; 2]> {
        Ok([self.dd(default[0])?, self.dd(default[1])?])
    }

    /// `BT`: bit thickness (spec §2.10). R13/R14 store a plain BD; R2000+ a flag bit
    /// that, when set, means 0.0.
    pub fn bt(&mut self, r2000_plus: bool) -> Result<f64> {
        if r2000_plus && self.b()? {
            return Ok(0.0);
        }
        self.bd()
    }

    /// `BE`: bit extrusion (spec §2.8). R13/R14 store 3BD; R2000+ a flag bit that,
    /// when set, means (0, 0, 1).
    pub fn be(&mut self, r2000_plus: bool) -> Result<[f64; 3]> {
        if r2000_plus && self.b()? {
            return Ok([0.0, 0.0, 1.0]);
        }
        self.bd3()
    }

    /// `MC`: signed modular char (spec §2.6). Groups of 7 bits, least significant
    /// first; in the last byte bit 0x40 is the sign and only 6 bits carry value.
    pub fn mc(&mut self) -> Result<i64> {
        let mut value: i64 = 0;
        let mut shift = 0u32;
        loop {
            let byte = self.rc()?;
            if byte & 0x80 == 0 {
                let last = i64::from(byte & 0x3F);
                if shift < 63 {
                    value |= last << shift;
                }
                return Ok(if byte & 0x40 != 0 { -value } else { value });
            }
            if shift < 63 {
                value |= i64::from(byte & 0x7F) << shift;
            }
            shift += 7;
            if shift > 63 {
                return Err(self.invalid("modular char longer than 64 bits"));
            }
        }
    }

    /// Unsigned modular char: like `MC` without the sign bit (used for handle
    /// offsets in the object map and the R2010+ handle stream size).
    pub fn umc(&mut self) -> Result<u64> {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let byte = self.rc()?;
            if shift < 64 {
                value |= u64::from(byte & 0x7F) << shift;
            }
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift > 63 {
                return Err(self.invalid("modular char longer than 64 bits"));
            }
        }
    }

    /// `MS`: modular short (spec §2.7). Little-endian 16-bit modules, 15 value bits
    /// each, bit 15 set when another module follows.
    pub fn ms(&mut self) -> Result<u64> {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let module = self.rs()?;
            if shift < 64 {
                value |= u64::from(module & 0x7FFF) << shift;
            }
            if module & 0x8000 == 0 {
                return Ok(value);
            }
            shift += 15;
            if shift > 63 {
                return Err(self.invalid("modular short longer than 64 bits"));
            }
        }
    }

    /// `H`: handle reference (spec §2.13).
    pub fn handle(&mut self) -> Result<HandleRef> {
        let form = self.rc()?;
        let code = form >> 4;
        let counter = form & 0x0F;
        if counter > 8 {
            return Err(self.invalid(format!("handle with {counter} value bytes")));
        }
        let mut value = 0u64;
        for _ in 0..counter {
            value = (value << 8) | u64::from(self.rc()?);
        }
        Ok(HandleRef { code, value })
    }

    /// Reads a handle reference and resolves it against `reference`.
    pub fn handle_abs(&mut self, reference: u64) -> Result<u64> {
        let at = self.offset();
        let h = self.handle()?;
        h.resolve(reference)
            .ok_or_else(|| Error::invalid(at, format!("invalid handle code {:#x}", h.code)))
    }

    /// `OT`: object type. Before R2010 a `BS`; from R2010 a bit pair followed by one
    /// or two bytes (spec §2.12).
    pub fn ot(&mut self, r2010_plus: bool) -> Result<u16> {
        if !r2010_plus {
            return Ok(self.bs()? as u16);
        }
        Ok(match self.bb()? {
            0 => u16::from(self.rc()?),
            1 => 0x1F0 + u16::from(self.rc()?),
            _ => self.rs()?,
        })
    }

    /// `T` (pre-R2007 `TV`): a `BS` byte length followed by 8-bit characters.
    /// Returns the raw bytes; decoding needs the drawing code page.
    pub fn t_bytes(&mut self, max_len: u32) -> Result<Vec<u8>> {
        let len = self.bs()?;
        let len = u16::try_from(len).map_err(|_| self.invalid("negative string length"))?;
        if u32::from(len) > max_len {
            return Err(Error::LimitExceeded(format!("string of {len} bytes")));
        }
        self.bytes(usize::from(len))
    }

    /// `TU` (R2007+ `TV`): a `BS` character count followed by UTF-16LE code units.
    /// Trailing NUL characters are removed.
    pub fn tu(&mut self, max_len: u32) -> Result<String> {
        let len = self.bs()?;
        let len = u16::try_from(len).map_err(|_| self.invalid("negative string length"))?;
        if u32::from(len).saturating_mul(2) > max_len {
            return Err(Error::LimitExceeded(format!("string of {len} characters")));
        }
        let mut units = Vec::with_capacity(usize::from(len));
        for _ in 0..len {
            units.push(self.rs()?);
        }
        while units.last() == Some(&0) {
            units.pop();
        }
        Ok(String::from_utf16_lossy(&units))
    }

    /// Reads a 16-byte sentinel and checks it against `expected`.
    pub fn sentinel(&mut self, expected: &[u8; 16]) -> Result<()> {
        let at = self.offset();
        let got = self.array::<16>()?;
        if &got != expected {
            return Err(Error::invalid(at, "sentinel mismatch"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a byte vector from a string of '0'/'1' characters (other characters
    /// are separators), padding the last byte with zeros.
    fn bits(pattern: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cur = 0u8;
        let mut n = 0;
        for c in pattern.chars() {
            let bit = match c {
                '0' => 0,
                '1' => 1,
                _ => continue,
            };
            cur = (cur << 1) | bit;
            n += 1;
            if n == 8 {
                out.push(cur);
                cur = 0;
                n = 0;
            }
        }
        if n > 0 {
            out.push(cur << (8 - n));
        }
        out
    }

    #[test]
    fn bitshort_example_from_spec() {
        // Spec §2.2: 00 00000001 00000001 | 10 | 11 | 01 00001111 | 10
        let data = bits("0000000001000000011011010000111110");
        let mut r = BitReader::new(&data);
        assert_eq!(r.bs().unwrap(), 257);
        assert_eq!(r.bs().unwrap(), 0);
        assert_eq!(r.bs().unwrap(), 256);
        assert_eq!(r.bs().unwrap(), 15);
        assert_eq!(r.bs().unwrap(), 0);
    }

    #[test]
    fn bitshort_negative_uses_short_form() {
        let data = bits("00 11111111 11111111");
        assert_eq!(BitReader::new(&data).bs().unwrap(), -1);
    }

    #[test]
    fn bitlong_example_from_spec() {
        // Spec §2.3: 00 + 4 bytes (257) | 10 | 01 00001111 | 10
        let data = bits("00 00000001 00000001 00000000 00000000 10 01 00001111 10");
        let mut r = BitReader::new(&data);
        assert_eq!(r.bl().unwrap(), 257);
        assert_eq!(r.bl().unwrap(), 0);
        assert_eq!(r.bl().unwrap(), 15);
        assert_eq!(r.bl().unwrap(), 0);
        let bad = bits("11");
        assert!(BitReader::new(&bad).bl().is_err());
    }

    #[test]
    fn bitdouble_codes() {
        let mut pattern = String::from("00");
        for byte in 2.5f64.to_le_bytes() {
            pattern.push_str(&format!("{byte:08b}"));
        }
        pattern.push_str("01 10");
        let data = bits(&pattern);
        let mut r = BitReader::new(&data);
        assert_eq!(r.bd().unwrap(), 2.5);
        assert_eq!(r.bd().unwrap(), 1.0);
        assert_eq!(r.bd().unwrap(), 0.0);
        assert!(BitReader::new(&bits("11")).bd().is_err());
    }

    #[test]
    fn three_bit_code() {
        let data = bits("0 10 110 111");
        let mut r = BitReader::new(&data);
        assert_eq!(r.b3().unwrap(), 0);
        assert_eq!(r.b3().unwrap(), 2);
        assert_eq!(r.b3().unwrap(), 6);
        assert_eq!(r.b3().unwrap(), 7);
    }

    #[test]
    fn bitlonglong_uses_fixed_3_bit_length() {
        // length 2 ("010"), bytes 0x34 0x12 -> 0x1234; then length 0 ("000").
        let data = bits("010 00110100 00010010 000");
        let mut r = BitReader::new(&data);
        assert_eq!(r.bll().unwrap(), 0x1234);
        assert_eq!(r.bll().unwrap(), 0);
        assert_eq!(r.bit_pos(), 3 + 16 + 3);
    }

    #[test]
    fn modular_char_examples_from_spec() {
        let mut r = BitReader::new(&[0b1000_0010, 0b0010_0100]);
        assert_eq!(r.mc().unwrap(), 4610);
        let mut r = BitReader::new(&[0b1110_1001, 0b1001_0111, 0b1110_0110, 0b0011_0101]);
        assert_eq!(r.mc().unwrap(), 112_823_273);
        let mut r = BitReader::new(&[0b1000_0101, 0b0100_1011]);
        assert_eq!(r.mc().unwrap(), -1413);
        // Unsigned form keeps bit 0x40 as value.
        let mut r = BitReader::new(&[0b1000_0101, 0b0100_1011]);
        assert_eq!(r.umc().unwrap(), 5 | (0x4B << 7));
    }

    #[test]
    fn modular_short_example_from_spec() {
        let mut r = BitReader::new(&[0b0011_0001, 0b1111_0100, 0b1000_1101, 0b0000_0000]);
        assert_eq!(r.ms().unwrap(), 4_650_033);
        let mut r = BitReader::new(&[0x34, 0x12]);
        assert_eq!(r.ms().unwrap(), 0x1234);
    }

    #[test]
    fn modular_codes_reject_runaway_continuation() {
        assert!(BitReader::new(&[0xFF; 20]).mc().is_err());
        assert!(BitReader::new(&[0xFF; 20]).umc().is_err());
        assert!(BitReader::new(&[0xFF; 20]).ms().is_err());
    }

    #[test]
    fn handle_reference_example_from_spec() {
        // Spec §2.13: 0101.0010.00000101.11100111 -> code 5, handle 0x5E7.
        let mut r = BitReader::new(&[0b0101_0010, 0b0000_0101, 0b1110_0111]);
        let h = r.handle().unwrap();
        assert_eq!(
            h,
            HandleRef {
                code: 5,
                value: 0x5E7
            }
        );
        assert_eq!(h.resolve(0), Some(0x5E7));
    }

    #[test]
    fn relative_handle_codes() {
        assert_eq!(HandleRef { code: 6, value: 0 }.resolve(0x10), Some(0x11));
        assert_eq!(HandleRef { code: 8, value: 0 }.resolve(0x10), Some(0x0F));
        assert_eq!(
            HandleRef {
                code: 0xA,
                value: 5
            }
            .resolve(0x10),
            Some(0x15)
        );
        assert_eq!(
            HandleRef {
                code: 0xC,
                value: 5
            }
            .resolve(0x10),
            Some(0x0B)
        );
        assert_eq!(HandleRef { code: 8, value: 0 }.resolve(0), None);
        assert_eq!(
            HandleRef {
                code: 0xF,
                value: 0
            }
            .resolve(1),
            None
        );
        // Counter above 8 bytes is rejected.
        assert!(
            BitReader::new(&[0x59, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .handle()
                .is_err()
        );
    }

    #[test]
    fn default_double_patches_bytes() {
        let default = 1.0f64;
        let d = default.to_le_bytes();
        // 00: default.
        assert_eq!(BitReader::new(&bits("00")).dd(default).unwrap(), default);
        // 01: four bytes replace bytes 0..4.
        let data = bits("01 00000001 00000010 00000011 00000100");
        let mut expect = d;
        expect[..4].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(
            BitReader::new(&data).dd(default).unwrap(),
            f64::from_le_bytes(expect)
        );
        // 10: two bytes replace bytes 4..6, then four replace bytes 0..4.
        let data = bits("10 00001010 00001011 00000001 00000010 00000011 00000100");
        let mut expect = d;
        expect[4] = 10;
        expect[5] = 11;
        expect[..4].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(
            BitReader::new(&data).dd(default).unwrap(),
            f64::from_le_bytes(expect)
        );
        // 11: full RD.
        let mut pattern = String::from("11");
        for byte in (-3.25f64).to_le_bytes() {
            pattern.push_str(&format!("{byte:08b}"));
        }
        assert_eq!(BitReader::new(&bits(&pattern)).dd(default).unwrap(), -3.25);
    }

    #[test]
    fn thickness_and_extrusion_by_version() {
        assert_eq!(BitReader::new(&bits("1")).bt(true).unwrap(), 0.0);
        assert_eq!(BitReader::new(&bits("0 01")).bt(true).unwrap(), 1.0);
        assert_eq!(BitReader::new(&bits("01")).bt(false).unwrap(), 1.0);
        assert_eq!(
            BitReader::new(&bits("1")).be(true).unwrap(),
            [0.0, 0.0, 1.0]
        );
        assert_eq!(
            BitReader::new(&bits("0 10 10 01")).be(true).unwrap(),
            [0.0, 0.0, 1.0]
        );
        assert_eq!(
            BitReader::new(&bits("01 10 10")).be(false).unwrap(),
            [1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn object_type_encodings() {
        // Pre-R2010: BS.
        assert_eq!(
            BitReader::new(&bits("01 00010011")).ot(false).unwrap(),
            0x13
        );
        // R2010+: 00 + byte, 01 + byte + 0x1F0, 10 + raw short.
        assert_eq!(BitReader::new(&bits("00 00010011")).ot(true).unwrap(), 0x13);
        assert_eq!(
            BitReader::new(&bits("01 00000100")).ot(true).unwrap(),
            0x1F4
        );
        assert_eq!(
            BitReader::new(&bits("10 11110100 00000001"))
                .ot(true)
                .unwrap(),
            0x1F4
        );
        assert_eq!(
            BitReader::new(&bits("11 11110100 00000001"))
                .ot(true)
                .unwrap(),
            0x1F4
        );
    }

    #[test]
    fn unaligned_raw_values() {
        // One leading bit, then RS 0x1234 and RL 0xDEADBEEF straddling bytes.
        let mut pattern = String::from("1");
        for byte in 0x1234u16.to_le_bytes() {
            pattern.push_str(&format!("{byte:08b}"));
        }
        for byte in 0xDEAD_BEEFu32.to_le_bytes() {
            pattern.push_str(&format!("{byte:08b}"));
        }
        let data = bits(&pattern);
        let mut r = BitReader::new(&data);
        assert!(r.b().unwrap());
        assert_eq!(r.rs().unwrap(), 0x1234);
        assert_eq!(r.rl().unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn strings() {
        // T: BS length 3 ("01 00000011") then "abc".
        let data = bits("01 00000011 01100001 01100010 01100011");
        assert_eq!(BitReader::new(&data).t_bytes(100).unwrap(), b"abc");
        assert!(BitReader::new(&data).t_bytes(2).is_err());
        // TU: BS length 2, "A" and NUL in UTF-16LE.
        let data = bits("01 00000010 01000001 00000000 00000000 00000000");
        assert_eq!(BitReader::new(&data).tu(100).unwrap(), "A");
    }

    #[test]
    fn reads_never_cross_the_limit() {
        let data = [0xFFu8; 4];
        let mut r = BitReader::new(&data).range(0, 10);
        assert_eq!(r.bits(10).unwrap(), 0x3FF);
        assert!(r.b().is_err());
        let mut r = BitReader::new(&data);
        assert!(r.bits(65).is_err());
        assert!(r.bytes(5).is_err());
        assert_eq!(r.bit_pos(), 0, "failed reads must not move the cursor");
        assert!(r.seek_bit(33).is_err());
        assert!(r.rll().is_err());
        assert!(BitReader::new(&[]).b().is_err());
    }

    #[test]
    fn truncation_offsets_are_absolute() {
        let mut r = BitReader::with_base(&[0u8; 2], 1000);
        r.skip_bits(8).unwrap();
        match r.rl() {
            Err(Error::Truncated { offset, needed }) => {
                assert_eq!(offset, 1001);
                assert_eq!(needed, 3);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn sentinel_check() {
        let s = [7u8; 16];
        assert!(BitReader::new(&s).sentinel(&s).is_ok());
        assert!(BitReader::new(&s).sentinel(&[0u8; 16]).is_err());
    }
}
