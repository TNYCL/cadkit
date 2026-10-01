//! Annotation decoders: MTEXT (§20.4.46), DIMENSION_* (§20.4.22–30), LEADER
//! (§20.4.47) and TOLERANCE (§20.4.49). Cross-checked with ACadSharp's readers (MIT).

use cadkit_core::Result;

use super::{Streams, checked_count};
use crate::native::{Dimension, Leader, MText, ObjectData, Tolerance};

/// The MTEXT fields after the common entity data. Shared with the MTEXT embedded in
/// R2018 multi-line attributes.
pub fn mtext_fields(s: &mut Streams<'_>) -> Result<MText> {
    let v = s.version();
    let mut m = MText {
        insertion: s.main.bd3()?,
        extrusion: s.main.bd3()?,
        x_axis: s.main.bd3()?,
        rect_width: s.main.bd()?,
        ..MText::default()
    };
    if v.r2007_plus() {
        m.rect_height = Some(s.main.bd()?);
    }
    m.height = s.main.bd()?;
    m.attachment = s.main.bs()?;
    m.direction = s.main.bs()?;
    m.extents_height = s.main.bd()?;
    m.extents_width = s.main.bd()?;
    m.value = s.tv()?;
    m.style = s.h_opt()?;
    if v.r2000_plus() {
        m.line_spacing_style = Some(s.main.bs()?);
        m.line_spacing = Some(s.main.bd()?);
        let _unknown = s.main.b()?;
    }
    if v.r2004_plus() {
        let flags = s.main.bl()?;
        m.background_flags = Some(flags);
        if flags & 1 != 0 || (v.r2018_plus() && flags & 0x10 != 0) {
            // The spec types the scale as BL; it is a BD (default 1.5) in the files.
            m.background_scale = Some(s.main.bd()?);
            m.background_color = Some(s.cmc()?);
            m.background_transparency = Some(s.main.bl()?);
        }
    }
    if v.r2018_plus() {
        let not_annotative = s.main.b()?;
        m.annotative = Some(!not_annotative);
        if not_annotative {
            let _version = s.main.bs()?;
            let _default_flag = s.main.b()?;
            let _app = s.h_opt()?;
            let _attachment = s.main.bl()?;
            let _x_axis = s.main.bd3()?;
            let _insertion = s.main.bd3()?;
            for _ in 0..4 {
                let _rect_or_extents = s.main.bd()?;
            }
            let column_type = s.main.bs()?;
            m.column_type = Some(column_type);
            if column_type != 0 {
                let count = s.main.bl()?;
                let _width = s.main.bd()?;
                let _gutter = s.main.bd()?;
                let auto_height = s.main.b()?;
                let _flow_reversed = s.main.b()?;
                if !auto_height && column_type == 2 {
                    let n = checked_count(s, count, 2)?;
                    for _ in 0..n {
                        m.column_heights.push(s.main.bd()?);
                    }
                }
            }
        }
    }
    Ok(m)
}

/// MTEXT.
pub fn mtext(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::MText(mtext_fields(s)?))
}

/// Common dimension data (§20.4.22).
fn dimension_common(s: &mut Streams<'_>, type_code: u16) -> Result<Dimension> {
    let v = s.version();
    let mut d = Dimension {
        type_code,
        ..Dimension::default()
    };
    if v.r2010_plus() {
        d.version = Some(s.main.rc()?);
    }
    d.extrusion = s.main.bd3()?;
    d.text_midpoint = s.main.rd2()?;
    d.elevation = s.main.bd()?;
    d.flags1 = s.main.rc()?;
    d.user_text = s.tv()?;
    d.text_rotation = s.main.bd()?;
    d.horizontal_direction = s.main.bd()?;
    d.insertion_scale = s.main.bd3()?;
    d.insertion_rotation = s.main.bd()?;
    if v.r2000_plus() {
        d.attachment = Some(s.main.bs()?);
        d.line_spacing_style = Some(s.main.bs()?);
        d.line_spacing = Some(s.main.bd()?);
        d.measurement = Some(s.main.bd()?);
    }
    if v.r2007_plus() {
        let _unknown = s.main.b()?;
        d.flip_arrows = Some([s.main.b()?, s.main.b()?]);
    }
    d.pt12 = s.main.rd2()?;
    Ok(d)
}

fn dimension_handles(s: &mut Streams<'_>, d: &mut Dimension) -> Result<()> {
    d.dimstyle = s.h_opt()?;
    d.block = s.h_opt()?;
    Ok(())
}

/// DIMENSION_ORDINATE (0x14).
pub fn dim_ordinate(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut d = dimension_common(s, 0x14)?;
    d.pt10 = s.main.bd3()?;
    d.pt13 = Some(s.main.bd3()?);
    d.pt14 = Some(s.main.bd3()?);
    d.flags2 = Some(s.main.rc()?);
    dimension_handles(s, &mut d)?;
    Ok(ObjectData::Dimension(d))
}

/// DIMENSION_LINEAR (0x15) and DIMENSION_ALIGNED (0x16): 13, 14, 10, extension line
/// rotation, and for linear the dimension rotation.
fn dim_linear_like(s: &mut Streams<'_>, type_code: u16) -> Result<ObjectData> {
    let mut d = dimension_common(s, type_code)?;
    d.pt13 = Some(s.main.bd3()?);
    d.pt14 = Some(s.main.bd3()?);
    d.pt10 = s.main.bd3()?;
    d.ext_line_rotation = Some(s.main.bd()?);
    if type_code == 0x15 {
        d.rotation = Some(s.main.bd()?);
    }
    dimension_handles(s, &mut d)?;
    Ok(ObjectData::Dimension(d))
}

/// DIMENSION_LINEAR (0x15).
pub fn dim_linear(s: &mut Streams<'_>) -> Result<ObjectData> {
    dim_linear_like(s, 0x15)
}

/// DIMENSION_ALIGNED (0x16).
pub fn dim_aligned(s: &mut Streams<'_>) -> Result<ObjectData> {
    dim_linear_like(s, 0x16)
}

/// DIMENSION_ANG_3PT (0x17): 10, 13, 14, 15.
pub fn dim_angular_3pt(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut d = dimension_common(s, 0x17)?;
    d.pt10 = s.main.bd3()?;
    d.pt13 = Some(s.main.bd3()?);
    d.pt14 = Some(s.main.bd3()?);
    d.pt15 = Some(s.main.bd3()?);
    dimension_handles(s, &mut d)?;
    Ok(ObjectData::Dimension(d))
}

/// DIMENSION_ANG_2LN (0x18): 16 (2RD), 13, 14, 15, 10.
pub fn dim_angular_2line(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut d = dimension_common(s, 0x18)?;
    d.pt16 = Some(s.main.rd2()?);
    d.pt13 = Some(s.main.bd3()?);
    d.pt14 = Some(s.main.bd3()?);
    d.pt15 = Some(s.main.bd3()?);
    d.pt10 = s.main.bd3()?;
    dimension_handles(s, &mut d)?;
    Ok(ObjectData::Dimension(d))
}

/// DIMENSION_RADIUS (0x19): 10, 15, leader length.
pub fn dim_radius(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut d = dimension_common(s, 0x19)?;
    d.pt10 = s.main.bd3()?;
    d.pt15 = Some(s.main.bd3()?);
    d.leader_length = Some(s.main.bd()?);
    dimension_handles(s, &mut d)?;
    Ok(ObjectData::Dimension(d))
}

/// DIMENSION_DIAMETER (0x1A): 15, 10, leader length.
pub fn dim_diameter(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut d = dimension_common(s, 0x1A)?;
    d.pt15 = Some(s.main.bd3()?);
    d.pt10 = s.main.bd3()?;
    d.leader_length = Some(s.main.bd()?);
    dimension_handles(s, &mut d)?;
    Ok(ObjectData::Dimension(d))
}

/// LEADER. Text box height/width are present only up to R2007 (spec says Common;
/// both streams are consumed exactly with this rule).
pub fn leader(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let _unknown = s.main.b()?;
    let mut l = Leader {
        annotation_type: s.main.bs()?,
        path_type: s.main.bs()?,
        ..Leader::default()
    };
    let count = s.main.bl()?;
    let n = checked_count(s, count, 6)?;
    for _ in 0..n {
        l.points.push(s.main.bd3()?);
    }
    l.origin = s.main.bd3()?;
    l.extrusion = s.main.bd3()?;
    l.x_direction = s.main.bd3()?;
    l.block_offset = s.main.bd3()?;
    if v >= crate::version::DwgVersion::R14 {
        l.annotation_offset = Some(s.main.bd3()?);
    }
    if v.r13_14() {
        let _dimgap = s.main.bd()?;
    }
    if v <= crate::version::DwgVersion::R2007 {
        l.box_height = Some(s.main.bd()?);
        l.box_width = Some(s.main.bd()?);
    }
    l.hook_line_on_x = s.main.b()?;
    l.arrowhead = s.main.b()?;
    if v.r13_14() {
        let _arrow_type = s.main.bs()?;
        let _dimasz = s.main.bd()?;
        let _ = (
            s.main.b()?,
            s.main.b()?,
            s.main.bs()?,
            s.main.bs()?,
            s.main.b()?,
            s.main.b()?,
        );
    } else {
        let _ = (s.main.bs()?, s.main.b()?, s.main.b()?);
    }
    l.annotation = s.h_opt()?;
    l.dimstyle = s.h_opt()?;
    Ok(ObjectData::Leader(l))
}

/// TOLERANCE.
pub fn tolerance(s: &mut Streams<'_>) -> Result<ObjectData> {
    if s.version().r13_14() {
        let _unknown = s.main.bs()?;
        let _height = s.main.bd()?;
        let _dimscale = s.main.bd()?;
    }
    let mut t = Tolerance {
        insertion: s.main.bd3()?,
        direction: s.main.bd3()?,
        extrusion: s.main.bd3()?,
        ..Tolerance::default()
    };
    t.text = s.tv()?;
    t.dimstyle = s.h_opt()?;
    Ok(ObjectData::Tolerance(t))
}
