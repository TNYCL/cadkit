//! Belgelenmiş V8 öğe alanlarının sınır denetimli kodlayıcısı.

use crate::native::v8::ModelHeader;
use cadkit_core::{Entity, Error, Point3, Result, Value, Vec3};

pub(super) fn put(b: &mut [u8], offset: usize, value: &[u8]) -> Result<()> {
    let end = offset
        .checked_add(value.len())
        .ok_or_else(|| Error::LimitExceeded("DGN field offset".into()))?;
    b.get_mut(offset..end)
        .ok_or_else(|| Error::invalid(offset as u64, "DGN output field outside record"))?
        .copy_from_slice(value);
    Ok(())
}
pub(super) fn u32_at(b: &mut [u8], o: usize, v: u32) -> Result<()> {
    put(b, o, &v.to_le_bytes())
}
pub(super) fn u64_at(b: &mut [u8], o: usize, v: u64) -> Result<()> {
    put(b, o, &v.to_le_bytes())
}
pub(super) fn f64_at(b: &mut [u8], o: usize, v: f64) -> Result<()> {
    if !v.is_finite() {
        return Err(Error::invalid(0, "non-finite DGN output value"));
    }
    put(b, o, &v.to_le_bytes())
}

pub(super) fn finish(b: Vec<u8>, links: &[u8]) -> Result<Vec<u8>> {
    finish_aligned(b, links, 8)
}

/// Appends linkages after the body padded to `align` bytes (a power of two) and sets the
/// attribute offset and record length words. MicroStation pads tag records to 2 bytes.
pub(super) fn finish_aligned(mut b: Vec<u8>, links: &[u8], align: usize) -> Result<Vec<u8>> {
    let mask = align.saturating_sub(1);
    let aligned = b
        .len()
        .checked_add(mask)
        .ok_or_else(|| Error::LimitExceeded("DGN record bytes".into()))?
        & !mask;
    b.resize(aligned, 0);
    u32_at(
        &mut b,
        8,
        u32::try_from(aligned / 2)
            .map_err(|_| Error::LimitExceeded("DGN attribute offset".into()))?,
    )?;
    b.extend_from_slice(links);
    let end = b
        .len()
        .checked_add(mask)
        .ok_or_else(|| Error::LimitExceeded("DGN record bytes".into()))?
        & !mask;
    b.resize(end, 0);
    u32_at(
        &mut b,
        4,
        u32::try_from(end / 2).map_err(|_| Error::LimitExceeded("DGN record bytes".into()))?,
    )?;
    Ok(b)
}

pub(super) fn prefix(kind: u32, size: usize, id: u64, level: u32, time: f64) -> Result<Vec<u8>> {
    let mut b = vec![0; size];
    u32_at(&mut b, 0, kind)?;
    u32_at(&mut b, 12, level)?;
    u64_at(&mut b, 16, id)?;
    f64_at(&mut b, 24, time)?;
    Ok(b)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn header(
    kind: u32,
    size: usize,
    id: u64,
    level: u32,
    time: f64,
    entity: &Entity,
    three: bool,
    weight: u32,
) -> Result<Vec<u8>> {
    // Undecoded flags MicroStation sets on its own graphics (type word `0x00C0_0000`,
    // property word `0x0600`) are reproduced when the entity carries them; the meaningful
    // bits (3D, hole, hidden) are always derived from the entity itself.
    let flags = |key: &str, mask: u32| match entity.props.get(key) {
        Some(Value::Int(n)) => u32::try_from(*n).map(|v| v & mask).unwrap_or(0),
        _ => 0,
    };
    let type_flags = flags("dgn.type_flags", 0x00c0) << 16;
    let mut b = prefix(kind | 0x1000_0000 | type_flags, size, id, level, time)?;
    let hole = matches!(entity.props.get("dgn.hole"), Some(Value::Bool(true)));
    u32_at(
        &mut b,
        0x28,
        if three { 0x800 } else { 0 }
            | if hole { 0x8000 } else { 0 }
            | if !entity.visible { 0x80 } else { 0 }
            | flags("dgn.properties", 0x0600),
    )?;
    if let Some(Value::Int(n)) = entity.props.get("dgn.graphic_group") {
        u32_at(
            &mut b,
            0x20,
            u32::try_from(*n).map_err(|_| Error::invalid(0, "DGN graphic group"))?,
        )?;
    }
    let color = match entity.props.get("dgn.color_index") {
        Some(Value::Int(n)) => {
            u32::try_from(*n).map_err(|_| Error::invalid(0, "negative DGN color"))?
        }
        _ => 0,
    };
    if color > 255 {
        return Err(Error::Unsupported(
            "extended DGN color table entries".into(),
        ));
    }
    u32_at(&mut b, 0x34, color)?;
    if let Some(Value::Int(n)) = entity.props.get("dgn.style") {
        u32_at(
            &mut b,
            0x2c,
            u32::try_from(*n).map_err(|_| Error::invalid(0, "DGN style"))?,
        )?;
    }
    u32_at(&mut b, 0x30, weight)?;
    Ok(b)
}

pub(super) fn coordinate(p: Point3, model: &ModelHeader) -> Result<[f64; 3]> {
    let origin = model.global_origin;
    let p = [p.x, p.y, p.z];
    let mut out = [0.0; 3];
    for ((dst, v), o) in out.iter_mut().zip(p).zip(origin) {
        *dst = v * model.uor_per_master + o;
        if !dst.is_finite() || dst.abs() >= i64::MAX as f64 {
            return Err(Error::invalid(0, "DGN coordinate outside UOR range"));
        }
    }
    if !model.is_3d && p.get(2).copied().unwrap_or(0.0) != 0.0 {
        return Err(Error::Unsupported(
            "nonzero Z cannot be written into a 2D seed".into(),
        ));
    }
    Ok(out)
}
pub(super) fn point(b: &mut [u8], o: usize, p: [f64; 3], three: bool) -> Result<()> {
    for (i, v) in p.into_iter().take(if three { 3 } else { 2 }).enumerate() {
        f64_at(b, o + 8 * i, v)?;
    }
    Ok(())
}
/// Writes the element range: the low corner at `0x38` and the extent (`high - low`) at
/// `0x50`, both in UOR. Every V8 graphic record stores the extent there, not an absolute
/// high corner (FORMAT_NOTES, "Element range"); the model header alone is absolute.
pub(super) fn range(b: &mut [u8], points: &[[f64; 3]]) -> Result<()> {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in points {
        for ((lo, hi), v) in low.iter_mut().zip(high.iter_mut()).zip(p) {
            *lo = lo.min(*v);
            *hi = hi.max(*v);
        }
    }
    if points.is_empty() {
        low = [0.0; 3];
        high = [0.0; 3];
    }
    let mut corners = [[0i64; 3]; 2];
    for (i, v) in low.into_iter().chain(high).enumerate() {
        if !v.is_finite() || v.abs() >= i64::MAX as f64 {
            return Err(Error::invalid(0, "DGN range overflow"));
        }
        let n = if i < 3 { v.floor() } else { v.ceil() } as i64;
        if let Some(slot) = corners.get_mut(i / 3).and_then(|c| c.get_mut(i % 3)) {
            *slot = n;
        }
    }
    let [low, high] = corners;
    put_range(b, low, high)
}

/// Stores absolute low/high corners as the V8 low corner and extent.
pub(super) fn put_range(b: &mut [u8], low: [i64; 3], high: [i64; 3]) -> Result<()> {
    for (axis, (lo, hi)) in low.into_iter().zip(high).enumerate() {
        let extent = hi
            .checked_sub(lo)
            .filter(|e| *e >= 0)
            .ok_or_else(|| Error::invalid(0, "DGN range extent is negative or overflows"))?;
        put(b, 0x38 + axis * 8, &lo.to_le_bytes())?;
        put(b, 0x50 + axis * 8, &extent.to_le_bytes())?;
    }
    Ok(())
}

/// Absolute bounds of a generated record: the low corner plus its stored extent.
pub(super) fn absolute_range(b: &[u8]) -> Result<[[i64; 3]; 2]> {
    let mut low = [0; 3];
    let mut high = [0; 3];
    for (axis, (lo, hi)) in low.iter_mut().zip(high.iter_mut()).enumerate() {
        *lo = crate::le::i64_at(b, 0x38 + axis * 8)
            .ok_or_else(|| Error::invalid(0, "short DGN record range"))?;
        let extent = crate::le::i64_at(b, 0x50 + axis * 8)
            .ok_or_else(|| Error::invalid(0, "short DGN record range"))?;
        if extent < 0 {
            return Err(Error::invalid(0, "negative DGN range extent"));
        }
        *hi = lo
            .checked_add(extent)
            .ok_or_else(|| Error::invalid(0, "DGN range high overflow"))?;
    }
    Ok([low, high])
}

fn utf16_units(value: &str) -> Result<Vec<u8>> {
    if value.contains('\0') {
        return Err(Error::invalid(0, "embedded NUL in DGN string"));
    }
    let mut b = vec![0xff, 0xfd];
    for w in value.encode_utf16() {
        b.extend_from_slice(&w.to_le_bytes());
    }
    Ok(b)
}

/// `ff fd` + UTF-16LE with a trailing NUL unit, as tag set definitions and tag values
/// store it (their readers split at the terminator; MicroStation counts it in tag lengths).
pub(super) fn utf16(value: &str) -> Result<Vec<u8>> {
    let mut b = utf16_units(value)?;
    b.extend_from_slice(&[0, 0]);
    Ok(b)
}

/// `ff fd` + UTF-16LE without a terminator, as text element payloads and string
/// linkages store it: MicroStation, ODA and GDAL files never count a NUL in their
/// lengths, and MicroStation draws a counted NUL as an extra glyph.
pub(super) fn utf16_unterminated(value: &str) -> Result<Vec<u8>> {
    utf16_units(value)
}

pub(super) fn string_link(id: u32, value: &str) -> Result<Vec<u8>> {
    let text = utf16_unterminated(value)?;
    let size = (12 + text.len() + 7) & !7;
    if size > 512 {
        return Err(Error::LimitExceeded(
            "DGN string linkage exceeds 512 bytes".into(),
        ));
    }
    let mut b = vec![0; size];
    put(&mut b, 0, &[(size / 2 - 1) as u8, 0x10])?;
    put(&mut b, 2, &0x56d2u16.to_le_bytes())?;
    u32_at(&mut b, 4, id)?;
    u32_at(&mut b, 8, text.len() as u32)?;
    put(&mut b, 12, &text)?;
    Ok(b)
}
pub(super) fn dependency(app: u16, id: u64) -> Result<Vec<u8>> {
    let mut b = vec![0; 24];
    put(&mut b, 0, &[11, 0x10])?;
    put(&mut b, 2, &0x56d0u16.to_le_bytes())?;
    put(&mut b, 4, &app.to_le_bytes())?;
    put(&mut b, 8, &[0, 2, 1, 0])?;
    u64_at(&mut b, 12, id)?;
    Ok(b)
}

/// Tag-to-element dependency as MicroStation 8.11 writes it: application `0x2710` with
/// value 1, copy option 2, one 16-byte root of type 9 whose first half is the constant
/// `01 00 00 00 00 00 01 00` (meaning not decoded) and whose second half is the element
/// id, then 28 reserved zero bytes. Readers take the id from the second half.
pub(super) fn tag_target_dependency(id: u64) -> Result<Vec<u8>> {
    let mut b = vec![0; 56];
    put(&mut b, 0, &[0x1b, 0x10])?;
    put(&mut b, 2, &0x56d0u16.to_le_bytes())?;
    put(&mut b, 4, &0x2710u16.to_le_bytes())?;
    put(&mut b, 6, &1u16.to_le_bytes())?;
    put(&mut b, 8, &[2, 9, 1, 0])?;
    put(&mut b, 12, &[1, 0, 0, 0, 0, 0, 1, 0])?;
    u64_at(&mut b, 20, id)?;
    Ok(b)
}

pub(super) fn rotation(
    b: &mut [u8],
    o: usize,
    x: Vec3,
    y: Vec3,
    n: Vec3,
    three: bool,
) -> Result<()> {
    if !three {
        if n.z < 0.999999 {
            return Err(Error::Unsupported("tilted geometry in 2D seed".into()));
        }
        return f64_at(b, o, x.y.atan2(x.x));
    }
    let (m00, m01, m02, m10, m11, m12, m20, m21, m22) =
        (x.x, y.x, n.x, x.y, y.y, n.y, x.z, y.z, n.z);
    let (w, qx, qy, qz) = if m00 + m11 + m22 > 0.0 {
        let s = (m00 + m11 + m22 + 1.0).sqrt() * 2.0;
        (s / 4.0, (m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s)
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        ((m21 - m12) / s, s / 4.0, (m01 + m10) / s, (m02 + m20) / s)
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        ((m02 - m20) / s, (m01 + m10) / s, s / 4.0, (m12 + m21) / s)
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        ((m10 - m01) / s, (m02 + m20) / s, (m12 + m21) / s, s / 4.0)
    };
    // DGN's stored quaternion is the conjugate of the local-to-world convention
    // used above; see native::element::Rotation::matrix and its GDAL attribution.
    for (i, v) in [w, -qx, -qy, -qz].into_iter().enumerate() {
        f64_at(b, o + i * 8, v)?;
    }
    Ok(())
}
