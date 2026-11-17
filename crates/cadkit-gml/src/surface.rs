//! Planar surface candidates: closed rings from polygons, faces, mesh faces and closed,
//! straight polylines, checked so that every accepted ring passes the crate's own
//! polygon validation.

use cadkit_core::{Error, Point3, Result, Vec3, Vertex};

/// Ring-point to plane distances above this are "not planar" regardless of the relative
/// tolerance; matches the default of [`crate::ValidationOptions::planarity_tolerance`].
pub(crate) const DEFAULT_MAX_PLANARITY_DEVIATION: f64 = 0.01;
/// Default planarity tolerance as a fraction of a face's bounding-box diagonal.
pub(crate) const DEFAULT_PLANARITY_TOLERANCE: f64 = 1e-3;
/// Boundary-crossing tolerance; the default of [`crate::ValidationOptions::tolerance`].
const INTERSECTION_TOLERANCE: f64 = 1e-6;

/// Why a face candidate cannot be written as a `gml:Polygon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reject {
    Curved,
    Wide,
    Degenerate,
    NotPlanar,
    SelfIntersecting,
    Invalid,
    IndexOutOfRange,
}

impl Reject {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Curved => "gml.face_curved",
            Self::Wide => "gml.face_wide",
            Self::Degenerate => "gml.face_degenerate",
            Self::NotPlanar => "gml.face_not_planar",
            Self::SelfIntersecting => "gml.face_self_intersecting",
            Self::Invalid => "gml.face_invalid",
            Self::IndexOutOfRange => "gml.face_index_out_of_range",
        }
    }
}

/// Planarity contract: a ring point may sit at most
/// `min(relative * bbox diagonal, absolute)` coordinate units off the face plane.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Planarity {
    pub relative: f64,
    pub absolute: f64,
}

impl Planarity {
    pub(crate) fn check(self) -> Result<()> {
        if !(self.relative.is_finite() && self.relative > 0.0)
            || !(self.absolute.is_finite() && self.absolute > 0.0)
        {
            return Err(Error::invalid(
                0,
                "planarity tolerances must be finite and positive",
            ));
        }
        Ok(())
    }
}

/// A validated planar face: closed rings (first point repeated last), holes wound
/// against the exterior, unit normal of the exterior by the right-hand rule.
#[derive(Debug, Clone)]
pub(crate) struct Surface {
    pub exterior: Vec<Point3>,
    pub interiors: Vec<Vec<Point3>>,
    pub normal: Vec3,
    pub tolerance: f64,
    pub z_min: f64,
}

impl Surface {
    /// Reverses the winding of every ring (normal flips). The plane distances are
    /// recomputed because the summation order changes; false if they no longer fit.
    pub(crate) fn reverse(&mut self) -> bool {
        self.exterior.reverse();
        for ring in &mut self.interiors {
            ring.reverse();
        }
        match cadkit_core::polygon::normal(&self.exterior) {
            Some(n) => {
                self.normal = n;
                within_plane(&self.exterior, &self.interiors, n, self.tolerance)
            }
            None => false,
        }
    }
}

/// The ring of a closed polyline, `None` when the polyline is open (neither flagged
/// closed nor ending on its first vertex). Bulges and widths are not surfaces.
pub(crate) fn polyline_ring(
    vertices: &[Vertex],
    closed: bool,
) -> Option<std::result::Result<Vec<Point3>, Reject>> {
    let explicit = vertices.len() >= 4
        && vertices.first().map(|v| v.position) == vertices.last().map(|v| v.position);
    if !closed && !explicit {
        return None;
    }
    if vertices.iter().any(|v| v.bulge != 0.0) {
        return Some(Err(Reject::Curved));
    }
    if vertices
        .iter()
        .any(|v| v.start_width != 0.0 || v.end_width != 0.0)
    {
        return Some(Err(Reject::Wide));
    }
    Some(Ok(vertices.iter().map(|v| v.position).collect()))
}

/// Drops consecutive exact duplicates (a zero-length edge is invalid GML but carries no
/// geometry) and closes the ring by repeating its first point.
fn close_ring(points: Vec<Point3>) -> Vec<Point3> {
    let mut ring: Vec<Point3> = Vec::with_capacity(points.len().saturating_add(1));
    for p in points {
        if ring.last() != Some(&p) {
            ring.push(p);
        }
    }
    while ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    if let Some(first) = ring.first().copied() {
        ring.push(first);
    }
    ring
}

fn within_plane(exterior: &[Point3], interiors: &[Vec<Point3>], n: Vec3, tolerance: f64) -> bool {
    // Same reference point and normal as cadkit_core::polygon::validate, so a ring
    // accepted here is accepted by validation with an equal or larger tolerance.
    let Some(o) = exterior.first() else {
        return false;
    };
    std::iter::once(exterior)
        .chain(interiors.iter().map(Vec::as_slice))
        .flatten()
        .all(|p| ((p.x - o.x) * n.x + (p.y - o.y) * n.y + (p.z - o.z) * n.z).abs() <= tolerance)
}

/// Checks closure, degeneracy, planarity and crossings of one face. `Ok(Err(_))` is a
/// rejected face; `Err(_)` only an exhausted `work` budget.
pub(crate) fn prepare(
    exterior: Vec<Point3>,
    interiors: Vec<Vec<Point3>>,
    planarity: Planarity,
    work: &mut u64,
) -> Result<std::result::Result<Surface, Reject>> {
    let exterior = close_ring(exterior);
    if exterior.len() < 4 {
        return Ok(Err(Reject::Degenerate));
    }
    let mut holes = Vec::with_capacity(interiors.len());
    for ring in interiors {
        let ring = close_ring(ring);
        if ring.len() < 4 {
            return Ok(Err(Reject::Invalid));
        }
        holes.push(ring);
    }
    if std::iter::once(&exterior)
        .chain(holes.iter())
        .flatten()
        .any(|p| !p.is_finite())
    {
        return Ok(Err(Reject::Invalid));
    }
    let first = exterior.first().copied().unwrap_or_default();
    let (mut lo, mut hi) = (first, first);
    for p in &exterior {
        lo = Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    let size = lo.distance_to(hi);
    let tolerance = (planarity.relative * size).min(planarity.absolute);
    if !(size.is_finite() && size > 0.0 && tolerance > 0.0) {
        return Ok(Err(Reject::Degenerate));
    }
    let Some(normal) = cadkit_core::polygon::normal(&exterior) else {
        return Ok(Err(Reject::Degenerate));
    };
    // A face narrower than the planarity tolerance has no meaningful orientation.
    if area(&exterior) <= tolerance * size {
        return Ok(Err(Reject::Degenerate));
    }
    for ring in &mut holes {
        if let Some(h) = cadkit_core::polygon::normal(ring)
            && h.dot(normal) >= 0.0
        {
            ring.reverse();
        }
    }
    if !within_plane(&exterior, &holes, normal, tolerance) {
        return Ok(Err(Reject::NotPlanar));
    }
    match cadkit_core::polygon::validate(&exterior, &holes, tolerance, INTERSECTION_TOLERANCE, work)
    {
        Ok(()) => {}
        Err(Error::Invalid { message, .. }) => {
            return Ok(Err(if message.contains("intersect") {
                Reject::SelfIntersecting
            } else {
                Reject::Invalid
            }));
        }
        Err(other) => return Err(other),
    }
    let z_min = std::iter::once(&exterior)
        .chain(holes.iter())
        .flatten()
        .fold(f64::INFINITY, |z, p| z.min(p.z));
    Ok(Ok(Surface {
        exterior,
        interiors: holes,
        normal,
        tolerance,
        z_min,
    }))
}

/// Area of a closed ring (half the length of its vector area), in squared coordinate units.
fn area(ring: &[Point3]) -> f64 {
    let Some(o) = ring.first() else {
        return 0.0;
    };
    let mut n = Vec3::default();
    for edge in ring.windows(2) {
        if let (Some(a), Some(b)) = (edge.first(), edge.get(1)) {
            let u = Vec3::new(a.x - o.x, a.y - o.y, a.z - o.z);
            let v = Vec3::new(b.x - o.x, b.y - o.y, b.z - o.z);
            n = n.plus(u.cross(v));
        }
    }
    n.length() / 2.0
}
