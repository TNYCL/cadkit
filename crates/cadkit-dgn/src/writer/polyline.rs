//! Exact line/arc decomposition of zero-width bulged polylines.

use cadkit_core::{Entity, EntityKind, Error, Result, Vec3, Vertex};

pub(super) fn segments(
    source: &Entity,
    vertices: &[Vertex],
    closed: bool,
    normal: Vec3,
) -> Result<Vec<Entity>> {
    let n = normal
        .normalized()
        .ok_or_else(|| Error::invalid(0, "invalid bulge plane normal"))?;
    if vertices.len() < 2 {
        return Err(Error::invalid(0, "bulged polyline requires two vertices"));
    }
    let mut children = Vec::new();
    let count = if closed {
        vertices.len()
    } else {
        vertices.len() - 1
    };
    for i in 0..count {
        let first = vertices
            .get(i)
            .ok_or_else(|| Error::invalid(0, "missing polyline vertex"))?;
        let next = vertices
            .get((i + 1) % vertices.len())
            .ok_or_else(|| Error::invalid(0, "missing polyline endpoint"))?;
        let (start, end, bulge) = (first.position, next.position, first.bulge);
        if !start.is_finite() || !end.is_finite() || !bulge.is_finite() {
            return Err(Error::invalid(0, "non-finite polyline segment"));
        }
        if start == end && bulge == 0.0 {
            continue;
        }
        let kind = if bulge == 0.0 {
            EntityKind::Line { start, end }
        } else {
            let chord = start.vector_to(end);
            let distance = chord.length();
            if distance <= 0.0 || !distance.is_finite() || chord.dot(n).abs() > 1e-9 * distance {
                return Err(Error::invalid(0, "degenerate or out-of-plane bulge chord"));
            }
            // tan(sweep/4) gives the signed center offset from the chord midpoint.
            let radius = distance * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
            let offset = distance * (1.0 - bulge * bulge) / (4.0 * bulge);
            let center = start.translated(
                chord
                    .scaled(0.5)
                    .plus(n.cross(chord.scaled(1.0 / distance)).scaled(offset)),
            );
            if !radius.is_finite() || !center.is_finite() {
                return Err(Error::invalid(0, "bulge arc overflow"));
            }
            let normal = n.scaled(bulge.signum());
            let (x, y, _) = cadkit_core::geom_ops::arbitrary_axes(normal);
            let radial = center.vector_to(start);
            let start_angle = radial.dot(y).atan2(radial.dot(x));
            EntityKind::Arc {
                center,
                radius,
                normal,
                start_angle,
                end_angle: start_angle + 4.0 * bulge.abs().atan(),
            }
        };
        let mut child = Entity::new(kind);
        child.layer = source.layer.clone();
        child.color = source.color;
        child.linetype = source.linetype.clone();
        child.lineweight = source.lineweight;
        child.visible = source.visible;
        for key in ["dgn.color_index", "dgn.style", "dgn.weight"] {
            if let Some(value) = source.props.get(key) {
                child.props.insert(key.into(), value.clone());
            }
        }
        children.push(child);
    }
    Ok(children)
}
