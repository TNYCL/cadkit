//! Decoders for images, viewports and the entities the model keeps as `Unknown`
//! (RAY, XLINE, SHAPE, MLINE, OLE2FRAME, REGION/3DSOLID/BODY). Cross-checked with
//! ACadSharp's readers (MIT).

use cadkit_core::Result;

use super::{Streams, checked_count, read_handles};
use crate::native::{
    AcisData, Image, ImageDef, MLine, ObjectData, Ole2Frame, RayLine, Shape, Viewport,
};

/// IMAGE and WIPEOUT (classes, §20.4.80).
pub fn image(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut im = Image {
        class_version: s.main.bl()?,
        insertion: s.main.bd3()?,
        u: s.main.bd3()?,
        v: s.main.bd3()?,
        size: s.main.rd2()?,
        flags: s.main.bs()?,
        clipping: s.main.b()?,
        brightness: s.main.rc()?,
        contrast: s.main.rc()?,
        fade: s.main.rc()?,
        ..Image::default()
    };
    if s.version().r2010_plus() {
        im.clip_inside = Some(s.main.b()?);
    }
    im.clip_type = s.main.bs()?;
    match im.clip_type {
        1 => im.clip_vertices = vec![s.main.rd2()?, s.main.rd2()?],
        2 => {
            let count = s.main.bl()?;
            let n = checked_count(s, count, 128)?;
            for _ in 0..n {
                im.clip_vertices.push(s.main.rd2()?);
            }
        }
        _ => {}
    }
    im.imagedef = s.h_opt()?;
    im.reactor = s.h_opt()?;
    Ok(ObjectData::Image(im))
}

/// IMAGEDEF (class, §20.4.81).
pub fn imagedef(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::ImageDef(ImageDef {
        class_version: s.main.bl()?,
        size: s.main.rd2()?,
        file_name: s.tv()?,
        loaded: s.main.b()?,
        units: s.main.rc()?,
        pixel_size: s.main.rd2()?,
    }))
}

/// VIEWPORT entity (§20.4.38).
pub fn viewport(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let mut vp = Viewport {
        center: s.main.bd3()?,
        width: s.main.bd()?,
        height: s.main.bd()?,
        ..Viewport::default()
    };
    if v.r2000_plus() {
        vp.view_target = s.main.bd3()?;
        vp.view_direction = s.main.bd3()?;
        vp.twist = s.main.bd()?;
        vp.view_height = s.main.bd()?;
        vp.lens_length = s.main.bd()?;
        vp.front_clip = s.main.bd()?;
        vp.back_clip = s.main.bd()?;
        vp.snap_angle = s.main.bd()?;
        vp.view_center = s.main.rd2()?;
        let _snap_base = s.main.rd2()?;
        let _snap_spacing = s.main.rd2()?;
        let _grid_spacing = s.main.rd2()?;
        let _circle_zoom = s.main.bs()?;
    }
    if v.r2007_plus() {
        let _grid_major = s.main.bs()?;
    }
    let mut frozen = 0;
    if v.r2000_plus() {
        frozen = s.main.bl()?;
        vp.status = s.main.bl()?;
        vp.style_sheet = s.tv()?;
        let _render_mode = s.main.rc()?;
        let _ucs_icon = s.main.b()?;
        let _ucs_per_viewport = s.main.b()?;
        let _ucs_origin = s.main.bd3()?;
        let _ucs_x = s.main.bd3()?;
        let _ucs_y = s.main.bd3()?;
        let _elevation = s.main.bd()?;
        let _ortho_type = s.main.bs()?;
    }
    if v.r2004_plus() {
        let _shade_plot_mode = s.main.bs()?;
    }
    if v.r2007_plus() {
        let _default_lighting = s.main.b()?;
        let _lighting_type = s.main.rc()?;
        let _brightness = s.main.bd()?;
        let _contrast = s.main.bd()?;
        let _ambient = s.cmc()?;
    }
    if v.r13_14() {
        let _vp_entity_header = s.h_opt()?;
    }
    if v.r2000_plus() {
        vp.frozen_layers = read_handles(s, frozen, "frozen layer")?;
        let _boundary = s.h_opt()?;
    }
    if v == crate::version::DwgVersion::R2000 {
        let _vp_entity_header = s.h_opt()?;
    }
    if v.r2000_plus() {
        let _named_ucs = s.h_opt()?;
        let _base_ucs = s.h_opt()?;
    }
    if v.r2007_plus() {
        for _ in 0..4 {
            // background, visual style, shade plot, sun
            let _ = s.h_opt()?;
        }
    }
    Ok(ObjectData::Viewport(vp))
}

/// RAY and XLINE (§20.4.42–43).
pub fn ray_line(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::RayLine(RayLine {
        point: s.main.bd3()?,
        direction: s.main.bd3()?,
    }))
}

/// SHAPE (§20.4.37).
pub fn shape(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut sh = Shape {
        insertion: s.main.bd3()?,
        size: s.main.bd()?,
        rotation: s.main.bd()?,
        x_scale: s.main.bd()?,
        oblique: s.main.bd()?,
        thickness: s.main.bd()?,
        index: s.main.bs()?,
        extrusion: s.main.bd3()?,
        style: None,
    };
    sh.style = s.h_opt()?;
    Ok(ObjectData::Shape(sh))
}

/// MLINE (§20.4.50): the per-vertex segment parameters are skipped.
pub fn mline(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut m = MLine {
        scale: s.main.bd()?,
        justification: s.main.rc()?,
        base: s.main.bd3()?,
        extrusion: s.main.bd3()?,
        flags: s.main.bs()?,
        line_count: s.main.rc()?,
        ..MLine::default()
    };
    let count = i32::from(s.main.bs()?);
    let n = checked_count(s, count, 18)?;
    for _ in 0..n {
        m.vertices.push(s.main.bd3()?);
        let _direction = s.main.bd3()?;
        let _miter = s.main.bd3()?;
        for _ in 0..m.line_count {
            for _ in 0..2 {
                // segment parameters, then area fill parameters
                let params = i32::from(s.main.bs()?);
                let k = checked_count(s, params, 2)?;
                for _ in 0..k {
                    let _ = s.main.bd()?;
                }
            }
        }
    }
    m.style = s.h_opt()?;
    Ok(ObjectData::MLine(m))
}

/// OLE2FRAME (§20.4.88): the OLE data is skipped, its size kept.
pub fn ole2frame(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let mut o = Ole2Frame {
        version: s.main.bs()?,
        ..Ole2Frame::default()
    };
    if v.r2000_plus() {
        o.mode = Some(s.main.bs()?);
    }
    o.data_size = s.main.bl()?;
    let size = u64::try_from(o.data_size).map_err(|_| s.main.invalid("negative OLE data size"))?;
    s.main.skip_bits(size.saturating_mul(8))?;
    if v.r2000_plus() {
        let _unknown = s.main.rc()?;
    }
    Ok(ObjectData::Ole2Frame(o))
}

/// REGION / 3DSOLID / BODY (§20.4.41): only the modeler data header is decoded; for
/// version 1 the SAT block sizes are summed. Wireframe/silhouette data is not read.
pub fn acis(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut a = AcisData {
        empty: s.main.b()?,
        ..AcisData::default()
    };
    if a.empty {
        return Ok(ObjectData::Acis(a));
    }
    let _unknown = s.main.b()?;
    a.version = s.main.bs()?;
    if a.version == 1 {
        let mut total: u64 = 0;
        loop {
            let size = s.main.bl()?;
            if size <= 0 {
                break;
            }
            let size = size as u64;
            s.main.skip_bits(size.saturating_mul(8))?;
            total = total.saturating_add(size);
        }
        a.sat_bytes = Some(total);
    }
    Ok(ObjectData::Acis(a))
}
