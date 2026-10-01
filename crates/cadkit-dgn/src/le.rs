//! Bounds-checked little-endian field access by offset.
//!
//! DGN element layouts are described as fixed offsets into an element, so offset-based
//! accessors read more naturally than a cursor. Every accessor returns `None` when the
//! field does not fit; nothing here can panic.

/// `len` bytes at `off`, or `None` when out of bounds.
pub(crate) fn bytes(b: &[u8], off: usize, len: usize) -> Option<&[u8]> {
    b.get(off..off.checked_add(len)?)
}

/// Fixed-size array at `off`.
pub(crate) fn array<const N: usize>(b: &[u8], off: usize) -> Option<[u8; N]> {
    let s = bytes(b, off, N)?;
    let mut out = [0u8; N];
    out.copy_from_slice(s);
    Some(out)
}

pub(crate) fn u8_at(b: &[u8], off: usize) -> Option<u8> {
    b.get(off).copied()
}

pub(crate) fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    array::<2>(b, off).map(u16::from_le_bytes)
}

pub(crate) fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    array::<4>(b, off).map(u32::from_le_bytes)
}

pub(crate) fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    array::<8>(b, off).map(u64::from_le_bytes)
}

pub(crate) fn i64_at(b: &[u8], off: usize) -> Option<i64> {
    array::<8>(b, off).map(i64::from_le_bytes)
}

/// IEEE double; `None` when out of bounds or not finite.
pub(crate) fn f64_at(b: &[u8], off: usize) -> Option<f64> {
    array::<8>(b, off)
        .map(f64::from_le_bytes)
        .filter(|v| v.is_finite())
}

/// `N` consecutive finite doubles.
pub(crate) fn f64s<const N: usize>(b: &[u8], off: usize) -> Option<[f64; N]> {
    let mut out = [0.0; N];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = f64_at(b, off.checked_add(i.checked_mul(8)?)?)?;
    }
    Some(out)
}

/// A point of 2 (z = 0) or 3 finite doubles.
pub(crate) fn point_at(b: &[u8], off: usize, is_3d: bool) -> Option<[f64; 3]> {
    if is_3d {
        f64s::<3>(b, off)
    } else {
        let [x, y] = f64s::<2>(b, off)?;
        Some([x, y, 0.0])
    }
}

/// V7 32-bit integer: two little-endian 16-bit words, high word first ("middle endian").
pub(crate) fn v7_i32_at(b: &[u8], off: usize) -> Option<i32> {
    let [b0, b1, b2, b3] = array::<4>(b, off)?;
    Some(i32::from_le_bytes([b2, b3, b0, b1]))
}

/// V7 32-bit unsigned value with the same word order as [`v7_i32_at`].
pub(crate) fn v7_u32_at(b: &[u8], off: usize) -> Option<u32> {
    let [b0, b1, b2, b3] = array::<4>(b, off)?;
    Some(u32::from_le_bytes([b2, b3, b0, b1]))
}

/// VAX D-float (as stored by V7 DGN) converted to an IEEE double.
///
/// D-float: four little-endian 16-bit words, most significant word first; sign bit, 8-bit
/// excess-128 exponent and a 55-bit fraction with a hidden leading `0.1` bit. The IEEE
/// value is `1.f * 2^(e - 129)`, so the exponent is rebased by `1023 - 129 = 894` and the
/// fraction is truncated by 3 bits. A zero exponent is zero (VAX has no denormals).
pub(crate) fn vax_f64_at(b: &[u8], off: usize) -> Option<f64> {
    let [b0, b1, b2, b3, b4, b5, b6, b7] = array::<8>(b, off)?;
    let bits = u64::from_be_bytes([b1, b0, b3, b2, b5, b4, b7, b6]);
    let sign = bits >> 63;
    let exp = (bits >> 55) & 0xff;
    if exp == 0 {
        return Some(0.0);
    }
    let fraction = (bits & ((1u64 << 55) - 1)) >> 3;
    let ieee = (sign << 63) | ((exp + 894) << 52) | fraction;
    Some(f64::from_bits(ieee)).filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_bounds_is_none() {
        let b = [1u8, 2, 3];
        assert_eq!(u16_at(&b, 1), Some(0x0302));
        assert_eq!(u16_at(&b, 2), None);
        assert_eq!(u32_at(&b, usize::MAX), None);
        assert_eq!(bytes(&b, usize::MAX, 2), None);
    }

    #[test]
    fn non_finite_doubles_are_rejected() {
        let nan = f64::NAN.to_le_bytes();
        assert_eq!(f64_at(&nan, 0), None);
        assert_eq!(f64_at(&1.5f64.to_le_bytes(), 0), Some(1.5));
    }

    #[test]
    fn v7_integers_are_middle_endian() {
        // 0x00010002 stored as high word (01 00) then low word (02 00).
        assert_eq!(v7_i32_at(&[0x01, 0x00, 0x02, 0x00], 0), Some(0x0001_0002));
        assert_eq!(v7_i32_at(&[0xff, 0xff, 0xfe, 0xff], 0), Some(-2));
    }

    fn vax_bytes(v: f64) -> [u8; 8] {
        // Inverse of `vax_f64_at` for normal numbers (test helper).
        let bits = v.to_bits();
        let sign = bits >> 63;
        let exp = ((bits >> 52) & 0x7ff) - 894;
        let frac = (bits & ((1u64 << 52) - 1)) << 3;
        let vax = (sign << 63) | (exp << 55) | frac;
        let be = vax.to_be_bytes();
        [be[1], be[0], be[3], be[2], be[5], be[4], be[7], be[6]]
    }

    #[test]
    fn vax_d_float_round_trips() {
        for v in [1.0, -2.5, 46_796.065_84, -249_879_416.0, 1e-10] {
            assert_eq!(vax_f64_at(&vax_bytes(v), 0), Some(v));
        }
        assert_eq!(vax_f64_at(&[0u8; 8], 0), Some(0.0));
        // 1.0 in VAX D: exponent 129, zero fraction -> word0 = 0x4080.
        assert_eq!(vax_f64_at(&[0x80, 0x40, 0, 0, 0, 0, 0, 0], 0), Some(1.0));
    }
}
