//! Byte layout of the 2D GPU instances that [`crate::Plan2d`] encodes and `cadkit-gpu` draws.
//!
//! One instance is [`INSTANCE_BYTES`] bytes, little-endian, with no padding:
//!
//! | Offset | Type | Line segment | Arc piece ([`KIND_ARC`]) |
//! |---|---|---|---|
//! | 0x00 | `[f32; 2]` | start, relative to the chunk origin | center, relative to the chunk origin |
//! | 0x08 | `[f32; 2]` | end, relative to the chunk origin | `[radius, 0]` |
//! | 0x10 | `[u32; 2]` | start / end joint direction, `snorm16x2` (see [`pack_direction`]) | start / end angle as `f32` bits, radians counter-clockwise |
//! | 0x18 | `[u8; 4]` | premultiplied sRGBA color | same |
//! | 0x1c | `f32` | stroke width in logical pixels | same |
//! | 0x20 | `u32` | flags | flags |
//!
//! A polyline corner is shared by two segments: each carries the unit bisector of the corner
//! and the [`START_JOINT`] / [`END_JOINT`] flag, so every pixel of a mitred corner belongs to
//! exactly one segment. Arcs are split into pieces of at most a quarter turn; neighbouring
//! pieces share the boundary angle bit for bit.

/// Size of one instance in bytes.
pub const INSTANCE_BYTES: usize = 36;
/// Flag: the instance is an arc piece rather than a line segment.
pub const KIND_ARC: u32 = 1;
/// Flag: the start of the instance joins a neighbour (corner or adjacent arc piece).
pub const START_JOINT: u32 = 1 << 2;
/// Flag: the end of the instance joins a neighbour.
pub const END_JOINT: u32 = 1 << 3;

/// Packs a unit direction as `snorm16x2` (x in the low half). Two segments that share a
/// corner read the same bits.
pub fn pack_direction(x: f64, y: f64) -> u32 {
    let snorm = |v: f64| u32::from((v.clamp(-1., 1.) * 32767.).round() as i16 as u16);
    snorm(x) | (snorm(y) << 16)
}

/// Premultiplies an unmultiplied sRGBA color in gamma space with rounding, as egui's
/// `Color32::from_rgba_unmultiplied` does, so both paths blend alike.
pub const fn premultiply(color: [u8; 4]) -> [u8; 4] {
    let [r, g, b, a] = color;
    match a {
        0 => [0, 0, 0, 0],
        255 => color,
        _ => [mul(r, a), mul(g, a), mul(b, a), a],
    }
}

/// `round(value * frac / 255)` in fixed point.
const fn mul(value: u8, frac: u8) -> u8 {
    let p = (value as u16) * (frac as u16) + 128;
    ((p + (p >> 8)) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_rounds_like_floating_point() {
        for value in 0..=255u8 {
            for alpha in 1..=254u8 {
                let exact = (f64::from(value) * f64::from(alpha) / 255.).round() as u8;
                assert_eq!(mul(value, alpha), exact, "{value} * {alpha}");
            }
        }
        assert_eq!(premultiply([10, 20, 30, 0]), [0; 4]);
        assert_eq!(premultiply([10, 20, 30, 255]), [10, 20, 30, 255]);
    }

    #[test]
    fn directions_saturate_and_keep_sign() {
        assert_eq!(pack_direction(1., -1.), 0x8001_7fff);
        assert_eq!(pack_direction(2., f64::NAN), 0x7fff);
        assert_eq!(pack_direction(0., 0.), 0);
    }
}
