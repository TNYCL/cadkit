//! Bounds-checked little-endian reader for untrusted input.
//!
//! Every read either succeeds or returns [`Error::Truncated`]; nothing here can panic.
//! Offsets in errors are absolute: a sub-reader remembers its base offset.

use crate::error::{Error, Result};

/// Cursor over a byte slice.
#[derive(Debug, Clone)]
pub struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
    base: u64,
}

macro_rules! read_le {
    ($($name:ident -> $ty:ty),* $(,)?) => {$(
        #[doc = concat!("Reads a little-endian `", stringify!($ty), "`.")]
        pub fn $name(&mut self) -> Result<$ty> {
            let bytes = self.array::<{ std::mem::size_of::<$ty>() }>()?;
            Ok(<$ty>::from_le_bytes(bytes))
        }
    )*};
}

impl<'a> ByteReader<'a> {
    /// Reader at the start of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            base: 0,
        }
    }

    /// Reader whose error offsets start at `base` (for data cut out of a larger stream).
    pub fn with_base(data: &'a [u8], base: u64) -> Self {
        Self { data, pos: 0, base }
    }

    /// Current position relative to the start of this reader.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Current absolute offset (base + position), for diagnostics.
    pub fn offset(&self) -> u64 {
        self.base.saturating_add(self.pos as u64)
    }

    /// Total length of the underlying slice.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// True when the underlying slice is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Bytes left after the current position.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// True when no bytes are left.
    pub fn at_end(&self) -> bool {
        self.remaining() == 0
    }

    /// The unread part of the data.
    pub fn rest(&self) -> &'a [u8] {
        self.data.get(self.pos..).unwrap_or(&[])
    }

    /// Moves to an absolute position within this reader.
    pub fn seek(&mut self, pos: usize) -> Result<()> {
        if pos > self.data.len() {
            return Err(self.truncated(pos.saturating_sub(self.data.len())));
        }
        self.pos = pos;
        Ok(())
    }

    /// Skips `n` bytes.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    /// Takes the next `n` bytes.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        match end.and_then(|e| self.data.get(self.pos..e).map(|s| (s, e))) {
            Some((slice, e)) => {
                self.pos = e;
                Ok(slice)
            }
            None => Err(self.truncated(n.saturating_sub(self.remaining()))),
        }
    }

    /// Takes the next `n` bytes as a new reader that keeps absolute error offsets.
    pub fn sub(&mut self, n: usize) -> Result<ByteReader<'a>> {
        let base = self.offset();
        let data = self.bytes(n)?;
        Ok(ByteReader::with_base(data, base))
    }

    /// Reads a fixed-size array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let slice = self.bytes(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    /// Looks at the next `n` bytes without consuming them.
    pub fn peek(&self, n: usize) -> Result<&'a [u8]> {
        self.pos
            .checked_add(n)
            .and_then(|e| self.data.get(self.pos..e))
            .ok_or_else(|| self.truncated(n.saturating_sub(self.remaining())))
    }

    /// Reads one byte.
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    /// Reads one signed byte.
    pub fn i8(&mut self) -> Result<i8> {
        Ok(i8::from_le_bytes(self.array::<1>()?))
    }

    read_le! {
        u16 -> u16, i16 -> i16, u32 -> u32, i32 -> i32,
        u64 -> u64, i64 -> i64, f32 -> f32, f64 -> f64,
    }

    /// Reads a little-endian `u32` and converts it to `usize`.
    pub fn u32_usize(&mut self) -> Result<usize> {
        let at = self.offset();
        let v = self.u32()?;
        usize::try_from(v).map_err(|_| Error::invalid(at, "length does not fit in usize"))
    }

    /// Error for a read that needed `needed` more bytes at the current position.
    pub fn truncated(&self, needed: usize) -> Error {
        Error::Truncated {
            offset: self.offset(),
            needed: needed as u64,
        }
    }

    /// [`Error::Invalid`] at the current offset.
    pub fn invalid(&self, message: impl Into<String>) -> Error {
        Error::invalid(self.offset(), message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_values() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let mut r = ByteReader::new(&data);
        assert_eq!(r.u16().unwrap(), 0x0201);
        assert_eq!(r.u32().unwrap(), 0x0605_0403);
        assert_eq!(r.remaining(), 2);
    }

    #[test]
    fn truncation_is_an_error_not_a_panic() {
        let mut r = ByteReader::with_base(&[1, 2, 3], 100);
        r.skip(2).unwrap();
        match r.u32() {
            Err(Error::Truncated { offset, needed }) => {
                assert_eq!(offset, 102);
                assert_eq!(needed, 3);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(r.pos(), 2, "failed read must not move the cursor");
        assert!(r.bytes(usize::MAX).is_err());
        assert!(r.seek(4).is_err());
    }

    #[test]
    fn sub_reader_keeps_absolute_offsets() {
        let data = [0u8; 16];
        let mut r = ByteReader::with_base(&data, 1000);
        r.skip(4).unwrap();
        let mut s = r.sub(8).unwrap();
        assert_eq!(s.offset(), 1004);
        s.skip(8).unwrap();
        assert!(matches!(s.u8(), Err(Error::Truncated { offset: 1012, .. })));
        assert_eq!(r.pos(), 12);
    }
}
