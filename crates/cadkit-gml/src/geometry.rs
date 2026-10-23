//! Work-bounded diagnostics for polygons and solid shells.

use crate::{
    CityGmlDocument, ValidationOptions,
    native::{Element, GML, XLINK},
    validate::{elements, polygon, validate},
};
use cadkit_core::{Error, Point3, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A geometry diagnostic that omits source identifiers and coordinate values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeometryIssue {
    /// Byte offset in the source XML input, or zero for programmatically built elements.
    pub offset: u64,
    /// Stable machine-readable diagnostic code.
    pub code: String,
    /// Human-readable description of the issue.
    pub message: String,
}

/// Collects all polygon and solid-shell geometry issues after structural validation.
/// Shell edge matching welds vertices within `options.tolerance` and does not test inter-solid volumetric intersections.
pub fn geometry_issues(
    doc: &CityGmlDocument,
    options: &ValidationOptions,
) -> Result<Vec<GeometryIssue>> {
    validate(
        doc,
        &ValidationOptions {
            geometry: false,
            ..options.clone()
        },
    )?;
    inspect(&elements(doc, &options.limits)?, options)
}

pub(crate) fn inspect(
    nodes: &[(&Element, usize)],
    options: &ValidationOptions,
) -> Result<Vec<GeometryIssue>> {
    let mut issues = vec![];
    let mut work = options.max_geometry_work;
    let ids: HashMap<_, _> = nodes
        .iter()
        .filter_map(|&(e, d)| e.attribute(GML, "id").map(|id| (id, (e, d))))
        .collect();
    for &(e, dim) in nodes {
        if e.name.is(GML, "Polygon") {
            let p = polygon(e, dim, &options.limits)?;
            let checked = cadkit_core::polygon::validate(
                &p.exterior,
                &p.interiors,
                options.planarity_tolerance,
                options.tolerance,
                &mut work,
            );
            match checked {
                Err(Error::Invalid { message, .. }) => issues.push(GeometryIssue {
                    offset: e.offset,
                    code: "gml.polygon_geometry".into(),
                    message,
                }),
                Err(other) => return Err(other),
                Ok(()) => {
                    if let Some(n) = cadkit_core::polygon::normal(&p.exterior) {
                        if p.interiors
                            .iter()
                            .filter_map(|r| cadkit_core::polygon::normal(r))
                            .any(|h| n.x * h.x + n.y * h.y + n.z * h.z >= 0.0)
                        {
                            issues.push(GeometryIssue {
                                offset: e.offset,
                                code: "gml.ring_orientation".into(),
                                message: "interior ring orientation must oppose exterior".into(),
                            });
                        }
                    }
                }
            }
        }
        if e.name.is(GML, "Solid") {
            let mut exterior = 0;
            for boundary in e
                .elements()
                .filter(|b| b.name.is(GML, "exterior") || b.name.is(GML, "interior"))
            {
                let outer = boundary.name.is(GML, "exterior");
                exterior += usize::from(outer);
                if let Some(message) = shell(boundary, dim, outer, &ids, options, &mut work)? {
                    issues.push(GeometryIssue {
                        offset: boundary.offset,
                        code: "gml.solid_shell".into(),
                        message,
                    });
                }
            }
            if exterior != 1 {
                issues.push(GeometryIssue {
                    offset: e.offset,
                    code: "gml.solid_shell".into(),
                    message: "Solid requires exactly one exterior shell".into(),
                });
            }
        }
    }
    Ok(issues)
}

struct VertexWelder {
    tol: f64,
    tol2: f64,
    points: Vec<Point3>,
    grid: HashMap<(i64, i64, i64), Vec<u32>>,
}

impl VertexWelder {
    fn new(tolerance: f64) -> Self {
        let tol = if tolerance.is_finite() && tolerance > 0.0 {
            tolerance
        } else {
            0.0
        };
        Self {
            tol,
            tol2: tol * tol,
            points: Vec::new(),
            grid: HashMap::new(),
        }
    }

    fn cell(&self, p: Point3) -> (i64, i64, i64) {
        let step = self.tol.max(1e-12);
        let q = |v: f64| {
            (v / step)
                .floor()
                .clamp(i64::MIN as f64 + 2.0, i64::MAX as f64 - 2.0) as i64
        };
        (q(p.x), q(p.y), q(p.z))
    }

    fn intern(&mut self, p: Point3, work: &mut u64) -> Result<u32> {
        let (cx, cy, cz) = self.cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    // f64 cannot represent a one-cell margin at i64 bounds. A saturated
                    // coordinate has no neighbor beyond the boundary, so skip that key.
                    let (Some(x), Some(y), Some(z)) =
                        (cx.checked_add(dx), cy.checked_add(dy), cz.checked_add(dz))
                    else {
                        continue;
                    };
                    if let Some(bucket) = self.grid.get(&(x, y, z)) {
                        for &idx in bucket {
                            charge(work)?;
                            if let Some(&q) = self.points.get(idx as usize) {
                                let (ux, uy, uz) = (p.x - q.x, p.y - q.y, p.z - q.z);
                                let d2 = ux * ux + uy * uy + uz * uz;
                                if d2 <= self.tol2 && (self.tol > 0.0 || p == q) {
                                    return Ok(idx);
                                }
                            }
                        }
                    }
                }
            }
        }
        let idx = u32::try_from(self.points.len())
            .map_err(|_| Error::LimitExceeded("GML shell vertices".into()))?;
        self.points.push(p);
        self.grid.entry((cx, cy, cz)).or_default().push(idx);
        Ok(idx)
    }
}

fn charge(work: &mut u64) -> Result<()> {
    *work = work
        .checked_sub(1)
        .ok_or_else(|| Error::LimitExceeded("GML geometry work".into()))?;
    Ok(())
}

fn shell(
    start: &Element,
    dim: usize,
    outer: bool,
    ids: &HashMap<&str, (&Element, usize)>,
    options: &ValidationOptions,
    work: &mut u64,
) -> Result<Option<String>> {
    let mut welder = VertexWelder::new(options.tolerance);
    let mut edges: HashMap<(u32, u32), (u32, i32)> = HashMap::new();
    let mut todo = vec![(start, dim, 1i32, 0u32)];
    let mut volume = 0.0;
    let mut origin = None;
    while let Some((e, dim, sign, depth)) = todo.pop() {
        charge(work)?;
        if depth >= options.limits.max_depth.min(256) {
            return Err(Error::LimitExceeded("GML shell depth".into()));
        }
        let dim = crate::validate::dimension(e, dim)?;
        let sign =
            if e.name.is(GML, "OrientableSurface") && e.attribute("", "orientation") == Some("-") {
                -sign
            } else {
                sign
            };
        if let Some(id) = e.attribute(XLINK, "href").and_then(|h| h.strip_prefix('#')) {
            let &(target, d) = ids
                .get(id)
                .ok_or_else(|| Error::invalid(e.offset, "unresolved shell reference"))?;
            todo.push((target, d, sign, depth + 1));
        }
        if e.name.is(GML, "Polygon") {
            let p = polygon(e, dim, &options.limits)?;
            for ring in std::iter::once(&p.exterior).chain(p.interiors.iter()) {
                for pair in ring.windows(2) {
                    charge(work)?;
                    if let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) {
                        let a_key = welder.intern(a, work)?;
                        let b_key = welder.intern(b, work)?;
                        if a_key == b_key {
                            continue;
                        }
                        let (edge, direction) = if a_key < b_key {
                            ((a_key, b_key), sign)
                        } else {
                            ((b_key, a_key), -sign)
                        };
                        let entry = edges.entry(edge).or_default();
                        if entry.0 >= 2 {
                            return Ok(Some("shell edge occurs more than twice".into()));
                        }
                        entry.0 += 1;
                        entry.1 += direction;
                    }
                }
                if let Some(&a) = ring.first() {
                    let o: Point3 = *origin.get_or_insert(a);
                    for pair in ring.get(1..).unwrap_or(&[]).windows(2) {
                        if let (Some(b), Some(c)) = (pair.first(), pair.get(1)) {
                            let (ax, ay, az) = (a.x - o.x, a.y - o.y, a.z - o.z);
                            let (bx, by, bz) = (b.x - o.x, b.y - o.y, b.z - o.z);
                            let (cx, cy, cz) = (c.x - o.x, c.y - o.y, c.z - o.z);
                            volume += f64::from(sign)
                                * (ax * (by * cz - bz * cy)
                                    + ay * (bz * cx - bx * cz)
                                    + az * (bx * cy - by * cx))
                                / 6.0;
                        }
                    }
                }
            }
        } else {
            for child in e.elements() {
                todo.push((child, dim, sign, depth + 1));
            }
        }
    }
    if edges.is_empty()
        || edges
            .values()
            .any(|&(count, balance)| count != 2 || balance != 0)
    {
        return Ok(Some(
            "shell edges must occur exactly twice with opposing directions".into(),
        ));
    }
    if !volume.is_finite() || volume.abs() <= options.tolerance.powi(3) || (volume > 0.0) != outer {
        return Ok(Some(
            "shell volume is degenerate or its orientation is reversed".into(),
        ));
    }
    Ok(None)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn vertex_grid_neighbors_handle_saturated_cells() {
        let mut welder = VertexWelder::new(1e-6);
        let mut work = 100;
        for point in [
            Point3::new(f64::MAX, f64::MAX, f64::MAX),
            Point3::new(-f64::MAX, -f64::MAX, -f64::MAX),
            Point3::new(f64::MAX, -f64::MAX, 0.0),
        ] {
            let id = welder.intern(point, &mut work).expect("finite vertex");
            assert_eq!(welder.intern(point, &mut work).expect("same vertex"), id);
        }
        assert_eq!(welder.points.len(), 3);
        let origin = welder.intern(Point3::default(), &mut work).expect("origin");
        assert_eq!(
            welder
                .intern(Point3::new(5e-7, 0.0, 0.0), &mut work)
                .expect("near origin"),
            origin
        );
    }
}
