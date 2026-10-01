//! Entity decoders (spec §20.4.3–§20.4.85). Each runs after the common entity
//! data and reads only the type-specific part.

use cadkit_core::Result;

use super::{Streams, checked_count};
use crate::native::{
    Arc, Attrib, Circle, Insert, InsertArray, Line, LwPolyline, ObjectData, PointEntity, Text,
};

/// LINE (§20.4.21).
pub fn line(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let (start, end) = if v.r13_14() {
        (s.main.bd3()?, s.main.bd3()?)
    } else {
        let z_zero = s.main.b()?;
        let sx = s.main.rd()?;
        let ex = s.main.dd(sx)?;
        let sy = s.main.rd()?;
        let ey = s.main.dd(sy)?;
        let (sz, ez) = if z_zero {
            (0.0, 0.0)
        } else {
            let sz = s.main.rd()?;
            (sz, s.main.dd(sz)?)
        };
        ([sx, sy, sz], [ex, ey, ez])
    };
    let thickness = s.main.bt(v.r2000_plus())?;
    let extrusion = s.main.be(v.r2000_plus())?;
    Ok(ObjectData::Line(Line {
        start,
        end,
        thickness,
        extrusion,
    }))
}

/// POINT (§20.4.31).
pub fn point(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let position = s.main.bd3()?;
    let thickness = s.main.bt(v.r2000_plus())?;
    let extrusion = s.main.be(v.r2000_plus())?;
    let x_axis_angle = s.main.bd()?;
    Ok(ObjectData::Point(PointEntity {
        position,
        thickness,
        extrusion,
        x_axis_angle,
    }))
}

/// CIRCLE (§20.4.20).
pub fn circle(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let center = s.main.bd3()?;
    let radius = s.main.bd()?;
    let thickness = s.main.bt(v.r2000_plus())?;
    let extrusion = s.main.be(v.r2000_plus())?;
    Ok(ObjectData::Circle(Circle {
        center,
        radius,
        thickness,
        extrusion,
    }))
}

/// ARC (§20.4.18).
pub fn arc(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let center = s.main.bd3()?;
    let radius = s.main.bd()?;
    let thickness = s.main.bt(v.r2000_plus())?;
    let extrusion = s.main.be(v.r2000_plus())?;
    let start_angle = s.main.bd()?;
    let end_angle = s.main.bd()?;
    Ok(ObjectData::Arc(Arc {
        center,
        radius,
        thickness,
        extrusion,
        start_angle,
        end_angle,
    }))
}

/// LWPOLYLINE (§20.4.85).
pub fn lwpolyline(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let flags = s.main.bs()? as u16;
    let mut p = LwPolyline {
        flags,
        extrusion: [0.0, 0.0, 1.0],
        ..LwPolyline::default()
    };
    if flags & 0x4 != 0 {
        p.const_width = s.main.bd()?;
    }
    if flags & 0x8 != 0 {
        p.elevation = s.main.bd()?;
    }
    if flags & 0x2 != 0 {
        p.thickness = s.main.bd()?;
    }
    if flags & 0x1 != 0 {
        p.extrusion = s.main.bd3()?;
    }
    let num_points = s.main.bl()?;
    let num_bulges = if flags & 0x10 != 0 { s.main.bl()? } else { 0 };
    let num_ids = if v.r2010_plus() && flags & 0x400 != 0 {
        s.main.bl()?
    } else {
        0
    };
    let num_widths = if flags & 0x20 != 0 { s.main.bl()? } else { 0 };

    // Each vertex needs at least 2 bits (two DD "use default" codes), raw ones 128.
    let n = checked_count(s, num_points, if v.r13_14() { 128 } else { 4 })?;
    p.points.reserve(n);
    if v.r13_14() {
        for _ in 0..n {
            p.points.push(s.main.rd2()?);
        }
    } else if n > 0 {
        let mut prev = s.main.rd2()?;
        p.points.push(prev);
        for _ in 1..n {
            prev = s.main.dd2(prev)?;
            p.points.push(prev);
        }
    }
    let nb = checked_count(s, num_bulges, 2)?;
    for _ in 0..nb {
        p.bulges.push(s.main.bd()?);
    }
    let ni = checked_count(s, num_ids, 2)?;
    for _ in 0..ni {
        p.vertex_ids.push(s.main.bl()?);
    }
    let nw = checked_count(s, num_widths, 4)?;
    for _ in 0..nw {
        p.widths.push(s.main.bd2()?);
    }
    Ok(ObjectData::LwPolyline(p))
}

/// Text fields shared by TEXT, ATTRIB and ATTDEF (§20.4.3), including the STYLE handle.
fn text_data(s: &mut Streams<'_>) -> Result<Text> {
    let v = s.version();
    let mut t = Text {
        width_factor: 1.0,
        extrusion: [0.0, 0.0, 1.0],
        ..Text::default()
    };
    if v.r13_14() {
        t.elevation = s.main.bd()?;
        t.insertion = s.main.rd2()?;
        t.alignment = Some(s.main.rd2()?);
        t.extrusion = s.main.bd3()?;
        t.thickness = s.main.bd()?;
        t.oblique = s.main.bd()?;
        t.rotation = s.main.bd()?;
        t.height = s.main.bd()?;
        t.width_factor = s.main.bd()?;
        t.value = s.tv()?;
        t.generation = s.main.bs()?;
        t.halign = s.main.bs()?;
        t.valign = s.main.bs()?;
    } else {
        let flags = s.main.rc()?;
        if flags & 0x01 == 0 {
            t.elevation = s.main.rd()?;
        }
        t.insertion = s.main.rd2()?;
        if flags & 0x02 == 0 {
            t.alignment = Some(s.main.dd2(t.insertion)?);
        }
        t.extrusion = s.main.be(true)?;
        t.thickness = s.main.bt(true)?;
        if flags & 0x04 == 0 {
            t.oblique = s.main.rd()?;
        }
        if flags & 0x08 == 0 {
            t.rotation = s.main.rd()?;
        }
        t.height = s.main.rd()?;
        if flags & 0x10 == 0 {
            t.width_factor = s.main.rd()?;
        }
        t.value = s.tv()?;
        if flags & 0x20 == 0 {
            t.generation = s.main.bs()?;
        }
        if flags & 0x40 == 0 {
            t.halign = s.main.bs()?;
        }
        if flags & 0x80 == 0 {
            t.valign = s.main.bs()?;
        }
    }
    t.style = s.h_opt()?;
    Ok(t)
}

/// TEXT (§20.4.3).
pub fn text(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::Text(text_data(s)?))
}

/// ATTRIB fields after the text data (§20.4.4). R2018+ multi-line attributes embed a
/// complete MTEXT starting at its entity mode (common entity part included, owner
/// handle possibly 0), followed by optional annotative data.
fn attrib_data(s: &mut Streams<'_>) -> Result<Attrib> {
    let v = s.version();
    let text = text_data(s)?;
    let mut a = Attrib {
        text,
        ..Attrib::default()
    };
    if v.r2010_plus() {
        a.version = Some(s.main.rc()?);
    }
    if v.r2018_plus() {
        let kind = s.main.rc()?;
        a.attribute_type = Some(kind);
        if kind > 1 {
            let mut embedded = crate::native::CommonData::default();
            let _entity = super::read_entity_mode(s, &mut embedded)?;
            a.mtext = Some(super::annotation::mtext_fields(s)?);
            let annotative_size = s.main.bs()?;
            if annotative_size > 0 {
                let size = usize::try_from(annotative_size)
                    .map_err(|_| s.main.invalid("annotative data size"))?;
                let _data = s.main.bytes(size)?;
                let _app = s.h_opt()?;
                let _unknown = s.main.bs()?;
            }
        }
    }
    a.tag = s.tv()?;
    a.field_length = s.main.bs()?;
    a.flags = s.main.rc()?;
    if v.r2007_plus() {
        a.lock_position = s.main.b()?;
    }
    Ok(a)
}

/// ATTRIB (§20.4.4).
pub fn attrib(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::Attrib(attrib_data(s)?))
}

/// ATTDEF (§20.4.5).
pub fn attdef(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut a = attrib_data(s)?;
    if s.version().r2010_plus() {
        let _version = s.main.rc()?;
    }
    a.prompt = Some(s.tv()?);
    Ok(ObjectData::AttDef(a))
}

/// BLOCK (§20.4.6).
pub fn block(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::Block { name: s.tv()? })
}

/// Common INSERT/MINSERT data (§20.4.9) up to the "has attribs" bit.
fn insert_data(s: &mut Streams<'_>) -> Result<(Insert, i32)> {
    let v = s.version();
    let mut ins = Insert {
        position: s.main.bd3()?,
        scale: [1.0; 3],
        ..Insert::default()
    };
    if v.r13_14() {
        ins.scale = s.main.bd3()?;
    } else {
        ins.scale = match s.main.bb()? {
            0 => {
                let x = s.main.rd()?;
                [x, s.main.dd(x)?, s.main.dd(x)?]
            }
            1 => [1.0, s.main.dd(1.0)?, s.main.dd(1.0)?],
            2 => {
                let x = s.main.rd()?;
                [x, x, x]
            }
            _ => [1.0, 1.0, 1.0],
        };
    }
    ins.rotation = s.main.bd()?;
    ins.extrusion = s.main.bd3()?;
    ins.has_attribs = s.main.b()?;
    // The owned-object count exists only when attributes follow (the spec lists it
    // unconditionally; the files agree with ACadSharp).
    let owned = if v.r2004_plus() && ins.has_attribs {
        s.main.bl()?
    } else {
        0
    };
    Ok((ins, owned))
}

/// INSERT/MINSERT handles (§20.4.9).
fn insert_handles(s: &mut Streams<'_>, ins: &mut Insert, owned: i32) -> Result<()> {
    ins.block_header = s.h_opt()?;
    if !ins.has_attribs {
        return Ok(());
    }
    if s.version().r2004_plus() {
        let n =
            usize::try_from(owned).map_err(|_| s.main.invalid("negative owned object count"))?;
        let remaining = s.handles.as_ref().map_or(0, |h| h.remaining_bits());
        if (n as u64).saturating_mul(8) > remaining {
            return Err(s
                .main
                .invalid(format!("{n} owned attributes exceed the handle stream")));
        }
        for _ in 0..n {
            ins.attribs.push(s.h()?);
        }
    } else {
        ins.attribs_are_range = true;
        let first = s.h()?;
        let last = s.h()?;
        ins.attribs.extend([first, last]);
    }
    ins.seqend = s.h_opt()?;
    Ok(())
}

/// INSERT (§20.4.9).
pub fn insert(s: &mut Streams<'_>) -> Result<ObjectData> {
    let (mut ins, owned) = insert_data(s)?;
    insert_handles(s, &mut ins, owned)?;
    Ok(ObjectData::Insert(ins))
}

/// MINSERT (§20.4.10).
pub fn minsert(s: &mut Streams<'_>) -> Result<ObjectData> {
    let (mut ins, owned) = insert_data(s)?;
    ins.array = Some(InsertArray {
        columns: s.main.bs()?,
        rows: s.main.bs()?,
        column_spacing: s.main.bd()?,
        row_spacing: s.main.bd()?,
    });
    insert_handles(s, &mut ins, owned)?;
    Ok(ObjectData::Insert(ins))
}
