//! [`surface_mesh`]: the faces of a model as triangles, plus face outlines and wire geometry
//! as segments, in the flat buffers of [`SurfaceMesh`].

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ops::Range;

use cadkit_core::ops::WalkOptions;
use cadkit_core::{Document, Point3};

use super::collect::{
    self, Collected, Encoder, Item, Ring, Segments, Settings, drop_axis, extend_bounds, project,
};
use crate::SceneError;
use crate::buffers::{LayerRange, SurfaceMesh, SurfaceReport};
use crate::triangulate::triangulate;

/// Largest triangle or segment budget accepted, so every index fits a `u32`.
pub(super) const MAX_ELEMENTS: usize = 1 << 28;

/// When closed polylines are drawn as faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PolylineSurfaces {
    /// Never: closed polylines are wires.
    Never,
    /// Always, when they are bulge-free, planar and have at least three distinct vertices.
    Always,
    /// As [`Self::Always`] in models flagged 3D (`Model::is_3d`), as [`Self::Never`] in
    /// others. DGN shapes arrive as closed polylines, and 3D building models are mostly made
    /// of them, while in 2D drawings closed polylines are outlines (rooms, plots).
    #[default]
    Auto,
}

/// Options of [`surface_mesh`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SurfaceOptions {
    /// Index into `Document::models`.
    pub model: usize,
    /// Traversal depth and work budget of the walk.
    pub walk: WalkOptions,
    /// Which closed polylines become faces. Default [`PolylineSurfaces::Auto`].
    pub polylines: PolylineSurfaces,
    /// Largest distance of a polyline face's vertex from the plane through its first vertex,
    /// as a fraction of the polyline's bounding-box diagonal. Default 1e-3.
    pub planarity: f64,
    /// Draw lines, open polylines, closed polylines that are not faces and curves as
    /// segments. Default true.
    pub wires: bool,
    /// Draw the outline of every face as segments. Default true.
    pub outlines: bool,
    /// Largest chordal error of flattened curves, in drawing units; `None` derives it from
    /// `Document::units` (0.1 mm, or 1e-4 drawing units when unitless).
    pub tolerance: Option<f64>,
    /// Fail with [`SceneError::Precision`] when a stored f32 coordinate is further than this
    /// from the drawing coordinate (drawing units). `None` (default) accepts any finite error;
    /// [`SurfaceMesh::max_error`] reports the largest one either way.
    pub max_error: Option<f64>,
    /// Color of faces whose color resolves to black: ACI 7 (the foreground color, black on a
    /// white sheet) and missing colors. `None` keeps them black. Default `[201, 204, 210]`.
    pub neutral_color: Option<[u8; 3]>,
    /// Background segment colors are contrasted against, as in the SVG export
    /// (`cadkit_core::svg::with_contrast`). Face outlines start from their face's color
    /// (after [`Self::neutral_color`]). `None` (default) keeps colors as resolved.
    pub line_background: Option<[u8; 3]>,
    /// Most triangles. Default 2,000,000; at most 2^28.
    pub max_triangles: usize,
    /// Most segments. Default 2,000,000; at most 2^28.
    pub max_segments: usize,
    /// Work budget of duplicate detection, hatch nesting and triangulation, in abstract
    /// units (about one per point visited). Default 200,000,000.
    pub max_work: u64,
}

impl Default for SurfaceOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceOptions {
    /// Model 0 with the defaults listed on each field.
    pub fn new() -> Self {
        Self {
            model: 0,
            walk: WalkOptions::new(),
            polylines: PolylineSurfaces::Auto,
            planarity: 1e-3,
            wires: true,
            outlines: true,
            tolerance: None,
            max_error: None,
            neutral_color: Some([201, 204, 210]),
            line_background: None,
            max_triangles: 2_000_000,
            max_segments: 2_000_000,
            max_work: 200_000_000,
        }
    }

    /// Sets [`Self::model`].
    #[must_use]
    pub fn with_model(mut self, model: usize) -> Self {
        self.model = model;
        self
    }

    /// Sets [`Self::walk`].
    #[must_use]
    pub fn with_walk(mut self, walk: WalkOptions) -> Self {
        self.walk = walk;
        self
    }

    /// Sets [`Self::polylines`].
    #[must_use]
    pub fn with_polylines(mut self, polylines: PolylineSurfaces) -> Self {
        self.polylines = polylines;
        self
    }

    /// Sets [`Self::planarity`].
    #[must_use]
    pub fn with_planarity(mut self, planarity: f64) -> Self {
        self.planarity = planarity;
        self
    }

    /// Sets [`Self::wires`].
    #[must_use]
    pub fn with_wires(mut self, wires: bool) -> Self {
        self.wires = wires;
        self
    }

    /// Sets [`Self::outlines`].
    #[must_use]
    pub fn with_outlines(mut self, outlines: bool) -> Self {
        self.outlines = outlines;
        self
    }

    /// Sets [`Self::tolerance`].
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = Some(tolerance);
        self
    }

    /// Sets [`Self::max_error`].
    #[must_use]
    pub fn with_max_error(mut self, max_error: Option<f64>) -> Self {
        self.max_error = max_error;
        self
    }

    /// Sets [`Self::neutral_color`].
    #[must_use]
    pub fn with_neutral_color(mut self, neutral_color: Option<[u8; 3]>) -> Self {
        self.neutral_color = neutral_color;
        self
    }

    /// Sets [`Self::line_background`].
    #[must_use]
    pub fn with_line_background(mut self, line_background: Option<[u8; 3]>) -> Self {
        self.line_background = line_background;
        self
    }

    /// Sets [`Self::max_triangles`].
    #[must_use]
    pub fn with_max_triangles(mut self, max_triangles: usize) -> Self {
        self.max_triangles = max_triangles;
        self
    }

    /// Sets [`Self::max_segments`].
    #[must_use]
    pub fn with_max_segments(mut self, max_segments: usize) -> Self {
        self.max_segments = max_segments;
        self
    }

    /// Sets [`Self::max_work`].
    #[must_use]
    pub fn with_max_work(mut self, max_work: u64) -> Self {
        self.max_work = max_work;
        self
    }
}

/// Checks the options shared by both builders.
pub(super) fn check(
    tolerance: Option<f64>,
    max_error: Option<f64>,
    max_segments: usize,
) -> Result<(), SceneError> {
    let positive = |v: f64| v.is_finite() && v > 0.;
    if tolerance.is_some_and(|t| !positive(t)) {
        return Err(SceneError::InvalidBudget { name: "tolerance" });
    }
    if max_error.is_some_and(|e| !positive(e)) {
        return Err(SceneError::InvalidBudget { name: "max_error" });
    }
    if max_segments > MAX_ELEMENTS {
        return Err(SceneError::InvalidBudget {
            name: "max_segments",
        });
    }
    Ok(())
}

pub(super) fn relative(p: [f64; 3], origin: [f64; 3]) -> [f64; 3] {
    [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]]
}

/// The faces of `options.model` as triangles, with face outlines and wire geometry as
/// segments.
///
/// Every visible entity is visited with blocks and arrays expanded and their placement
/// applied (`Document::walk_while`). Faces are `Polygon`s (with holes), `Face`s, every face of a
/// `Mesh`, the regions of solid `Hatch`es whose boundary edges are all straight (lines and
/// bulge-free polylines; loops are nested even-odd), and closed polylines as
/// [`SurfaceOptions::polylines`] says. Wires are lines, leaders, other polylines and curves
/// (flattened within [`SurfaceOptions::tolerance`]). Everything else is counted in
/// [`SurfaceReport::skipped`].
///
/// Two faces with the same rings are drawn once, as the one with the longer walk path. Rings
/// are the same when they visit the same corners in the same cyclic order, from any corner
/// and either way round; holes count, and corners are compared rounded to a millionth of the
/// face's own size, so a face and the same face with a hole, or two different polygons
/// through one set of corners, are both drawn. A CityGML building lists
/// the faces of its solid and again each boundary surface (wall, roof, ground), whose nested
/// copy carries the surface type as its layer. Colors are resolved like the SVG export
/// (`Entity::resolved_color_in`: `ByLayer`, `ByBlock` through block references).
///
/// Positions are f32 offsets from [`SurfaceMesh::origin`], the per-axis median of the model's
/// points, so map coordinates keep their precision ([`SurfaceMesh::max_error`]); an item
/// with a coordinate beyond f32 range relative to it is left out (counted as degenerate)
/// unless [`SurfaceOptions::max_error`] is set, which turns any excess into an error. Faces are processed in
/// walk order; when a budget runs out the rest is left out and
/// [`SurfaceReport::truncated`] is set. A missing model gives an empty mesh.
///
/// # Errors
///
/// [`SceneError::InvalidBudget`] for an option out of range (a non-positive tolerance or
/// error budget, a negative planarity, budgets above 2^28); [`SceneError::Precision`] when a
/// coordinate exceeds [`SurfaceOptions::max_error`] (`vertex` is the index of the vertex,
/// or of the segment end point, that failed).
pub fn surface_mesh(doc: &Document, options: &SurfaceOptions) -> Result<SurfaceMesh, SceneError> {
    check(options.tolerance, options.max_error, options.max_segments)?;
    if options.max_triangles > MAX_ELEMENTS {
        return Err(SceneError::InvalidBudget {
            name: "max_triangles",
        });
    }
    if !options.planarity.is_finite() || options.planarity < 0. {
        return Err(SceneError::InvalidBudget { name: "planarity" });
    }
    let is_3d = doc.models.get(options.model).is_some_and(|m| m.is_3d);
    let settings = Settings {
        model: options.model,
        walk: options.walk,
        faces: true,
        polyline_faces: match options.polylines {
            PolylineSurfaces::Never => false,
            PolylineSurfaces::Always => true,
            PolylineSurfaces::Auto => is_3d,
        },
        planarity: options.planarity,
        wires: options.wires,
        tolerance: options
            .tolerance
            .unwrap_or_else(|| super::default_tolerance(doc)),
        neutral: options.neutral_color,
        background: options.line_background,
        // Each point of a face gives at least a third of a triangle (duplicates included
        // twice), each wire point at most one segment.
        max_points: options
            .max_triangles
            .saturating_mul(6)
            .saturating_add(options.max_segments.saturating_mul(2))
            .saturating_add(64),
    };
    let mut work = options.max_work;
    let collected = collect::collect(doc, &settings, &mut work);
    let mut report = SurfaceReport {
        degenerate: collected.degenerate,
        flattened: collected.flattened,
        skipped: collected.skipped.clone(),
        truncated: collected.truncated,
        ..SurfaceReport::default()
    };
    let origin = collected.median().unwrap_or_default();
    let sequence = deduplicate(&collected, origin, &mut work, &mut report);
    let (mut drawn, triangles) = triangulate_faces(
        &collected,
        &sequence,
        options.max_triangles,
        &mut work,
        &mut report,
    );
    let mut used = 0usize;
    let mut exhausted = false;
    for d in &mut drawn {
        let Some(item) = collected.items.get(d.item) else {
            continue;
        };
        if item.surface && !options.outlines {
            d.segments = false;
            continue;
        }
        let n = collected
            .rings_of(item)
            .iter()
            .map(Ring::segments)
            .fold(0usize, usize::saturating_add);
        match used.checked_add(n) {
            Some(total) if !exhausted && total <= options.max_segments => used = total,
            _ => {
                d.segments = false;
                exhausted = true;
                report.truncated = true;
            }
        }
    }
    let mut mesh = SurfaceMesh {
        origin,
        ..SurfaceMesh::default()
    };
    // The box covers what was drawn, not faces left out as degenerate or over budget.
    let mut bounds = None;
    let mut encoder = Encoder {
        origin,
        components: 3,
        limit: options.max_error,
        max_error: 0.,
    };
    let mut segments = Segments::default();
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); collected.layers.len()];
    for (k, d) in drawn.iter().enumerate() {
        if let Some(bucket) = collected
            .items
            .get(d.item)
            .and_then(|item| buckets.get_mut(item.layer as usize))
        {
            bucket.push(k);
        }
    }
    for (bucket, name) in buckets.iter().zip(&collected.layers) {
        let first_triangle = mesh.triangle_items.len();
        let first_segment = segments.items.len();
        for d in bucket.iter().filter_map(|&k| drawn.get(k)) {
            let Some(item) = collected.items.get(d.item) else {
                continue;
            };
            if !item.surface && !d.segments {
                continue;
            }
            let index = u32::try_from(mesh.items.len()).unwrap_or(u32::MAX);
            let vertices = mesh.positions.len() / 3;
            let mark = segments.mark();
            let mut encoded = Ok(());
            if item.surface {
                for p in collected
                    .rings_of(item)
                    .iter()
                    .flat_map(|r| collected.points_of(r))
                {
                    encoded = encoder.push(*p, &mut mesh.positions, mesh.normals.len() / 3);
                    if encoded.is_err() {
                        break;
                    }
                    mesh.normals.extend(d.normal);
                    mesh.colors.extend(item.color);
                }
            }
            if encoded.is_ok() && d.segments {
                // Face outlines are segments too: contrasted like wires.
                let color = match options.line_background {
                    Some([br, bg, bb]) if item.surface => {
                        let [r, g, b] = item.color;
                        let (r, g, b) = cadkit_core::svg::with_contrast((r, g, b), (br, bg, bb));
                        [r, g, b]
                    }
                    _ => item.color,
                };
                encoded = segments.push(
                    &collected,
                    collected.rings_of(item),
                    color,
                    index,
                    &mut encoder,
                );
            }
            if let Err(error) = encoded {
                // Only coordinates beyond f32 range get here without an error budget.
                if options.max_error.is_some() {
                    return Err(error);
                }
                mesh.positions.truncate(vertices * 3);
                mesh.normals.truncate(vertices * 3);
                mesh.colors.truncate(vertices * 3);
                segments.rollback(mark);
                report.degenerate += 1;
                continue;
            }
            for ring in collected.rings_of(item) {
                extend_bounds(&mut bounds, collected.points_of(ring));
            }
            collect::push_item(&mut mesh.items, &collected, item);
            if item.surface {
                let base = u32::try_from(vertices).unwrap_or(u32::MAX);
                for t in triangles.get(d.triangles.clone()).unwrap_or_default() {
                    mesh.indices.extend(t.map(|i| base.saturating_add(i)));
                    mesh.triangle_items.push(index);
                }
                report.faces += 1;
            } else {
                report.wires += 1;
            }
        }
        mesh.layers.push(range(
            name,
            first_triangle,
            mesh.triangle_items.len(),
            first_segment,
            segments.items.len(),
        ));
    }
    if let Some((lo, hi)) = bounds {
        mesh.min = relative(lo, origin);
        mesh.max = relative(hi, origin);
    }
    report.triangles = mesh.triangle_items.len();
    report.segments = segments.items.len();
    mesh.segments = segments.positions;
    mesh.segment_colors = segments.colors;
    mesh.segment_items = segments.items;
    mesh.max_error = encoder.max_error;
    mesh.report = report;
    Ok(mesh)
}

/// A layer's ranges from start and end offsets.
pub(super) fn range(name: &str, t0: usize, t1: usize, s0: usize, s1: usize) -> LayerRange {
    let u = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
    LayerRange {
        name: name.to_owned(),
        first_triangle: u(t0),
        triangle_count: u(t1.saturating_sub(t0)),
        first_segment: u(s0),
        segment_count: u(s1.saturating_sub(s0)),
    }
}

/// One face or wire to draw, in walk order.
#[derive(Debug, Clone)]
struct Drawn {
    item: usize,
    /// Triangles in the pool, empty for wires.
    triangles: Range<usize>,
    normal: [f32; 3],
    /// Its segments fit the budget.
    segments: bool,
}

/// Items in walk order with duplicate faces removed; a duplicate takes the place of the face
/// it repeats when its walk path is longer.
fn deduplicate(
    collected: &Collected,
    origin: [f64; 3],
    work: &mut u64,
    report: &mut SurfaceReport,
) -> Vec<usize> {
    let mut sequence: Vec<usize> = Vec::with_capacity(collected.items.len());
    let mut slots: HashMap<Vec<[i64; 3]>, usize> = HashMap::new();
    for (index, item) in collected.items.iter().enumerate() {
        if !item.surface {
            sequence.push(index);
            continue;
        }
        let Some(key) = face_key(collected, item, origin, work) else {
            report.truncated = true;
            break;
        };
        match slots.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(sequence.len());
                sequence.push(index);
            }
            Entry::Occupied(slot) => {
                report.duplicates += 1;
                if let Some(kept) = sequence.get_mut(*slot.get()) {
                    let deeper = collected
                        .items
                        .get(*kept)
                        .is_some_and(|other| item.path.len() > other.path.len());
                    if deeper {
                        *kept = index;
                    }
                }
            }
        }
    }
    sequence
}

/// Spends `cost` units of `work`; `None` (and an empty budget) when it cannot cover it.
fn charge(work: &mut u64, cost: usize) -> Option<()> {
    let cost = u64::try_from(cost).unwrap_or(u64::MAX);
    match work.checked_sub(cost) {
        Some(left) => {
            *work = left;
            Some(())
        }
        None => {
            *work = 0;
            None
        }
    }
}

/// What makes two faces the same: every ring as its corners in cyclic order, read from its
/// smallest corner in the direction that gives the smaller sequence (so neither the first
/// corner nor the winding matters), the outline first and the holes sorted after it, each
/// ring led by its length. Corners are rounded to a millionth of the face's own size rather
/// than the model's: a stray entity far away must not make small faces look alike. `None`
/// when the work budget runs out.
fn face_key(
    collected: &Collected,
    item: &Item,
    origin: [f64; 3],
    work: &mut u64,
) -> Option<Vec<[i64; 3]>> {
    let rings = collected.rings_of(item);
    let mut bounds = None;
    if let Some(outline) = rings.first() {
        extend_bounds(&mut bounds, collected.points_of(outline));
    }
    let size = bounds.map_or(0., |(lo, hi)| {
        (hi[0] - lo[0]).hypot(hi[1] - lo[1]).hypot(hi[2] - lo[2])
    });
    let scale = if size.is_finite() && size > 0. {
        1e6 / size
    } else {
        1.
    };
    // Saturates instead of overflowing for a degenerate size far from the origin.
    let q = |v: f64, o: f64| ((v - o) * scale).round() as i64;
    let mut keys: Vec<Vec<[i64; 3]>> = Vec::with_capacity(rings.len());
    for ring in rings {
        let points = collected.points_of(ring);
        charge(work, points.len().saturating_add(1))?;
        let mut corners: Vec<[i64; 3]> = points
            .iter()
            .map(|p| [q(p.x, origin[0]), q(p.y, origin[1]), q(p.z, origin[2])])
            .collect();
        corners.dedup();
        while corners.len() > 1 && corners.first() == corners.last() {
            corners.pop();
        }
        keys.push(canonical(&corners, work)?);
    }
    // The outline stays first; the holes may come in any order.
    keys.get_mut(1..).unwrap_or_default().sort_unstable();
    let total = keys
        .iter()
        .fold(0usize, |n, k| n.saturating_add(k.len()).saturating_add(1));
    let mut key = Vec::with_capacity(total);
    for ring in &keys {
        key.push([i64::try_from(ring.len()).unwrap_or(i64::MAX), 0, 0]);
        key.extend_from_slice(ring);
    }
    Some(key)
}

/// `ring` rotated to start at its smallest corner and read in the direction that gives the
/// smaller sequence; `None` when the work budget runs out (a ring that visits its smallest
/// corner many times costs one pass per visit).
fn canonical(ring: &[[i64; 3]], work: &mut u64) -> Option<Vec<[i64; 3]>> {
    let n = ring.len();
    let Some(&smallest) = ring.iter().min() else {
        return Some(Vec::new());
    };
    let mut best: Option<Vec<[i64; 3]>> = None;
    for start in (0..n).filter(|&i| ring.get(i) == Some(&smallest)) {
        for forward in [true, false] {
            charge(work, n)?;
            let candidate: Vec<[i64; 3]> = (0..n)
                .filter_map(|k| {
                    let i = if forward {
                        (start + k) % n
                    } else {
                        (start + n - k) % n
                    };
                    ring.get(i).copied()
                })
                .collect();
            if best.as_ref().is_none_or(|b| candidate < *b) {
                best = Some(candidate);
            }
        }
    }
    best
}

/// Triangulates the faces of `sequence` until a budget runs out.
fn triangulate_faces(
    collected: &Collected,
    sequence: &[usize],
    max_triangles: usize,
    work: &mut u64,
    report: &mut SurfaceReport,
) -> (Vec<Drawn>, Vec<[u32; 3]>) {
    let mut drawn = Vec::with_capacity(sequence.len());
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut flat: Vec<[f64; 2]> = Vec::new();
    for &index in sequence {
        let Some(item) = collected.items.get(index) else {
            continue;
        };
        if !item.surface {
            drawn.push(Drawn {
                item: index,
                triangles: 0..0,
                normal: [0.; 3],
                segments: true,
            });
            continue;
        }
        let rings = collected.rings_of(item);
        let outline = rings
            .first()
            .map(|r| collected.points_of(r))
            .unwrap_or_default();
        let Some(normal) = cadkit_core::polygon::normal(outline) else {
            report.degenerate += 1;
            continue;
        };
        // Project relative to the face's own first point: small numbers keep the
        // triangulation's orientation tests exact enough at map coordinates.
        let axis = drop_axis(normal);
        let o = outline.first().copied().unwrap_or_default();
        flat.clear();
        for ring in rings {
            flat.extend(
                collected
                    .points_of(ring)
                    .iter()
                    .map(|p| project(Point3::new(p.x - o.x, p.y - o.y, p.z - o.z), axis)),
            );
        }
        let mut slices: Vec<&[[f64; 2]]> = Vec::with_capacity(rings.len());
        let mut rest = flat.as_slice();
        for ring in rings {
            let (head, tail) = rest.split_at(ring.points.len().min(rest.len()));
            slices.push(head);
            rest = tail;
        }
        let Some(mut face) = triangulate(&slices, work) else {
            report.truncated = true;
            break;
        };
        if face.is_empty() {
            report.degenerate += 1;
            continue;
        }
        if triangles.len().saturating_add(face.len()) > max_triangles {
            report.truncated = true;
            break;
        }
        // The triangles wind counter-clockwise in the projection; dropping y mirrors the
        // remaining axes, and a normal pointing away from the dropped axis mirrors again.
        let along = match axis {
            0 => normal.x,
            1 => -normal.y,
            _ => normal.z,
        };
        if along < 0. {
            for t in &mut face {
                t.swap(1, 2);
            }
        }
        let start = triangles.len();
        triangles.extend(face);
        drawn.push(Drawn {
            item: index,
            triangles: start..triangles.len(),
            normal: [normal.x as f32, normal.y as f32, normal.z as f32],
            segments: true,
        });
    }
    (drawn, triangles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SurfaceMesh;
    use cadkit_core::{
        Block, Color, Entity, EntityKind, GroupKind, HatchEdge, HatchLoop, Layer, Vec3, Vertex,
    };

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    fn polygon(layer: &str, ring: &[Point3]) -> Entity {
        Entity::new(EntityKind::Polygon {
            exterior: ring.to_vec(),
            interiors: vec![],
        })
        .on_layer(layer)
    }

    fn group(layer: &str, children: Vec<Entity>) -> Entity {
        Entity::new(EntityKind::Group {
            group_kind: GroupKind::Other,
            name: None,
            origin: None,
            children,
        })
        .on_layer(layer)
    }

    fn vertex(mesh: &SurfaceMesh, i: u32) -> [f64; 3] {
        let i = i as usize * 3;
        [0, 1, 2].map(|k| f64::from(mesh.positions[i + k]))
    }

    fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    /// Total triangle area; also checks that every triangle winds counter-clockwise about
    /// its vertices' normal.
    fn area(mesh: &SurfaceMesh) -> f64 {
        mesh.indices
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (vertex(mesh, t[0]), vertex(mesh, t[1]), vertex(mesh, t[2]));
                let n = cross(sub(b, a), sub(c, a));
                let i = t[0] as usize * 3;
                let normal = [0, 1, 2].map(|k| f64::from(mesh.normals[i + k]));
                let along = n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2];
                assert!(along > 0., "triangle {t:?} winds against its normal");
                along / 2.
            })
            .sum()
    }

    #[test]
    fn faces_normals_layers_duplicates_and_paths() {
        let floor = [
            p(0., 0., 0.),
            p(4., 0., 0.),
            p(4., 3., 0.),
            p(0., 3., 0.),
            p(0., 0., 0.),
        ];
        let wall = [
            p(0., 0., 0.),
            p(4., 0., 0.),
            p(4., 0., 2.),
            p(0., 0., 2.),
            p(0., 0., 0.),
        ];
        let mut doc = Document::new();
        doc.layers
            .push(Layer::new("Building").with_color(Color::rgb(180, 186, 196)));
        doc.layers
            .push(Layer::new("Ground").with_color(Color::rgb(10, 20, 30)));
        doc.layers
            .push(Layer::new("Wall").with_color(Color::aci(7)));
        let mut hidden = polygon("Wall", &[p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 9.)]);
        hidden.visible = false;
        let model = doc.model_space_mut();
        model.entities.push(group(
            "Building",
            vec![
                // The solid's copy of the floor, then the boundary surface's copy: that one wins.
                polygon("Building", &floor),
                group("Ground", vec![polygon("Ground", &floor)]),
                polygon("Wall", &wall),
                hidden,
            ],
        ));
        model
            .entities
            .push(Entity::line(p(0., 0., 0.), p(0., 0., 2.)).on_layer("Wires"));
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        let report = &mesh.report;
        assert_eq!(
            (report.faces, report.duplicates, report.triangles),
            (2, 1, 4)
        );
        assert_eq!((report.wires, report.segments), (1, 9));
        assert!(!report.truncated);
        let world = |v: [f64; 3]| [0, 1, 2].map(|k| mesh.origin[k] + v[k]);
        assert_eq!(
            (world(mesh.min), world(mesh.max)),
            ([0., 0., 0.], [4., 3., 2.])
        );
        let names: Vec<&str> = mesh.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Building", "Ground", "Wall", "Wires"]);
        let tri: Vec<(u32, u32)> = mesh
            .layers
            .iter()
            .map(|l| (l.first_triangle, l.triangle_count))
            .collect();
        assert_eq!(tri, [(0, 0), (0, 2), (2, 2), (4, 0)]);
        let seg: Vec<(u32, u32)> = mesh
            .layers
            .iter()
            .map(|l| (l.first_segment, l.segment_count))
            .collect();
        assert_eq!(seg, [(0, 0), (0, 4), (4, 4), (8, 1)]);
        // Item table: the winning floor is the nested one; paths resolve in the document.
        let ground = mesh.triangle_items[0] as usize;
        assert_eq!(mesh.items.path(ground), Some(&[0, 1, 0][..]));
        let item = mesh.items.items[ground];
        assert_eq!(
            (item.kind, item.surface, item.model_path_len),
            ("polygon", true, 3)
        );
        assert_eq!(mesh.layers[item.layer as usize].name, "Ground");
        let entity = doc
            .entity_at(0, &mesh.items.path_indices(ground).unwrap())
            .unwrap();
        assert_eq!(entity.layer.as_deref(), Some("Ground"));
        let wall_item = mesh.triangle_items[2] as usize;
        assert_eq!(mesh.items.path(wall_item), Some(&[0, 2][..]));
        let wire = mesh.segment_items[8] as usize;
        assert_eq!(mesh.items.path(wire), Some(&[1][..]));
        assert!(!mesh.items.items[wire].surface);
        // Colors: the layer color; ACI 7 (black on a white sheet) becomes the neutral color.
        assert_eq!(mesh.colors[..3], [10, 20, 30]);
        let wall_vertex = mesh.indices[6] as usize * 3;
        assert_eq!(mesh.colors[wall_vertex..wall_vertex + 3], [201, 204, 210]);
        let black = surface_mesh(&doc, &SurfaceOptions::new().with_neutral_color(None)).unwrap();
        assert_eq!(black.colors[wall_vertex..wall_vertex + 3], [0, 0, 0]);
        // Normals: unit vectors along Z (floor) and Y (wall); triangles wind about them.
        assert_eq!(mesh.normals[2].abs(), 1.);
        assert_eq!(mesh.normals[wall_vertex + 1].abs(), 1.);
        assert!((area(&mesh) - 20.).abs() < 1e-9);
        // Picking: straight down through the floor, relative to the origin.
        let down = [0., 0., -1.];
        let at = |x: f64, y: f64| [x - mesh.origin[0], y - mesh.origin[1], 10. - mesh.origin[2]];
        let hit = mesh.pick(at(0.5, 0.5), down, &[]).unwrap();
        assert_eq!(
            mesh.layers[mesh.items.items[hit.item as usize].layer as usize].name,
            "Ground"
        );
        assert!((hit.distance - 10.).abs() < 1e-6);
        assert_eq!(mesh.pick(at(0.5, 0.5), down, &[1]), None);
        assert_eq!(mesh.pick(at(50., 50.), down, &[]), None);
    }

    fn unit_box() -> Vec<Vec<Point3>> {
        let c = |x: f64, y: f64, z: f64| p(x, y, z);
        vec![
            vec![c(0., 0., 0.), c(0., 1., 0.), c(1., 1., 0.), c(1., 0., 0.)],
            vec![c(0., 0., 1.), c(1., 0., 1.), c(1., 1., 1.), c(0., 1., 1.)],
            vec![c(0., 0., 0.), c(1., 0., 0.), c(1., 0., 1.), c(0., 0., 1.)],
            vec![c(1., 1., 0.), c(0., 1., 0.), c(0., 1., 1.), c(1., 1., 1.)],
            vec![c(0., 1., 0.), c(0., 0., 0.), c(0., 0., 1.), c(0., 1., 1.)],
            vec![c(1., 0., 0.), c(1., 1., 0.), c(1., 1., 1.), c(1., 0., 1.)],
        ]
    }

    #[test]
    fn closed_polylines_follow_the_option() {
        let mut doc = Document::new();
        for (k, face) in unit_box().into_iter().enumerate() {
            // Half of them repeat the closing point instead of setting the closed flag.
            let closed = k % 2 == 0;
            let mut points = face.clone();
            if !closed {
                points.push(face[0]);
            }
            doc.model_space_mut()
                .entities
                .push(Entity::polyline(points, closed));
        }
        // Not planar, and bulged: wires in every state.
        let skew = vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.5), p(0., 1., 0.)];
        doc.model_space_mut()
            .entities
            .push(Entity::polyline(skew, true));
        let mut bulged = Entity::polyline([p(5., 0., 0.), p(6., 0., 0.), p(6., 1., 0.)], true);
        if let EntityKind::Polyline { vertices, .. } = &mut bulged.kind {
            vertices[0].bulge = 0.5;
        }
        doc.model_space_mut().entities.push(bulged);
        let run = |doc: &Document, polylines| {
            surface_mesh(doc, &SurfaceOptions::new().with_polylines(polylines)).unwrap()
        };
        for (is_3d, polylines, faces) in [
            (false, PolylineSurfaces::Never, 0),
            (true, PolylineSurfaces::Never, 0),
            (false, PolylineSurfaces::Always, 6),
            (false, PolylineSurfaces::Auto, 0),
            (true, PolylineSurfaces::Auto, 6),
        ] {
            doc.model_space_mut().is_3d = is_3d;
            let mesh = run(&doc, polylines);
            let r = &mesh.report;
            assert_eq!(r.faces, faces, "{is_3d} {polylines:?}");
            assert_eq!(r.triangles, 2 * faces);
            assert_eq!(r.wires, 8 - faces);
            assert_eq!(r.flattened, 1);
            if faces == 6 {
                // f32 positions relative to an origin taken among the flattened arc's points.
                assert!((area(&mesh) - 6.).abs() < 1e-5, "{}", area(&mesh));
                // Box faces wind outwards as stored: the bottom normal points down.
                assert_eq!(mesh.normals[..3], [0., 0., -1.]);
                assert_eq!(mesh.items.items[0].kind, "polyline");
            }
        }
        // A looser planarity tolerance accepts the skew quad (0.24 off its plane at a size of
        // 1.5).
        let loose = SurfaceOptions::new()
            .with_polylines(PolylineSurfaces::Always)
            .with_planarity(0.5);
        assert_eq!(surface_mesh(&doc, &loose).unwrap().report.faces, 7);
        assert!(surface_mesh(&doc, &loose.with_planarity(-1.)).is_err());
    }

    #[test]
    fn block_references_place_faces_and_resolve_by_block_colors() {
        let mut doc = Document::new();
        doc.layers.push(Layer::new("0"));
        doc.layers
            .push(Layer::new("roof").with_color(Color::rgb(200, 0, 0)));
        let mut panel = Entity::new(EntityKind::Face {
            points: vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)],
            filled: false,
        });
        panel.color = Color::ByBlock;
        doc.blocks.push(
            Block::new("panel")
                .with_entities(vec![panel, Entity::line(p(0., 0., 0.), p(0., 0., 1.))]),
        );
        let mut insert = Entity::insert("panel", p(100., 200., 0.)).on_layer("roof");
        insert.color = Color::rgb(0, 0, 255);
        if let EntityKind::Insert {
            scale, rotation, ..
        } = &mut insert.kind
        {
            *scale = Vec3::new(2., 2., 2.);
            *rotation = std::f64::consts::FRAC_PI_2;
        }
        let nested = group("roof", vec![insert.clone()]);
        let model = doc.model_space_mut();
        model
            .entities
            .push(Entity::text(p(0., 0., 0.), 1., "label"));
        model.entities.push(insert);
        model.entities.push(nested);
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        let r = &mesh.report;
        assert_eq!((r.faces, r.wires, r.duplicates), (1, 2, 1));
        assert_eq!(r.skipped.get("text"), Some(&1));
        // The nested reference repeats the same corners and wins.
        let face = mesh.triangle_items[0] as usize;
        assert_eq!(mesh.items.path(face), Some(&[2, 0, 0][..]));
        assert_eq!(mesh.items.items[face].model_path_len, 2);
        let entity = doc
            .entity_at(0, &mesh.items.path_indices(face).unwrap())
            .unwrap();
        assert!(matches!(entity.kind, EntityKind::Face { .. }));
        // ByBlock takes the reference's color; layer 0 takes the reference's layer.
        assert_eq!(mesh.colors[..3], [0, 0, 255]);
        assert_eq!(
            mesh.layers[mesh.items.items[face].layer as usize].name,
            "roof"
        );
        // Wires are not deduplicated: the line is drawn once per reference.
        let wire = mesh.segment_items[4] as usize;
        assert_eq!(mesh.items.path(wire), Some(&[1, 1][..]));
        assert_eq!(mesh.items.items[wire].model_path_len, 1);
        let nested_wire = mesh.segment_items[5] as usize;
        assert_eq!(mesh.items.path(nested_wire), Some(&[2, 0, 1][..]));
        assert_eq!(
            mesh.segment_colors[mesh.segment_colors.len() - 3..],
            [200, 0, 0]
        );
        // Placement: scaled by 2, turned a quarter, moved to (100, 200).
        let world = |i: u32| {
            let v = vertex(&mesh, i);
            [0, 1, 2].map(|k| v[k] + mesh.origin[k])
        };
        let corners: Vec<[f64; 3]> = (0..4).map(world).collect();
        assert_eq!(
            corners,
            [
                [100., 200., 0.],
                [100., 202., 0.],
                [98., 202., 0.],
                [98., 200., 0.]
            ]
        );
        assert!((area(&mesh) - 4.).abs() < 1e-9);
    }

    #[test]
    fn map_coordinates_keep_their_precision() {
        let mut doc = Document::new();
        let (x0, y0, z0) = (500_000.123, 4_000_000.456, 50.789);
        for i in 0..100 {
            for j in 0..10 {
                // Irregular steps, so the offsets are not round numbers in f32.
                let (x, y) = (
                    x0 + f64::from(i) * 9.876_543,
                    y0 + f64::from(j) * 10.123_457,
                );
                let ring = [
                    p(x, y, z0),
                    p(x + 9.5, y, z0),
                    p(x + 9.5, y + 9.5, z0 + 1.),
                    p(x, y + 9.5, z0 + 1.),
                ];
                doc.model_space_mut()
                    .entities
                    .push(Entity::new(EntityKind::Face {
                        points: ring.to_vec(),
                        filled: true,
                    }));
            }
        }
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        assert_eq!(mesh.report.faces, 1000);
        // Up to 500 m from the origin: f32 steps are 2^-15 m (31 µm) there, so the error
        // stays below half of that.
        assert!(
            mesh.max_error > 0. && mesh.max_error < 1.6e-5,
            "{}",
            mesh.max_error
        );
        let mut k = 0;
        for e in &doc.models[0].entities {
            let EntityKind::Face { points, .. } = &e.kind else {
                unreachable!()
            };
            for q in points {
                let v = vertex(&mesh, k);
                for (axis, value) in [q.x, q.y, q.z].into_iter().enumerate() {
                    assert!((mesh.origin[axis] + v[axis] - value).abs() <= mesh.max_error);
                }
                k += 1;
            }
        }
        let strict = SurfaceOptions::new().with_max_error(Some(1e-9));
        assert!(matches!(
            surface_mesh(&doc, &strict),
            Err(SceneError::Precision { .. })
        ));
        let fine = SurfaceOptions::new().with_max_error(Some(1.6e-5));
        assert!(surface_mesh(&doc, &fine).is_ok());
    }

    #[test]
    fn concave_faces_with_holes_in_any_plane() {
        // A gable wall in the XZ plane at y = 5 with a window, wound both ways.
        let gable = [
            p(0., 5., 0.),
            p(8., 5., 0.),
            p(8., 5., 6.),
            p(4., 5., 9.),
            p(0., 5., 6.),
            p(0., 5., 0.),
        ];
        let window = vec![
            p(2., 5., 2.),
            p(4., 5., 2.),
            p(4., 5., 4.),
            p(2., 5., 4.),
            p(2., 5., 2.),
        ];
        let mut reversed = gable.to_vec();
        reversed.reverse();
        // An L in a tilted plane.
        let tilt = |x: f64, y: f64| p(x + 20., y, x * 0.5);
        let ell = [
            tilt(0., 0.),
            tilt(3., 0.),
            tilt(3., 1.),
            tilt(1., 1.),
            tilt(1., 3.),
            tilt(0., 3.),
        ];
        let mut doc = Document::new();
        for exterior in [gable.to_vec(), reversed] {
            doc.model_space_mut()
                .entities
                .push(Entity::new(EntityKind::Polygon {
                    exterior,
                    interiors: vec![window.clone()],
                }));
        }
        doc.model_space_mut().entities.push(polygon("0", &ell));
        let mut options = SurfaceOptions::new();
        let mesh = surface_mesh(&doc, &options).unwrap();
        // The reversed wall has the same corners: one is a duplicate.
        assert_eq!((mesh.report.faces, mesh.report.duplicates), (2, 1));
        let tilted_area = 5. * 1.25f64.sqrt();
        assert!(
            (area(&mesh) - (56. + tilted_area)).abs() < 1e-5,
            "{}",
            area(&mesh)
        );
        // Outline segments include the hole.
        assert_eq!(mesh.report.segments, 5 + 4 + 6);
        options.outlines = false;
        assert_eq!(surface_mesh(&doc, &options).unwrap().report.segments, 0);
    }

    #[test]
    fn solid_hatches_with_straight_edges_become_faces() {
        let square = |a: f64, b: f64| HatchLoop {
            edges: vec![
                HatchEdge::Line {
                    start: p(a, a, 0.),
                    end: p(b, a, 0.),
                },
                HatchEdge::Line {
                    start: p(b, a, 0.),
                    end: p(b, b, 0.),
                },
                HatchEdge::Line {
                    start: p(b, b, 0.),
                    end: p(a, b, 0.),
                },
                HatchEdge::Line {
                    start: p(a, b, 0.),
                    end: p(a, a, 0.),
                },
            ],
            external: false,
        };
        let polyline_loop = HatchLoop {
            edges: vec![HatchEdge::Polyline {
                vertices: [p(20., 0., 0.), p(21., 0., 0.), p(21., 1., 0.)]
                    .map(Vertex::at)
                    .to_vec(),
                closed: true,
            }],
            external: true,
        };
        let hatch = |loops: Vec<HatchLoop>, solid: bool| {
            Entity::new(EntityKind::Hatch {
                loops,
                solid,
                pattern: None,
                pattern_scale: 1.,
                pattern_angle: 0.,
                normal: Vec3::Z,
            })
        };
        let arc = HatchLoop {
            edges: vec![HatchEdge::Arc {
                center: p(0., 0., 0.),
                radius: 1.,
                start_angle: 0.,
                end_angle: std::f64::consts::TAU,
                ccw: true,
            }],
            external: true,
        };
        let mut doc = Document::new();
        let model = doc.model_space_mut();
        // Outline, hole, island in the hole: two faces of 96 and 1.
        model.entities.push(hatch(
            vec![square(0., 10.), square(2., 4.), square(2.5, 3.5)],
            true,
        ));
        model.entities.push(hatch(vec![polyline_loop], true));
        model.entities.push(hatch(vec![square(0., 1.), arc], true));
        model.entities.push(hatch(vec![square(0., 1.)], false));
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        assert_eq!(mesh.report.faces, 3);
        assert_eq!(mesh.report.skipped.get("hatch"), Some(&2));
        assert!((area(&mesh) - 97.5).abs() < 1e-9);
        let paths: Vec<_> = (0..mesh.items.len())
            .map(|i| mesh.items.path(i).unwrap().to_vec())
            .collect();
        assert_eq!(paths, [vec![0], vec![0], vec![1]]);
    }

    #[test]
    fn budgets_truncate_in_walk_order() {
        let mut doc = Document::new();
        for i in 0..10 {
            let x = f64::from(i) * 2.;
            let quad = [
                p(x, 0., 0.),
                p(x + 1., 0., 0.),
                p(x + 1., 1., 0.),
                p(x, 1., 0.),
            ];
            doc.model_space_mut()
                .entities
                .push(polygon(if i % 2 == 0 { "a" } else { "b" }, &quad));
        }
        let full = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        assert_eq!(
            (
                full.report.faces,
                full.report.triangles,
                full.report.segments
            ),
            (10, 20, 40)
        );
        assert!(!full.report.truncated);
        let few = surface_mesh(&doc, &SurfaceOptions::new().with_max_triangles(5)).unwrap();
        assert_eq!((few.report.faces, few.report.triangles), (2, 4));
        assert!(few.report.truncated);
        // The first two faces in walk order, one per layer.
        let paths: Vec<_> = (0..few.items.len())
            .map(|i| few.items.path(i).unwrap()[0])
            .collect();
        assert_eq!(paths, [0, 1]);
        let lines = surface_mesh(&doc, &SurfaceOptions::new().with_max_segments(10)).unwrap();
        assert_eq!((lines.report.triangles, lines.report.segments), (20, 8));
        assert!(lines.report.truncated);
        let work = surface_mesh(&doc, &SurfaceOptions::new().with_max_work(30)).unwrap();
        assert!(work.report.truncated && work.report.faces < 10);
        let walk = SurfaceOptions::new().with_walk(WalkOptions::new().with_max_work(12));
        let walked = surface_mesh(&doc, &walk).unwrap();
        assert!(walked.report.truncated && walked.report.faces < 10);
        assert_eq!(
            surface_mesh(&doc, &SurfaceOptions::new().with_max_triangles(0))
                .unwrap()
                .report
                .faces,
            0
        );
        for bad in [
            SurfaceOptions::new().with_max_triangles(MAX_ELEMENTS + 1),
            SurfaceOptions::new().with_max_segments(usize::MAX),
            SurfaceOptions::new().with_tolerance(0.),
            SurfaceOptions::new().with_max_error(Some(f64::NAN)),
        ] {
            assert!(matches!(
                surface_mesh(&doc, &bad),
                Err(SceneError::InvalidBudget { .. })
            ));
        }
    }

    #[test]
    fn hostile_documents_do_not_panic() {
        let mut doc = Document::new();
        let model = doc.model_space_mut();
        model.entities.push(polygon(
            "0",
            &[p(f64::NAN, 0., 0.), p(1., 0., 0.), p(1., 1., 0.)],
        ));
        model
            .entities
            .push(polygon("0", &[p(0., 0., 0.), p(1., 1., 1.), p(2., 2., 2.)]));
        model.entities.push(polygon(
            "0",
            &[p(1e308, 0., 0.), p(-1e308, 0., 0.), p(0., 1e308, 0.)],
        ));
        model.entities.push(Entity::new(EntityKind::Polygon {
            exterior: vec![],
            interiors: vec![vec![]],
        }));
        model.entities.push(Entity::new(EntityKind::Mesh {
            vertices: vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.)],
            faces: vec![vec![0, 1, 2], vec![0, 1, 99], vec![], vec![0, 0, 0]],
        }));
        model
            .entities
            .push(Entity::circle(p(0., 0., 0.), f64::INFINITY));
        model
            .entities
            .push(Entity::line(p(1., 1., 1.), p(1., 1., 1.)));
        let options = SurfaceOptions::new().with_tolerance(1e-300);
        let mesh = surface_mesh(&doc, &options).unwrap();
        assert_eq!(mesh.report.faces, 1);
        assert!(mesh.report.degenerate >= 6, "{:?}", mesh.report);
        assert_eq!(mesh.indices.len(), 3);
        assert!(mesh.positions.iter().all(|v| v.is_finite()));
        assert!(
            mesh.indices
                .iter()
                .all(|&i| (i as usize) < mesh.positions.len() / 3)
        );
        let missing = surface_mesh(&doc, &SurfaceOptions::new().with_model(9)).unwrap();
        assert_eq!(missing, SurfaceMesh::default());
        let empty = surface_mesh(&Document::new(), &SurfaceOptions::new()).unwrap();
        assert!(empty.positions.is_empty() && empty.layers.is_empty());
    }

    /// World coordinates of mesh vertex i.
    fn world(mesh: &SurfaceMesh, i: u32) -> [f64; 3] {
        let v = vertex(mesh, i);
        [0, 1, 2].map(|k| mesh.origin[k] + v[k])
    }

    #[test]
    fn faces_that_share_corners_are_not_duplicates() {
        let outer = vec![
            p(0., 0., 0.),
            p(10., 0., 0.),
            p(10., 10., 0.),
            p(0., 10., 0.),
        ];
        let hole = vec![p(2., 2., 0.), p(4., 2., 0.), p(4., 4., 0.), p(2., 4., 0.)];
        // Two concave quads through the same four corners (the fourth inside the triangle of
        // the other three), joined differently: different faces of area 5 each.
        let (a, b, c, d) = (
            p(20., 0., 0.),
            p(24., 0., 0.),
            p(22., 4., 0.),
            p(22., 1., 0.),
        );
        let mut doc = Document::new();
        let model = doc.model_space_mut();
        model.entities.push(polygon("0", &outer));
        model.entities.push(Entity::new(EntityKind::Polygon {
            exterior: outer.clone(),
            interiors: vec![hole],
        }));
        model.entities.push(polygon("0", &[a, b, c, d]));
        model.entities.push(polygon("0", &[a, b, d, c]));
        // Two triangles of one quad share an edge but not all corners.
        model.entities.push(polygon(
            "0",
            &[p(30., 0., 0.), p(31., 0., 0.), p(31., 1., 0.)],
        ));
        model.entities.push(polygon(
            "0",
            &[p(30., 0., 0.), p(31., 1., 0.), p(30., 1., 0.)],
        ));
        // The same face again, starting elsewhere and wound the other way: a duplicate.
        model.entities.push(polygon("0", &[d, c, b, a]));
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        assert_eq!((mesh.report.faces, mesh.report.duplicates), (6, 1));
        assert!(
            (area(&mesh) - (100. + 96. + 5. + 5. + 1.)).abs() < 1e-9,
            "{}",
            area(&mesh)
        );
    }

    #[test]
    fn a_stray_entity_at_zero_keeps_the_origin_extents_and_errors_right() {
        let (x0, y0, z0) = (500_000.123, 4_000_000.456, 50.789);
        let mut doc = Document::new();
        let model = doc.model_space_mut();
        // The usual leftover at 0,0 in a drawing at map coordinates.
        model
            .entities
            .push(Entity::line(p(0., 0., 0.), p(1., 0., 0.)));
        for i in 0..20 {
            let x = x0 + f64::from(i) * 3.1;
            model.entities.push(polygon(
                "0",
                &[p(x, y0, z0), p(x + 1., y0, z0), p(x + 1., y0 + 1., z0 + 2.)],
            ));
        }
        // Two different small triangles a few decimetres apart: not duplicates, although
        // they are tiny against the extents of the model.
        model.entities.push(polygon(
            "0",
            &[
                p(x0, y0 + 5., z0),
                p(x0 + 1., y0 + 5., z0),
                p(x0, y0 + 6., z0),
            ],
        ));
        model.entities.push(polygon(
            "0",
            &[
                p(x0, y0 + 5., z0),
                p(x0 + 1., y0 + 5., z0),
                p(x0 + 0.3, y0 + 6., z0),
            ],
        ));
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        assert_eq!((mesh.report.faces, mesh.report.duplicates), (22, 0));
        // The origin sits among the faces, not between them and the stray line.
        assert!((mesh.origin[0] - x0).abs() < 100. && (mesh.origin[1] - y0).abs() < 10.);
        let lo = [0, 1, 2].map(|k| mesh.origin[k] + mesh.min[k]);
        let hi = [0, 1, 2].map(|k| mesh.origin[k] + mesh.max[k]);
        for (got, want) in lo.into_iter().zip([0., 0., 0.]).chain(hi.into_iter().zip([
            x0 + 19. * 3.1 + 1.,
            y0 + 6.,
            z0 + 2.,
        ])) {
            assert!((got - want).abs() < 1e-6, "{lo:?} {hi:?}");
        }
        // The stray line is 4,000 km from the origin: its f32 offsets are a quarter metre
        // apart, and max_error says so.
        assert!(
            mesh.max_error > 0.01 && mesh.max_error <= 0.25,
            "{}",
            mesh.max_error
        );
        // The stray line comes first in walk order, and so first on its layer.
        let wire = mesh.layers[0].first_segment as usize;
        assert_eq!(
            mesh.items.path(mesh.segment_items[wire] as usize),
            Some(&[0][..])
        );
        for (k, want) in [0., 0., 0., 1., 0., 0.].into_iter().enumerate() {
            let axis = k % 3;
            let got = mesh.origin[axis] + f64::from(mesh.segments[wire * 6 + k]);
            assert!((got - want).abs() <= mesh.max_error, "{got} {want}");
        }
        // Every face vertex round-trips within the stated error, which is far smaller there.
        let mut source = Vec::new();
        for e in &doc.models[0].entities {
            if let EntityKind::Polygon { exterior, .. } = &e.kind {
                source.extend(exterior.iter().copied());
            }
        }
        assert_eq!(source.len(), mesh.positions.len() / 3);
        for (i, q) in source.iter().enumerate() {
            let w = world(&mesh, i as u32);
            for (got, want) in w.into_iter().zip([q.x, q.y, q.z]) {
                assert!((got - want).abs() <= mesh.max_error);
                assert!((got - want).abs() < 1e-4);
            }
        }
        // The line buffer, flat and in 3D, uses the same origin rule.
        for three_d in [false, true] {
            let options = super::super::LineOptions::new().with_three_d(three_d);
            let lines = super::super::line_buffer(&doc, &options).unwrap();
            let n = usize::from(lines.components);
            assert_eq!(lines.origin[..2], mesh.origin[..2]);
            assert_eq!(lines.origin[2], if three_d { mesh.origin[2] } else { 0. });
            assert!(lines.max_error > 0.01 && lines.max_error <= 0.25);
            let first = lines.layers[0].first_segment as usize;
            assert_eq!(
                lines.items.path(lines.segment_items[first] as usize),
                Some(&[0][..])
            );
            for k in 0..2 * n {
                let want = if k == n { 1. } else { 0. };
                let got = lines.origin[k % n] + f64::from(lines.segments[first * 2 * n + k]);
                assert!((got - want).abs() <= lines.max_error, "{got} {want}");
            }
            let lo = [0, 1, 2].map(|k| lines.origin[k] + lines.min[k]);
            assert!(lo.iter().all(|v| v.abs() < 1e-6), "{lo:?}");
            assert_eq!(
                lines.max[2],
                if three_d { hi[2] - lines.origin[2] } else { 0. }
            );
        }
    }

    #[test]
    fn extents_cover_what_was_drawn() {
        let mut doc = Document::new();
        let model = doc.model_space_mut();
        model
            .entities
            .push(polygon("0", &[p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.)]));
        // A collinear "face" far away is not drawn and must not stretch the box.
        model.entities.push(polygon(
            "0",
            &[p(1e5, 1e5, 0.), p(2e5, 2e5, 0.), p(3e5, 3e5, 0.)],
        ));
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        assert_eq!((mesh.report.faces, mesh.report.degenerate), (1, 1));
        let hi = [0, 1, 2].map(|k| mesh.origin[k] + mesh.max[k]);
        assert_eq!(hi, [1., 1., 0.]);
        // A face left out by the triangle budget is not in the box either.
        let mut doc = Document::new();
        let model = doc.model_space_mut();
        model
            .entities
            .push(polygon("0", &[p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.)]));
        model.entities.push(polygon(
            "0",
            &[p(9., 0., 0.), p(10., 0., 0.), p(10., 1., 0.)],
        ));
        let mesh = surface_mesh(&doc, &SurfaceOptions::new().with_max_triangles(1)).unwrap();
        assert!(mesh.report.truncated);
        let hi = [0, 1, 2].map(|k| mesh.origin[k] + mesh.max[k]);
        assert_eq!(hi, [1., 1., 0.]);
        // Nothing drawn: an empty box at the origin.
        let none = surface_mesh(&doc, &SurfaceOptions::new().with_max_triangles(0)).unwrap();
        assert_eq!((none.min, none.max), ([0.; 3], [0.; 3]));
    }

    #[test]
    fn mirrored_blocks_keep_triangles_wound_about_their_normals() {
        let mut doc = Document::new();
        doc.model_space_mut().is_3d = true;
        let square = vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)];
        let ell = vec![
            p(0., 0., 1.),
            p(3., 0., 1.),
            p(3., 1., 1.),
            p(1., 1., 1.),
            p(1., 3., 1.),
            p(0., 3., 1.),
        ];
        doc.blocks.push(Block::new("tile").with_entities(vec![
            Entity::new(EntityKind::Face {
                points: square,
                filled: true,
            }),
            Entity::polyline(ell, true),
        ]));
        for (k, scale) in [
            Vec3::new(1., 1., 1.),
            Vec3::new(-1., 1., 1.),
            Vec3::new(1., -2., 1.),
            Vec3::new(1., 1., -1.),
            Vec3::new(-1., -1., -1.),
        ]
        .into_iter()
        .enumerate()
        {
            let mut insert = Entity::insert("tile", p(10. * k as f64, 0., 0.));
            if let EntityKind::Insert { scale: s, .. } = &mut insert.kind {
                *s = scale;
            }
            doc.model_space_mut().entities.push(insert);
        }
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        assert_eq!(mesh.report.faces, 10);
        // area() checks each triangle against its normal; |det| scales the areas.
        let expected = (1. + 5.) * (1. + 1. + 2. + 1. + 1.);
        assert!((area(&mesh) - expected).abs() < 1e-9, "{}", area(&mesh));
        // A mirror that reverses the winding seen along the face's normal flips the normal: x
        // or y mirrored (an odd number of them), but not z, which keeps the plane's winding.
        let z: Vec<f32> = (0..mesh.items.len())
            .map(|item| {
                let t = mesh
                    .triangle_items
                    .iter()
                    .position(|&i| i as usize == item)
                    .unwrap();
                mesh.normals[mesh.indices[3 * t] as usize * 3 + 2]
            })
            .collect();
        assert_eq!(z, [1., 1., -1., -1., -1., -1., 1., 1., 1., 1.]);
    }

    #[test]
    fn colors_through_nested_blocks_match_the_svg_export() {
        let mut doc = Document::new();
        doc.layers.push(Layer::new("0"));
        doc.layers
            .push(Layer::new("roof").with_color(Color::rgb(200, 0, 0)));
        doc.layers
            .push(Layer::new("blue").with_color(Color::rgb(0, 0, 250)));
        let line = |y: f64, color: Color, layer: Option<&str>| {
            let mut e = Entity::line(p(0., y, 0.), p(1., y, 0.));
            e.color = color;
            e.layer = layer.map(str::to_owned);
            e
        };
        let mut face = Entity::new(EntityKind::Face {
            points: vec![p(0., 5., 0.), p(1., 5., 0.), p(1., 6., 0.)],
            filled: true,
        });
        face.color = Color::ByBlock;
        doc.blocks.push(Block::new("inner").with_entities(vec![
            line(0., Color::ByBlock, Some("0")),
            line(1., Color::ByLayer, Some("0")),
            line(2., Color::ByLayer, Some("blue")),
            line(3., Color::ByLayer, None),
            face,
        ]));
        let mut nested = Entity::insert("inner", p(0., 0., 0.));
        nested.color = Color::ByBlock;
        doc.blocks.push(
            Block::new("outer").with_entities(vec![nested, line(4., Color::ByBlock, Some("0"))]),
        );
        let mut green = Entity::insert("outer", p(0., 0., 0.)).on_layer("roof");
        green.color = Color::rgb(0, 128, 0);
        let mut by_layer = Entity::insert("outer", p(10., 0., 0.)).on_layer("roof");
        by_layer.color = Color::ByLayer;
        let mut on_zero = Entity::insert("inner", p(20., 0., 0.));
        on_zero.color = Color::ByLayer;
        let model = doc.model_space_mut();
        model.entities.extend([green, by_layer, on_zero]);

        let lines = super::super::line_buffer(&doc, &super::super::LineOptions::new()).unwrap();
        let mut ours = std::collections::BTreeSet::new();
        for s in 0..lines.segment_items.len() {
            let item = lines.segment_items[s] as usize;
            let path = lines.items.path(item).unwrap();
            let model_path = &path[..lines.items.items[item].model_path_len as usize];
            let rgb = &lines.segment_colors[s * 6..s * 6 + 3];
            ours.insert((
                format!("{}", model_path[0]),
                format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]),
            ));
        }
        let mut options = cadkit_core::svg::SvgOptions::new();
        options.entity_refs = true;
        let svg = cadkit_core::svg::render(&doc, &options);
        let mut theirs = std::collections::BTreeSet::new();
        for element in svg.split('<').skip(1) {
            let attr = |name: &str| {
                let key = format!("{name}=\"");
                let start = element.find(&key)? + key.len();
                Some(element[start..].split('"').next()?.to_owned())
            };
            if let (Some(e), Some(stroke)) = (attr("data-e"), attr("stroke")) {
                theirs.insert((e, stroke));
            }
        }
        assert_eq!(ours, theirs, "{svg}");
        // And the colors themselves: the green reference, its ByLayer twin on red "roof",
        // and the reference on layer 0 whose ByBlock children come out black.
        let expect = [
            ("0", "#008000"), // ByBlock, nested ByBlock
            ("0", "#c80000"), // ByLayer on layer 0 inherits "roof"
            ("0", "#0000fa"), // ByLayer on "blue"
            ("1", "#c80000"),
            ("1", "#0000fa"),
            ("2", "#000000"),
            ("2", "#0000fa"),
        ];
        let expect: std::collections::BTreeSet<_> = expect
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        assert_eq!(ours, expect);
        // Faces resolve the same way (without the neutral color for black).
        let mesh = surface_mesh(&doc, &SurfaceOptions::new().with_neutral_color(None)).unwrap();
        let mut face_colors: Vec<(u32, [u8; 3])> = (0..mesh.items.len())
            .filter(|&i| mesh.items.items[i].surface)
            .map(|i| {
                let t = mesh
                    .triangle_items
                    .iter()
                    .position(|&k| k as usize == i)
                    .unwrap();
                let v = mesh.indices[3 * t] as usize * 3;
                (
                    mesh.items.path(i).unwrap()[0],
                    [mesh.colors[v], mesh.colors[v + 1], mesh.colors[v + 2]],
                )
            })
            .collect();
        face_colors.sort_unstable();
        assert_eq!(
            face_colors,
            [(0, [0, 128, 0]), (1, [200, 0, 0]), (2, [0, 0, 0])]
        );
    }

    #[test]
    fn face_outlines_are_contrasted_like_other_segments() {
        let mut doc = Document::new();
        let mut white = polygon("0", &[p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.)]);
        white.color = Color::rgb(255, 255, 255);
        doc.model_space_mut().entities.push(white);
        let options = SurfaceOptions::new().with_line_background(Some([255; 3]));
        let mesh = surface_mesh(&doc, &options).unwrap();
        // The face keeps its color; its outline on a white background turns black.
        assert_eq!(mesh.colors[..3], [255, 255, 255]);
        assert_eq!(mesh.segment_colors[..6], [0, 0, 0, 0, 0, 0]);
        let plain = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        assert_eq!(plain.segment_colors[..3], [255, 255, 255]);
    }

    #[test]
    fn a_huge_insert_array_stops_at_the_budgets() {
        let mut doc = Document::new();
        doc.blocks.push(
            Block::new("cell").with_entities(vec![Entity::new(EntityKind::Face {
                points: vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)],
                filled: true,
            })]),
        );
        let mut array = Entity::insert("cell", p(0., 0., 0.));
        if let EntityKind::Insert {
            columns,
            rows,
            column_spacing,
            row_spacing,
            ..
        } = &mut array.kind
        {
            (*columns, *rows, *column_spacing, *row_spacing) = (1000, 1000, 2., 2.);
        }
        doc.model_space_mut().entities.push(array);
        let options = SurfaceOptions::new()
            .with_max_triangles(1000)
            .with_max_segments(1000);
        let start = std::time::Instant::now();
        let mesh = surface_mesh(&doc, &options).unwrap();
        assert!(mesh.report.truncated);
        assert!(mesh.report.triangles <= 1000 && mesh.report.segments <= 1000);
        assert!(mesh.report.faces >= 250, "{:?}", mesh.report);
        // Every cell has the same path; none of them is a duplicate of another.
        assert_eq!(mesh.report.duplicates, 0);
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
        let lines = super::super::line_buffer(
            &doc,
            &super::super::LineOptions::new().with_max_segments(1000),
        )
        .unwrap();
        assert!(lines.report.truncated && lines.report.segments <= 1000);
    }

    /// Peak resident memory of the test process (Linux), for the measurement below.
    fn peak_memory() -> String {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("VmHWM"))
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    }

    /// Timing and buffer sizes of 100,000 closed quads at map coordinates. Run with
    /// `cargo test -p cadkit-scene --features document --profile release -- --ignored --nocapture`
    /// (a debug build is several times slower).
    #[test]
    #[ignore = "measurement"]
    fn measure_hundred_thousand_quads() {
        let mut doc = Document::new();
        doc.model_space_mut().is_3d = true;
        for i in 0..100_000u32 {
            let (x, y) = (
                500_000.123 + f64::from(i % 400) * 3.217,
                5_600_000.456 + f64::from(i / 400) * 3.173,
            );
            let z = f64::from(i % 7);
            let ring = vec![
                p(x, y, z),
                p(x + 2., y, z),
                p(x + 2., y + 2., z + 0.5),
                p(x, y + 2., z + 0.5),
            ];
            doc.model_space_mut()
                .entities
                .push(Entity::polyline(ring, true).on_layer(if i % 2 == 0 { "a" } else { "b" }));
        }
        let start = std::time::Instant::now();
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        let elapsed = start.elapsed();
        let bytes = mesh.positions.len() * 4
            + mesh.normals.len() * 4
            + mesh.colors.len()
            + mesh.indices.len() * 4
            + mesh.triangle_items.len() * 4
            + mesh.segments.len() * 4
            + mesh.segment_colors.len()
            + mesh.segment_items.len() * 4
            + mesh.items.items.len() * std::mem::size_of::<crate::SceneItem>()
            + mesh.items.paths.len() * 4;
        println!(
            "100k quads: {elapsed:?}, {} faces, {} triangles, {} segments, {} vertices, {:.1} MB, max error {:e}",
            mesh.report.faces,
            mesh.report.triangles,
            mesh.report.segments,
            mesh.positions.len() / 3,
            bytes as f64 / 1e6,
            mesh.max_error,
        );
        assert_eq!(mesh.report.faces, 100_000);
        assert_eq!(mesh.report.triangles, 200_000);
        let start = std::time::Instant::now();
        let lines = super::super::line_buffer(&doc, &super::super::LineOptions::new()).unwrap();
        println!(
            "100k quads as 2D lines: {:?}, {} segments, {:.1} MB",
            start.elapsed(),
            lines.report.segments,
            (lines.segments.len() * 4 + lines.segment_colors.len() + lines.segment_items.len() * 4)
                as f64
                / 1e6,
        );
        println!("peak memory so far: {}", peak_memory());
        // A hostile insert array: 1000 x 1000 references to a block with one face, with the
        // default budgets.
        let mut doc = Document::new();
        doc.blocks.push(
            Block::new("cell").with_entities(vec![Entity::new(EntityKind::Face {
                points: vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)],
                filled: true,
            })]),
        );
        let mut array = Entity::insert("cell", p(500_000., 5_600_000., 0.));
        if let EntityKind::Insert {
            columns,
            rows,
            column_spacing,
            row_spacing,
            ..
        } = &mut array.kind
        {
            (*columns, *rows, *column_spacing, *row_spacing) = (1000, 1000, 2., 2.);
        }
        doc.model_space_mut().entities.push(array);
        let start = std::time::Instant::now();
        let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
        println!(
            "1000 x 1000 array: {:?}, {} faces, {} triangles, {} segments, truncated {}, peak memory {}",
            start.elapsed(),
            mesh.report.faces,
            mesh.report.triangles,
            mesh.report.segments,
            mesh.report.truncated,
            peak_memory(),
        );
    }
}
