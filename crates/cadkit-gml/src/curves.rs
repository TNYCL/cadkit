//! Exact three-point circle and circular-arc encoding/decoding from GML 3.1.1.

use cadkit_core::LimitKind;
use std::f64::consts::TAU;

use crate::{
    native::{Element, GML},
    validate::{dimension, positions},
};
use cadkit_core::{EntityKind, Error, Limits, Point3, Result, Vec3, Vertex};

pub(crate) fn circle(center: Point3, radius: f64, normal: Vec3) -> Result<Element> {
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 || normal.normalized().is_none()
    {
        return Err(Error::invalid(0, "invalid CityGML circle"));
    }
    let (x, y, _) = cadkit_core::geom_ops::arbitrary_axes(normal);
    // Three distinct points define the complete circle, not an approximation.
    let points = [
        center.translated(x.scaled(radius)),
        center.translated(y.scaled(radius)),
        center.translated(x.scaled(-radius)),
    ];
    if points.iter().any(|p| !p.is_finite()) {
        return Err(Error::invalid(0, "CityGML circle coordinate overflow"));
    }
    // Extremely small radii relative to the coordinate origin may collapse in f64.
    decode_circle(&points, 0)?;
    let mut segment = Element::new(GML, "gml:Circle");
    segment.set_attribute("", "numArc", "1");
    segment.set_attribute("", "interpolation", "circularArc3Points");
    for p in points {
        segment.push(Element::with_text(
            GML,
            "gml:pos",
            format!("{} {} {}", p.x, p.y, p.z),
        ));
    }
    let mut segments = Element::new(GML, "gml:segments");
    segments.push(segment);
    let mut curve = Element::new(GML, "gml:Curve");
    curve.push(segments);
    Ok(curve)
}

pub(crate) fn arc(
    center: Point3,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    normal: Vec3,
) -> Result<Element> {
    if !center.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
        || !start_angle.is_finite()
        || !end_angle.is_finite()
        || normal.normalized().is_none()
    {
        return Err(Error::invalid(0, "invalid CityGML arc"));
    }
    let sweep = cadkit_core::geom_ops::ccw_sweep(start_angle, end_angle);
    if sweep >= TAU - 1e-12 {
        return circle(center, radius, normal);
    }
    if sweep <= 1e-12 {
        return Err(Error::invalid(0, "zero-sweep CityGML arc"));
    }
    let (x, y, _) = cadkit_core::geom_ops::arbitrary_axes(normal);
    let at = |ang: f64| {
        center
            .translated(x.scaled(radius * ang.cos()))
            .translated(y.scaled(radius * ang.sin()))
    };
    let points = [
        at(start_angle),
        at(start_angle + sweep * 0.5),
        at(start_angle + sweep),
    ];
    if points.iter().any(|p| !p.is_finite()) {
        return Err(Error::invalid(0, "CityGML arc coordinate overflow"));
    }
    if !matches!(decode_arc(&points, 0)?, EntityKind::Arc { .. }) {
        return Err(Error::invalid(0, "degenerate CityGML arc"));
    }
    let mut segment = Element::new(GML, "gml:Arc");
    segment.set_attribute("", "numArc", "1");
    segment.set_attribute("", "interpolation", "circularArc3Points");
    for p in points {
        segment.push(Element::with_text(
            GML,
            "gml:pos",
            format!("{} {} {}", p.x, p.y, p.z),
        ));
    }
    let mut segments = Element::new(GML, "gml:segments");
    segments.push(segment);
    let mut curve = Element::new(GML, "gml:Curve");
    curve.push(segments);
    Ok(curve)
}

/// Decodes a `gml:Curve` element into a `Circle`, `Arc`, `Line`, or chained `Polyline`.
pub(crate) fn read_curve(
    curve: &Element,
    inherited: usize,
    limits: &Limits,
) -> Result<Option<(EntityKind, u64)>> {
    let seg_containers: Vec<_> = curve
        .elements()
        .filter(|e| e.name.is(GML, "segments"))
        .collect();
    let [segments] = seg_containers.as_slice() else {
        return Ok(None);
    };
    let seg_dim = dimension(segments, inherited)?;
    let seg_elems: Vec<_> = segments.elements().collect();
    if seg_elems.is_empty() {
        return Ok(None);
    }
    if let [single] = seg_elems.as_slice() {
        if single.name.is(GML, "Circle") {
            return match read_segment(single, seg_dim, limits) {
                Ok(kind) => Ok(Some((kind, 3))),
                Err(Error::Unsupported(_)) => Ok(None),
                Err(e) => Err(e),
            };
        }
        if single.name.is(GML, "Arc") {
            return match read_arc_segment(single, seg_dim, limits) {
                Ok(kind) => Ok(Some((kind, 3))),
                Err(Error::Unsupported(_)) => Ok(None),
                Err(e) => Err(e),
            };
        }
        if single.name.is(GML, "LineStringSegment") {
            let pts = match segment_points(single, seg_dim, limits) {
                Ok(pts) => pts,
                Err(Error::Unsupported(_)) => return Ok(None),
                Err(e) => return Err(e),
            };
            let count = pts.len() as u64;
            return Ok(Some((
                EntityKind::Polyline {
                    vertices: pts.into_iter().map(Vertex::at).collect(),
                    closed: false,
                    normal: Vec3::Z,
                },
                count,
            )));
        }
        return Ok(None);
    }

    let mut chained: Vec<Point3> = Vec::new();
    for seg in seg_elems {
        let part = match segment_points(seg, seg_dim, limits) {
            Ok(pts) => pts,
            Err(Error::Unsupported(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        append_chain(&mut chained, &part, limits)?;
    }
    if chained.len() < 2 {
        return Err(Error::invalid(
            curve.offset,
            "GML Curve requires at least two positions",
        ));
    }
    let count = chained.len() as u64;
    Ok(Some((
        EntityKind::Polyline {
            vertices: chained.into_iter().map(Vertex::at).collect(),
            closed: false,
            normal: Vec3::Z,
        },
        count,
    )))
}

pub(crate) fn append_chain(dst: &mut Vec<Point3>, part: &[Point3], limits: &Limits) -> Result<()> {
    let skip =
        usize::from(!dst.is_empty() && dst.last().zip(part.first()).is_some_and(|(a, b)| a == b));
    let added = part.len().saturating_sub(skip);
    if dst.len().saturating_add(added) as u64 > u64::from(limits.max_vertices) {
        return Err(Error::limit(
            LimitKind::Vertices,
            "GML curve vertices".into(),
        ));
    }
    if let Some(tail) = part.get(skip..) {
        dst.extend_from_slice(tail);
    }
    Ok(())
}

/// Samples a `gml:LineStringSegment` or `gml:Arc` segment into ordered coordinates,
/// preserving exact first and last endpoints.
pub(crate) fn segment_points(
    segment: &Element,
    inherited: usize,
    limits: &Limits,
) -> Result<Vec<Point3>> {
    if segment.name.is(GML, "LineStringSegment") {
        let dim = dimension(segment, inherited)?;
        let mut pts = Vec::new();
        for p in segment.elements() {
            if p.name.is(GML, "pos") || p.name.is(GML, "posList") {
                let values = positions(p, dim, limits)?;
                if pts.len().saturating_add(values.len()) as u64 > u64::from(limits.max_vertices) {
                    return Err(Error::limit(
                        LimitKind::Vertices,
                        "GML segment vertices".into(),
                    ));
                }
                pts.extend(values);
            } else if !p.name.is(GML, "interpolation") {
                return Err(Error::Unsupported(format!(
                    "LineStringSegment child {}",
                    p.name.local()
                )));
            }
        }
        if pts.len() < 2 {
            return Err(Error::invalid(
                segment.offset,
                "LineStringSegment requires at least two positions",
            ));
        }
        return Ok(pts);
    }
    if segment.name.is(GML, "Arc") {
        let points = three_point_coords(segment, inherited, limits, "Arc")?;
        let [start_pt, _, end_pt] = points.as_slice() else {
            return Err(Error::invalid(
                segment.offset,
                "GML Arc requires three positions",
            ));
        };
        return match decode_arc(&points, segment.offset)? {
            EntityKind::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                normal,
            } => {
                let sweep = cadkit_core::geom_ops::ccw_sweep(start_angle, end_angle);
                let (ax, ay, _) = cadkit_core::geom_ops::arbitrary_axes(normal);
                let steps = 16usize;
                let mut out = Vec::with_capacity(steps + 1);
                out.push(*start_pt);
                for i in 1..steps {
                    let ang = start_angle + sweep * (i as f64 / steps as f64);
                    out.push(
                        center
                            .translated(ax.scaled(radius * ang.cos()))
                            .translated(ay.scaled(radius * ang.sin())),
                    );
                }
                out.push(*end_pt);
                Ok(out)
            }
            EntityKind::Line { start, end } => Ok(vec![start, end]),
            _ => Err(Error::invalid(segment.offset, "unexpected GML Arc decode")),
        };
    }
    Err(Error::Unsupported(format!(
        "GML curve segment {}",
        segment.name.local()
    )))
}

pub(crate) fn read_segment(
    segment: &Element,
    inherited: usize,
    limits: &Limits,
) -> Result<EntityKind> {
    let points = three_point_coords(segment, inherited, limits, "Circle")?;
    decode_circle(&points, segment.offset)
}

pub(crate) fn read_arc_segment(
    segment: &Element,
    inherited: usize,
    limits: &Limits,
) -> Result<EntityKind> {
    let points = three_point_coords(segment, inherited, limits, "Arc")?;
    decode_arc(&points, segment.offset)
}

fn three_point_coords(
    segment: &Element,
    inherited: usize,
    limits: &Limits,
    label: &str,
) -> Result<Vec<Point3>> {
    if segment.attribute("", "numArc").is_some_and(|v| v != "1")
        || segment
            .attribute("", "interpolation")
            .is_some_and(|v| v != "circularArc3Points")
    {
        return Err(Error::invalid(
            segment.offset,
            format!("invalid GML {label} interpolation"),
        ));
    }
    let dim = dimension(segment, inherited)?;
    let mut seg_limits = *limits;
    seg_limits.max_vertices = seg_limits.max_vertices.min(3);
    let mut points = vec![];
    for p in segment.elements() {
        if !p.name.is(GML, "pos") && !p.name.is(GML, "posList") {
            return Err(Error::Unsupported(format!(
                "GML {label} requires explicit pos or posList coordinates"
            )));
        }
        let part = positions(p, dim, &seg_limits)?;
        if points.len().saturating_add(part.len()) > 3 {
            return Err(Error::invalid(
                segment.offset,
                format!("GML {label} requires three positions"),
            ));
        }
        points.extend(part);
    }
    if points.len() != 3 {
        return Err(Error::invalid(
            segment.offset,
            format!("GML {label} requires three positions"),
        ));
    }
    Ok(points)
}

fn circumcircle(
    points: &[Point3],
    offset: u64,
    label: &str,
) -> Result<Option<(Point3, f64, Vec3)>> {
    let [a, b, c] = points else {
        return Err(Error::invalid(
            offset,
            format!("GML {label} requires three positions"),
        ));
    };
    let u = a.vector_to(*b);
    let v = a.vector_to(*c);
    let scale = u.length().max(v.length());
    if !scale.is_finite()
        || scale <= 0.0
        || u.length() <= 1e-12 * scale
        || v.length() <= 1e-12 * scale
        || b.distance_to(*c) <= 1e-12 * scale
    {
        return Err(Error::invalid(offset, format!("degenerate GML {label}")));
    }
    // Normalize lengths before the circumcenter calculation to avoid squaring
    // world-coordinate magnitudes; translation has already removed the origin.
    let u_s = u.scaled(1.0 / scale);
    let v_s = v.scaled(1.0 / scale);
    let w = u_s.cross(v_s);
    let divisor = 2.0 * w.dot(w);
    let Some(normal) = w.normalized() else {
        return Ok(None);
    };
    let delta = v_s
        .cross(w)
        .scaled(u_s.dot(u_s))
        .plus(w.cross(u_s).scaled(v_s.dot(v_s)))
        .scaled(scale / divisor);
    let center = a.translated(delta);
    let radius = delta.length();
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return Err(Error::invalid(
            offset,
            format!("invalid GML {label} coordinates"),
        ));
    }
    Ok(Some((center, radius, normal)))
}

fn decode_circle(points: &[Point3], offset: u64) -> Result<EntityKind> {
    let Some((center, radius, normal)) = circumcircle(points, offset, "Circle")? else {
        return Err(Error::invalid(offset, "collinear GML Circle"));
    };
    Ok(EntityKind::Circle {
        center,
        radius,
        normal,
    })
}

fn decode_arc(points: &[Point3], offset: u64) -> Result<EntityKind> {
    let [a, b, c] = points else {
        return Err(Error::invalid(offset, "GML Arc requires three positions"));
    };
    match circumcircle(points, offset, "Arc")? {
        Some((center, radius, normal)) => {
            let (ax, ay, _) = cadkit_core::geom_ops::arbitrary_axes(normal);
            let angle_of = |p: Point3| {
                let d = center.vector_to(p);
                d.dot(ay).atan2(d.dot(ax)).rem_euclid(TAU)
            };
            Ok(EntityKind::Arc {
                center,
                radius,
                start_angle: angle_of(*a),
                end_angle: angle_of(*c),
                normal,
            })
        }
        None => {
            // Collinear fallback when the midpoint lies strictly between start and end.
            let ab = a.vector_to(*b);
            let bc = b.vector_to(*c);
            if ab.dot(bc) > 0.0 {
                Ok(EntityKind::Line { start: *a, end: *c })
            } else {
                Err(Error::invalid(offset, "collinear folding GML Arc"))
            }
        }
    }
}
