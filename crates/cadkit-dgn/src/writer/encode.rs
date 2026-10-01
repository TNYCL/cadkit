//! Belgelenmiş V8 öğe alanlarının sınır denetimli kodlayıcısı.

use crate::native::v8::ModelHeader;
use cadkit_core::{Entity, Error, Lineweight, Point3, Result, Value, Vec3};

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

pub(super) fn finish(mut b: Vec<u8>, links: &[u8]) -> Result<Vec<u8>> {
    let aligned = b
        .len()
        .checked_add(7)
        .ok_or_else(|| Error::LimitExceeded("DGN record bytes".into()))?
        & !7;
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
        .checked_add(7)
        .ok_or_else(|| Error::LimitExceeded("DGN record bytes".into()))?
        & !7;
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

pub(super) fn header(
    kind: u32,
    size: usize,
    id: u64,
    level: u32,
    time: f64,
    entity: &Entity,
    three: bool,
) -> Result<Vec<u8>> {
    let mut b = prefix(kind | 0x1000_0000, size, id, level, time)?;
    let hole = matches!(entity.props.get("dgn.hole"), Some(Value::Bool(true)));
    u32_at(
        &mut b,
        0x28,
        if three { 0x800 } else { 0 }
            | if hole { 0x8000 } else { 0 }
            | if !entity.visible { 0x80 } else { 0 },
    )?;
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
    let weight = match entity.props.get("dgn.weight") {
        Some(Value::Int(n)) => u32::try_from(*n).map_err(|_| Error::invalid(0, "DGN weight"))?,
        _ => match entity.lineweight {
            Lineweight::Millimeters(_) => {
                return Err(Error::Unsupported(
                    "DGN lineweight requires dgn.weight index".into(),
                ));
            }
            _ => 0,
        },
    };
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
    for (i, v) in low.into_iter().chain(high).enumerate() {
        if !v.is_finite() || v.abs() >= i64::MAX as f64 {
            return Err(Error::invalid(0, "DGN range overflow"));
        }
        let n = if i < 3 { v.floor() } else { v.ceil() } as i64;
        put(b, 0x38 + 8 * i, &n.to_le_bytes())?;
    }
    Ok(())
}

pub(super) fn utf16(value: &str) -> Result<Vec<u8>> {
    if value.contains('\0') {
        return Err(Error::invalid(0, "embedded NUL in DGN string"));
    }
    let mut b = vec![0xff, 0xfd];
    for w in value.encode_utf16() {
        b.extend_from_slice(&w.to_le_bytes());
    }
    b.extend_from_slice(&[0, 0]);
    Ok(b)
}
pub(super) fn string_link(id: u32, value: &str) -> Result<Vec<u8>> {
    let text = utf16(value)?;
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
