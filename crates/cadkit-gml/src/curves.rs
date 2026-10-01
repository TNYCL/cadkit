//! Exact three-point circle encoding from the public GML 3.1.1 geometry schema.

use crate::{
    native::{Element, GML},
    validate::{dimension, positions},
};
use cadkit_core::{EntityKind, Error, Limits, Point3, Result, Vec3};

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
    decode(&points, 0)?;
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

pub(crate) fn read_circle(
    curve: &Element,
    inherited: usize,
    limits: &Limits,
) -> Result<Option<EntityKind>> {
    let children: Vec<_> = curve
        .elements()
        .filter(|e| e.name.is(GML, "segments"))
        .collect();
    let [segments] = children.as_slice() else {
        return Ok(None);
    };
    let mut children = segments.elements();
    let Some(segment) = children.next() else {
        return Ok(None);
    };
    if children.next().is_some() || !segment.name.is(GML, "Circle") {
        return Ok(None);
    }
    match read_segment(segment, dimension(segments, inherited)?, limits) {
        Ok(kind) => Ok(Some(kind)),
        Err(Error::Unsupported(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

pub(crate) fn read_segment(
    segment: &Element,
    inherited: usize,
    limits: &Limits,
) -> Result<EntityKind> {
    if segment.attribute("", "numArc").is_some_and(|v| v != "1")
        || segment
            .attribute("", "interpolation")
            .is_some_and(|v| v != "circularArc3Points")
    {
        return Err(Error::invalid(
            segment.offset,
            "invalid GML Circle interpolation",
        ));
    }
    let dim = dimension(segment, inherited)?;
    let mut circle_limits = *limits;
    circle_limits.max_vertices = circle_limits.max_vertices.min(3);
    let mut points = vec![];
    for p in segment.elements() {
        if !p.name.is(GML, "pos") && !p.name.is(GML, "posList") {
            return Err(Error::Unsupported(
                "GML Circle requires explicit pos or posList coordinates".into(),
            ));
        }
        let part = positions(p, dim, &circle_limits)?;
        if points.len().saturating_add(part.len()) > 3 {
            return Err(Error::invalid(
                segment.offset,
                "GML Circle requires three positions",
            ));
        }
        points.extend(part);
    }
    decode(&points, segment.offset)
}

fn decode(points: &[Point3], offset: u64) -> Result<EntityKind> {
    let [a, b, c] = points else {
        return Err(Error::invalid(
            offset,
            "GML Circle requires three positions",
        ));
    };
    let u = a.vector_to(*b);
    let v = a.vector_to(*c);
    let scale = u.length().max(v.length());
    if !scale.is_finite() || scale <= 0.0 {
        return Err(Error::invalid(offset, "degenerate GML Circle"));
    }
    // Normalize lengths before the circumcenter calculation to avoid squaring
    // world-coordinate magnitudes; translation has already removed the origin.
    let u = u.scaled(1.0 / scale);
    let v = v.scaled(1.0 / scale);
    let w = u.cross(v);
    let divisor = 2.0 * w.dot(w);
    let Some(normal) = w.normalized() else {
        return Err(Error::invalid(offset, "collinear GML Circle"));
    };
    let delta = v
        .cross(w)
        .scaled(u.dot(u))
        .plus(w.cross(u).scaled(v.dot(v)))
        .scaled(scale / divisor);
    let center = a.translated(delta);
    let radius = delta.length();
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return Err(Error::invalid(offset, "invalid GML Circle coordinates"));
    }
    Ok(EntityKind::Circle {
        center,
        radius,
        normal,
    })
}
