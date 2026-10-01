//! Curve, polyline and face decoders: ELLIPSE (§20.4.39), SPLINE (§20.4.40), the
//! POLYLINE family with its VERTEX records (§20.4.11–17, 33, 34), SOLID / TRACE
//! (§20.4.35–36) and 3DFACE (§20.4.32). Cross-checked with ACadSharp's readers (MIT).

use cadkit_core::Result;

use super::{Streams, checked_count, read_handles, spline_degree};
use crate::native::{
    Ellipse, Face, FaceKind, ObjectData, PfaceFace, Polyline, PolylineKind, PolylineVertex, Spline,
};

/// ELLIPSE.
pub fn ellipse(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::Ellipse(Ellipse {
        center: s.main.bd3()?,
        major_axis: s.main.bd3()?,
        extrusion: s.main.bd3()?,
        ratio: s.main.bd()?,
        start: s.main.bd()?,
        end: s.main.bd()?,
    }))
}

/// SPLINE. From R2013 the scenario is derived from the flags: control-point data
/// unless the knot parameterization is not custom and "use knot parameter" (flag 8)
/// is set (as ACadSharp; consumption checked on the samples).
pub fn spline(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let mut sp = Spline {
        scenario: s.main.bl()?,
        ..Spline::default()
    };
    if v.r2013_plus() {
        let flags1 = s.main.bl()?;
        let knot_parameter = s.main.bl()?;
        sp.closed = flags1 & 4 != 0;
        sp.scenario = if knot_parameter == 15 || flags1 & 8 == 0 {
            1
        } else {
            2
        };
        sp.flags1 = Some(flags1);
        sp.knot_parameter = Some(knot_parameter);
    }
    let degree = s.main.bl()?;
    sp.degree = spline_degree(s, degree);
    let (mut knots, mut controls, mut fits, mut weighted) = (0, 0, 0, false);
    match sp.scenario {
        1 => {
            sp.rational = s.main.b()?;
            sp.closed = s.main.b()?;
            sp.periodic = s.main.b()?;
            sp.knot_tolerance = s.main.bd()?;
            sp.control_tolerance = s.main.bd()?;
            knots = s.main.bl()?;
            controls = s.main.bl()?;
            weighted = s.main.b()?;
        }
        2 => {
            sp.fit_tolerance = s.main.bd()?;
            sp.start_tangent = Some(s.main.bd3()?);
            sp.end_tangent = Some(s.main.bd3()?);
            fits = s.main.bl()?;
        }
        other => return Err(s.main.invalid(format!("spline scenario {other}"))),
    }
    let knots = checked_count(s, knots, 2)?;
    for _ in 0..knots {
        sp.knots.push(s.main.bd()?);
    }
    let controls = checked_count(s, controls, 6)?;
    for _ in 0..controls {
        sp.control_points.push(s.main.bd3()?);
        if weighted {
            sp.weights.push(s.main.bd()?);
        }
    }
    let fits = checked_count(s, fits, 6)?;
    for _ in 0..fits {
        sp.fit_points.push(s.main.bd3()?);
    }
    Ok(ObjectData::Spline(sp))
}

/// VERTEX_2D: a negative start width means start = end = |width| and no end width.
pub fn vertex_2d(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut vx = PolylineVertex {
        flags: s.main.rc()?,
        position: s.main.bd3()?,
        ..PolylineVertex::default()
    };
    let width = s.main.bd()?;
    if width < 0.0 {
        vx.start_width = -width;
        vx.end_width = -width;
    } else {
        vx.start_width = width;
        vx.end_width = s.main.bd()?;
    }
    vx.bulge = s.main.bd()?;
    if s.version().r2010_plus() {
        vx.id = Some(s.main.bl()?);
    }
    vx.tangent = s.main.bd()?;
    Ok(ObjectData::Vertex(vx))
}

/// VERTEX_3D, VERTEX_MESH and VERTEX_PFACE: flags and a 3D point.
pub fn vertex_3d(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::Vertex(PolylineVertex {
        flags: s.main.rc()?,
        position: s.main.bd3()?,
        ..PolylineVertex::default()
    }))
}

/// VERTEX_PFACE_FACE: four 1-based vertex indices.
pub fn pface_face(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::PfaceFace(PfaceFace {
        indices: [s.main.bs()?, s.main.bs()?, s.main.bs()?, s.main.bs()?],
    }))
}

/// Owned-vertex count (R2004+) in the main stream, then the vertex handles (owned list
/// or first/last before R2004) and the SEQEND handle.
fn polyline_handles(s: &mut Streams<'_>, p: &mut Polyline, owned: i32) -> Result<()> {
    if s.version().r2004_plus() {
        p.vertices = read_handles(s, owned, "vertex")?;
    } else {
        p.vertices = vec![s.h()?, s.h()?];
        p.vertices_are_range = true;
    }
    p.seqend = s.h_opt()?;
    Ok(())
}

fn owned_count(s: &mut Streams<'_>) -> Result<i32> {
    if s.version().r2004_plus() {
        s.main.bl()
    } else {
        Ok(0)
    }
}

/// POLYLINE_2D.
pub fn polyline_2d(s: &mut Streams<'_>) -> Result<ObjectData> {
    let r2000 = s.version().r2000_plus();
    let mut p = Polyline {
        kind: PolylineKind::TwoD,
        flags: s.main.bs()?,
        curve_type: s.main.bs()?,
        start_width: s.main.bd()?,
        end_width: s.main.bd()?,
        thickness: s.main.bt(r2000)?,
        elevation: s.main.bd()?,
        extrusion: s.main.be(r2000)?,
        ..Polyline::default()
    };
    let owned = owned_count(s)?;
    polyline_handles(s, &mut p, owned)?;
    Ok(ObjectData::Polyline(p))
}

/// POLYLINE_3D: two flag bytes; the DXF 70 value is synthesized (1 closed, 4 splined,
/// 8 3D) and the curve type is 5 or 6 for the two spline flags.
pub fn polyline_3d(s: &mut Streams<'_>) -> Result<ObjectData> {
    let spline_flags = s.main.rc()?;
    let closed_flags = s.main.rc()?;
    let mut flags = 8;
    let mut curve_type = 0;
    if spline_flags & 1 != 0 {
        curve_type = 5;
    } else if spline_flags & 2 != 0 {
        curve_type = 6;
    }
    if spline_flags & 3 != 0 {
        flags |= 4;
    }
    if closed_flags & 1 != 0 {
        flags |= 1;
    }
    let mut p = Polyline {
        kind: PolylineKind::ThreeD,
        flags,
        curve_type,
        extrusion: [0.0, 0.0, 1.0],
        ..Polyline::default()
    };
    let owned = owned_count(s)?;
    polyline_handles(s, &mut p, owned)?;
    Ok(ObjectData::Polyline(p))
}

/// POLYLINE_PFACE: vertex and face counts.
pub fn polyline_pface(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut p = Polyline {
        kind: PolylineKind::PolyFace,
        flags: 64,
        m: s.main.bs()?,
        n: s.main.bs()?,
        extrusion: [0.0, 0.0, 1.0],
        ..Polyline::default()
    };
    let owned = owned_count(s)?;
    polyline_handles(s, &mut p, owned)?;
    Ok(ObjectData::Polyline(p))
}

/// POLYLINE_MESH: flags, curve type, M/N counts and densities.
pub fn polyline_mesh(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut p = Polyline {
        kind: PolylineKind::Mesh,
        flags: s.main.bs()?,
        curve_type: s.main.bs()?,
        m: s.main.bs()?,
        n: s.main.bs()?,
        m_density: s.main.bs()?,
        n_density: s.main.bs()?,
        extrusion: [0.0, 0.0, 1.0],
        ..Polyline::default()
    };
    let owned = owned_count(s)?;
    polyline_handles(s, &mut p, owned)?;
    Ok(ObjectData::Polyline(p))
}

fn solid_or_trace(s: &mut Streams<'_>, kind: FaceKind) -> Result<ObjectData> {
    let r2000 = s.version().r2000_plus();
    let thickness = s.main.bt(r2000)?;
    let elevation = s.main.bd()?;
    let mut corners = [[0.0; 3]; 4];
    for corner in &mut corners {
        let [x, y] = s.main.rd2()?;
        *corner = [x, y, elevation];
    }
    let extrusion = s.main.be(r2000)?;
    Ok(ObjectData::Face(Face {
        kind,
        corners,
        thickness,
        extrusion,
        invisible_edges: 0,
    }))
}

/// SOLID.
pub fn solid(s: &mut Streams<'_>) -> Result<ObjectData> {
    solid_or_trace(s, FaceKind::Solid)
}

/// TRACE.
pub fn trace(s: &mut Streams<'_>) -> Result<ObjectData> {
    solid_or_trace(s, FaceKind::Trace)
}

/// 3DFACE. R2000+: "no flags" bit, "Z is zero" bit, the first corner as raw doubles,
/// the others as `3DD` defaulting to the previous corner, then the optional edge flags.
pub fn face_3d(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut f = Face {
        kind: FaceKind::Face3d,
        extrusion: [0.0, 0.0, 1.0],
        ..Face::default()
    };
    if s.version().r13_14() {
        for corner in &mut f.corners {
            *corner = s.main.bd3()?;
        }
        f.invisible_edges = s.main.bs()?;
    } else {
        let no_flags = s.main.b()?;
        let z_zero = s.main.b()?;
        let x = s.main.rd()?;
        let y = s.main.rd()?;
        let z = if z_zero { 0.0 } else { s.main.rd()? };
        let mut prev = [x, y, z];
        f.corners[0] = prev;
        for corner in f.corners.iter_mut().skip(1) {
            prev = [
                s.main.dd(prev[0])?,
                s.main.dd(prev[1])?,
                s.main.dd(prev[2])?,
            ];
            *corner = prev;
        }
        if !no_flags {
            f.invisible_edges = s.main.bs()?;
        }
    }
    Ok(ObjectData::Face(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::testbits::{BitWriter, ctx, streams};
    use crate::version::DwgVersion;

    #[test]
    fn absurd_spline_degree_is_clamped() {
        // R2000 control-point spline (scenario 1) of degree 1000 without knots/points.
        let mut w = BitWriter::default();
        w.bl(1)
            .bl(1000)
            .bit(false)
            .bit(false)
            .bit(false)
            .bd(0.0)
            .bd(0.0)
            .bl(0)
            .bl(0)
            .bit(false);
        let data = w.bytes();
        let mut s = streams(&data, ctx(DwgVersion::R2000, 100));
        let ObjectData::Spline(sp) = spline(&mut s).unwrap() else {
            panic!("not a spline")
        };
        assert_eq!(sp.degree, 25);
        assert_eq!(s.warnings[0].code, "dwg.spline_degree_clamped");
    }
}
