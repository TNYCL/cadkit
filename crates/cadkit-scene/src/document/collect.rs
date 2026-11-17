//! One walk of a model into f64 rings, shared by [`super::surface_mesh`] and
//! [`super::line_buffer`], and the f32 encoding of what it collected.
//!
//! Points are stored placed (block transforms applied) in drawing coordinates, cleaned of
//! repeated consecutive points and of a repeated closing point. Faces keep their outer ring
//! first and their holes after it; wires keep one ring per polyline with its own closed flag.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use cadkit_core::ops::{WalkContext, WalkOptions};
use cadkit_core::{Document, Entity, EntityKind, HatchEdge, HatchLoop, Point3, Transform, Vec3};

use crate::SceneError;
use crate::buffers::{SceneItem, SceneItems};
use crate::mesh::encode_relative;

/// A run of points in [`Collected::points`].
#[derive(Debug, Clone)]
pub(super) struct Ring {
    pub points: Range<usize>,
    /// The ring returns to its first point (always true for faces).
    pub closed: bool,
}

impl Ring {
    /// Segments drawn for the ring.
    pub fn segments(&self) -> usize {
        let n = self.points.len();
        if self.closed && n > 2 {
            n
        } else {
            n.saturating_sub(1)
        }
    }
}

/// One face or the wire geometry of one entity.
#[derive(Debug, Clone)]
pub(super) struct Item {
    /// Rings in [`Collected::rings`]; for a face the first is the outline.
    pub rings: Range<usize>,
    pub surface: bool,
    pub layer: u32,
    pub color: [u8; 3],
    /// Walk path in [`Collected::paths`].
    pub path: Range<usize>,
    pub model_path_len: u32,
    pub kind: &'static str,
}

/// Everything one walk collected.
#[derive(Debug, Default)]
pub(super) struct Collected {
    pub points: Vec<Point3>,
    pub rings: Vec<Ring>,
    pub items: Vec<Item>,
    pub paths: Vec<u32>,
    /// Layer table: the document's layers, then layers only entities name, in walk order.
    pub layers: Vec<String>,
    pub wires: usize,
    pub flattened: usize,
    pub degenerate: usize,
    pub skipped: BTreeMap<&'static str, usize>,
    pub truncated: bool,
}

impl Collected {
    pub fn rings_of(&self, item: &Item) -> &[Ring] {
        self.rings.get(item.rings.clone()).unwrap_or_default()
    }

    pub fn points_of(&self, ring: &Ring) -> &[Point3] {
        self.points.get(ring.points.clone()).unwrap_or_default()
    }

    /// Per-axis median of all points: the buffers' origin. Unlike the center of the box, a
    /// few stray entities far from the drawing (a common leftover at 0,0 in drawings at map
    /// coordinates) do not pull it away from the bulk of the geometry.
    pub fn median(&self) -> Option<[f64; 3]> {
        if self.points.is_empty() {
            return None;
        }
        let mut values: Vec<f64> = Vec::with_capacity(self.points.len());
        let mut out = [0.; 3];
        for (axis, slot) in out.iter_mut().enumerate() {
            values.clear();
            values.extend(self.points.iter().map(|p| match axis {
                0 => p.x,
                1 => p.y,
                _ => p.z,
            }));
            let middle = values.len() / 2;
            let (_, value, _) = values.select_nth_unstable_by(middle, f64::total_cmp);
            *slot = *value;
        }
        Some(out)
    }
}

/// Grows the box `bounds` (lowest and highest corner, `None` while empty) by `points`.
pub(super) fn extend_bounds(bounds: &mut Option<([f64; 3], [f64; 3])>, points: &[Point3]) {
    for p in points {
        let v = [p.x, p.y, p.z];
        let (lo, hi) = bounds.get_or_insert((v, v));
        for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(v) {
            *l = l.min(v);
            *h = h.max(v);
        }
    }
}

/// What to collect.
#[derive(Debug, Clone, Copy)]
pub(super) struct Settings {
    pub model: usize,
    pub walk: WalkOptions,
    /// Collect polygons, faces, mesh faces and solid hatches as faces; otherwise as wires.
    pub faces: bool,
    /// Closed, bulge-free, planar polylines become faces (only with `faces`).
    pub polyline_faces: bool,
    /// Largest distance of a polyline face's point from its plane, relative to the size of
    /// the polyline.
    pub planarity: f64,
    /// Collect lines, polylines and curves as wires.
    pub wires: bool,
    /// Chordal tolerance of flattened curves in drawing units.
    pub tolerance: f64,
    /// Color of faces whose color resolves to black.
    pub neutral: Option<[u8; 3]>,
    /// Background wire colors are contrasted against.
    pub background: Option<[u8; 3]>,
    /// Most points collected before the walk stops.
    pub max_points: usize,
}

/// Walks `settings.model` and collects its faces and wires; `work` pays for hatch nesting.
pub(super) fn collect(doc: &Document, settings: &Settings, work: &mut u64) -> Collected {
    let mut collector = Collector {
        doc,
        settings,
        out: Collected::default(),
        layer_index: HashMap::new(),
        scratch: Document::new(),
        work,
    };
    for layer in &doc.layers {
        collector.layer(&layer.name);
    }
    let stats = doc.walk_while(settings.model, &settings.walk, |entity, ctx| {
        collector.visit(entity, ctx)
    });
    let mut out = collector.out;
    out.truncated |= stats.truncated;
    out
}

/// Start of an item in the pools, for completing or rolling it back.
#[derive(Debug, Clone, Copy)]
struct Mark {
    rings: usize,
    points: usize,
}

struct Collector<'d, 's, 'w> {
    doc: &'d Document,
    settings: &'s Settings,
    out: Collected,
    layer_index: HashMap<String, u32>,
    /// One-entity document reused to flatten curves with `Document::tessellate_curves`.
    scratch: Document,
    work: &'w mut u64,
}

impl Collector<'_, '_, '_> {
    fn layer(&mut self, name: &str) -> u32 {
        if let Some(index) = self.layer_index.get(name) {
            return *index;
        }
        let index = u32::try_from(self.out.layers.len()).unwrap_or(u32::MAX);
        self.out.layers.push(name.to_owned());
        self.layer_index.insert(name.to_owned(), index);
        index
    }

    fn skip(&mut self, kind: &'static str) {
        *self.out.skipped.entry(kind).or_default() += 1;
    }

    fn mark(&self) -> Mark {
        Mark {
            rings: self.out.rings.len(),
            points: self.out.points.len(),
        }
    }

    fn rollback(&mut self, mark: Mark) {
        self.out.rings.truncate(mark.rings);
        self.out.points.truncate(mark.points);
    }

    /// Places and cleans one ring; `Err` on a non-finite point, `Ok(false)` (nothing kept)
    /// when too few distinct points remain (three for a face, two for a wire).
    fn ring(
        &mut self,
        transform: &Transform,
        points: impl IntoIterator<Item = Point3>,
        closed: bool,
        face: bool,
    ) -> Result<bool, ()> {
        let start = self.out.points.len();
        for p in points {
            let q = transform.apply_point(p);
            if !q.is_finite() {
                self.out.points.truncate(start);
                return Err(());
            }
            if self.out.points.len() == start || self.out.points.last() != Some(&q) {
                self.out.points.push(q);
            }
        }
        let first = self.out.points.get(start).copied();
        if closed && self.out.points.len() > start + 1 && self.out.points.last().copied() == first {
            self.out.points.pop();
        }
        if self.out.points.len() - start < if face { 3 } else { 2 } {
            self.out.points.truncate(start);
            return Ok(false);
        }
        self.out.rings.push(Ring {
            points: start..self.out.points.len(),
            closed,
        });
        Ok(true)
    }

    /// Completes the item started at `mark`; false (and a rollback) when it has no ring.
    fn finish(
        &mut self,
        mark: Mark,
        entity: &Entity,
        ctx: &WalkContext<'_>,
        surface: bool,
        path: &mut Option<Range<usize>>,
    ) -> bool {
        if self.out.rings.len() == mark.rings {
            self.rollback(mark);
            return false;
        }
        let path = match path {
            Some(path) => path.clone(),
            None => {
                let start = self.out.paths.len();
                self.out.paths.extend(
                    ctx.path()
                        .into_iter()
                        .map(|i| u32::try_from(i).unwrap_or(u32::MAX)),
                );
                let range = start..self.out.paths.len();
                *path = Some(range.clone());
                range
            }
        };
        let (r, g, b) = entity.resolved_color_in(self.doc, ctx);
        let color = if surface {
            match self.settings.neutral {
                Some(neutral) if (r, g, b) == (0, 0, 0) => neutral,
                _ => [r, g, b],
            }
        } else {
            match self.settings.background {
                Some([br, bg, bb]) => {
                    let (r, g, b) = cadkit_core::svg::with_contrast((r, g, b), (br, bg, bb));
                    [r, g, b]
                }
                None => [r, g, b],
            }
        };
        let layer = self.layer(ctx.layer_name(entity));
        self.out.items.push(Item {
            rings: mark.rings..self.out.rings.len(),
            surface,
            layer,
            color,
            path,
            model_path_len: u32::try_from(ctx.model_path_len()).unwrap_or(u32::MAX),
            kind: entity.kind.type_name(),
        });
        true
    }

    /// One face of `rings` (outline first); degenerate when the outline has no area.
    fn face<'p>(
        &mut self,
        entity: &Entity,
        ctx: &WalkContext<'_>,
        rings: impl IntoIterator<Item = &'p [Point3]>,
        path: &mut Option<Range<usize>>,
    ) {
        let mark = self.mark();
        for (k, ring) in rings.into_iter().enumerate() {
            match self.ring(&ctx.transform, ring.iter().copied(), true, true) {
                Ok(true) => {}
                // A degenerate hole is left out; a degenerate outline drops the face.
                Ok(false) if k > 0 => {}
                _ => {
                    self.rollback(mark);
                    self.out.degenerate += 1;
                    return;
                }
            }
        }
        self.finish(mark, entity, ctx, true, path);
    }

    /// Wire geometry of one entity made of `rings`.
    fn wire<'p>(
        &mut self,
        entity: &Entity,
        ctx: &WalkContext<'_>,
        rings: impl IntoIterator<Item = (&'p [Point3], bool)>,
    ) {
        if !self.settings.wires {
            self.skip(entity.kind.type_name());
            return;
        }
        let mark = self.mark();
        for (ring, closed) in rings {
            if self
                .ring(&ctx.transform, ring.iter().copied(), closed, false)
                .is_err()
            {
                self.rollback(mark);
                break;
            }
        }
        if self.finish(mark, entity, ctx, false, &mut None) {
            self.out.wires += 1;
        } else {
            self.out.degenerate += 1;
        }
    }

    /// Visits one entity; false stops the walk when the point budget is used up.
    fn visit(&mut self, entity: &Entity, ctx: &WalkContext<'_>) -> bool {
        let faces = self.settings.faces;
        match &entity.kind {
            EntityKind::Polygon {
                exterior,
                interiors,
            } => {
                let rings = std::iter::once(exterior)
                    .chain(interiors)
                    .map(Vec::as_slice);
                if faces {
                    self.face(entity, ctx, rings, &mut None);
                } else {
                    self.wire(entity, ctx, rings.map(|r| (r, true)));
                }
            }
            EntityKind::Face { points, .. } => {
                if faces {
                    self.face(entity, ctx, [points.as_slice()], &mut None);
                } else {
                    self.wire(entity, ctx, [(points.as_slice(), true)]);
                }
            }
            EntityKind::Mesh {
                vertices,
                faces: indices,
            } => {
                self.mesh(entity, ctx, vertices, indices);
            }
            EntityKind::Hatch {
                loops, solid: true, ..
            } if faces => match straight_loops(loops) {
                Some(loops) => self.hatch(entity, ctx, &loops),
                None => self.skip("hatch"),
            },
            EntityKind::Polyline {
                vertices, closed, ..
            } => {
                let bulged = vertices.iter().any(|v| v.bulge != 0.);
                let points: Vec<Point3> = vertices.iter().map(|v| v.position).collect();
                if faces
                    && self.settings.polyline_faces
                    && !bulged
                    && self.polyline_face(entity, ctx, &points, *closed)
                {
                    return self.within_budget();
                }
                if bulged {
                    self.flattened(entity, ctx);
                } else {
                    self.wire(entity, ctx, [(points.as_slice(), *closed)]);
                }
            }
            EntityKind::Line { start, end } => {
                self.wire(entity, ctx, [([*start, *end].as_slice(), false)]);
            }
            EntityKind::Leader { vertices, .. } => {
                self.wire(entity, ctx, [(vertices.as_slice(), false)]);
            }
            EntityKind::Circle { .. }
            | EntityKind::Arc { .. }
            | EntityKind::Ellipse { .. }
            | EntityKind::Spline { .. } => self.flattened(entity, ctx),
            other => self.skip(other.type_name()),
        }
        self.within_budget()
    }

    fn within_budget(&mut self) -> bool {
        if self.out.points.len() > self.settings.max_points {
            self.out.truncated = true;
            return false;
        }
        true
    }

    fn mesh(
        &mut self,
        entity: &Entity,
        ctx: &WalkContext<'_>,
        vertices: &[Point3],
        faces: &[Vec<u32>],
    ) {
        let mut path = None;
        let wire = !self.settings.faces;
        if wire && !self.settings.wires {
            self.skip("mesh");
            return;
        }
        let mark = self.mark();
        let mut drawn = false;
        for face in faces {
            // A face with an index outside the vertex list is not drawn.
            let Some(ring) = face
                .iter()
                .map(|&i| vertices.get(i as usize).copied())
                .collect::<Option<Vec<Point3>>>()
            else {
                self.out.degenerate += 1;
                continue;
            };
            if wire {
                drawn |= self.ring(&ctx.transform, ring.iter().copied(), true, false) == Ok(true);
            } else {
                self.face(entity, ctx, [ring.as_slice()], &mut path);
            }
            if self.out.points.len() > self.settings.max_points {
                break;
            }
        }
        if wire {
            if drawn && self.finish(mark, entity, ctx, false, &mut None) {
                self.out.wires += 1;
            } else {
                self.rollback(mark);
                self.out.degenerate += 1;
            }
        }
    }

    /// Faces of a solid hatch whose loops are all straight: loops nested an even number of
    /// times are outlines, the others holes of the loop directly around them (even-odd fill,
    /// as the SVG export draws hatches).
    fn hatch(&mut self, entity: &Entity, ctx: &WalkContext<'_>, loops: &[Vec<Point3>]) {
        // Nesting is decided before the placement: an affine map keeps containment.
        let mut cleaned: Vec<Vec<Point3>> = Vec::new();
        for ring in loops {
            let mark = self.mark();
            if self.ring(&Transform::identity(), ring.iter().copied(), true, true) == Ok(true) {
                cleaned.push(
                    self.out
                        .points
                        .get(mark.points..)
                        .unwrap_or_default()
                        .to_vec(),
                );
            }
            self.rollback(mark);
        }
        let Some(normal) = cleaned.iter().find_map(|r| cadkit_core::polygon::normal(r)) else {
            self.out.degenerate += 1;
            return;
        };
        let axis = drop_axis(normal);
        let flat: Vec<Vec<[f64; 2]>> = cleaned
            .iter()
            .map(|r| r.iter().map(|p| project(*p, axis)).collect())
            .collect();
        let cost = flat
            .len()
            .saturating_mul(flat.iter().map(Vec::len).sum::<usize>());
        let cost = u64::try_from(cost).unwrap_or(u64::MAX);
        if cost > *self.work {
            *self.work = 0;
            self.out.truncated = true;
            return;
        }
        *self.work -= cost;
        // containers[i]: the loops around loop i.
        let containers: Vec<Vec<usize>> = flat
            .iter()
            .enumerate()
            .map(|(i, ring)| {
                let probe = ring.first().copied().unwrap_or_default();
                flat.iter()
                    .enumerate()
                    .filter(|(j, other)| *j != i && inside(other, probe))
                    .map(|(j, _)| j)
                    .collect()
            })
            .collect();
        let depth = |i: usize| containers.get(i).map_or(0, Vec::len);
        let mut path = None;
        for (i, outline) in cleaned.iter().enumerate() {
            if depth(i) % 2 == 1 {
                continue;
            }
            let holes = cleaned.iter().enumerate().filter(|(j, _)| {
                depth(*j) == depth(i) + 1 && containers.get(*j).is_some_and(|c| c.contains(&i))
            });
            let rings = std::iter::once(outline.as_slice()).chain(holes.map(|(_, h)| h.as_slice()));
            self.face(entity, ctx, rings, &mut path);
        }
    }

    /// A closed, planar polyline as a face; false when it is not one.
    fn polyline_face(
        &mut self,
        entity: &Entity,
        ctx: &WalkContext<'_>,
        points: &[Point3],
        closed: bool,
    ) -> bool {
        let returns = points.len() >= 4 && points.first() == points.last();
        if !closed && !returns {
            return false;
        }
        let mark = self.mark();
        if self.ring(&ctx.transform, points.iter().copied(), true, true) != Ok(true) {
            self.rollback(mark);
            return false;
        }
        let ring = self.out.points.get(mark.points..).unwrap_or_default();
        if !planar(ring, self.settings.planarity) {
            self.rollback(mark);
            return false;
        }
        self.finish(mark, entity, ctx, true, &mut None)
    }

    /// A curve (or bulged polyline) flattened with `Document::tessellate_curves`.
    fn flattened(&mut self, entity: &Entity, ctx: &WalkContext<'_>) {
        if !self.settings.wires {
            self.skip(entity.kind.type_name());
            return;
        }
        // Flatten in the entity's own units so the chordal error holds after the placement.
        let t = &ctx.transform;
        let scale = [t.x, t.y, t.z]
            .iter()
            .map(|v| v.length())
            .fold(0., f64::max);
        let tolerance = if scale.is_finite() && scale > 0. {
            self.settings.tolerance / scale
        } else {
            self.settings.tolerance
        };
        let model = self.scratch.model_space_mut();
        model.entities.clear();
        model.entities.push(Entity::new(entity.kind.clone()));
        self.scratch.tessellate_curves(tolerance);
        let flat = self
            .scratch
            .models
            .first()
            .and_then(|m| m.entities.first())
            .and_then(|e| match &e.kind {
                EntityKind::Polyline {
                    vertices, closed, ..
                } if vertices.iter().all(|v| v.bulge == 0.) => Some((
                    vertices.iter().map(|v| v.position).collect::<Vec<_>>(),
                    *closed,
                )),
                _ => None,
            });
        match flat {
            Some((points, closed)) => {
                let wires = self.out.wires;
                self.wire(entity, ctx, [(points.as_slice(), closed)]);
                if self.out.wires > wires {
                    self.out.flattened += 1;
                }
            }
            None => self.out.degenerate += 1,
        }
    }
}

/// The loops of a hatch if every edge is straight (lines, bulge-free polylines).
fn straight_loops(loops: &[HatchLoop]) -> Option<Vec<Vec<Point3>>> {
    loops
        .iter()
        .map(|l| {
            let mut ring = Vec::new();
            for edge in &l.edges {
                match edge {
                    HatchEdge::Line { start, end } => ring.extend([*start, *end]),
                    HatchEdge::Polyline { vertices, .. }
                        if vertices.iter().all(|v| v.bulge == 0.) =>
                    {
                        ring.extend(vertices.iter().map(|v| v.position));
                    }
                    _ => return None,
                }
            }
            Some(ring)
        })
        .collect()
}

/// The coordinate dropped when projecting a plane with `normal` onto two axes: the one the
/// normal is most aligned with, so the projection never collapses the polygon.
pub(super) fn drop_axis(normal: Vec3) -> usize {
    let (x, y, z) = (normal.x.abs(), normal.y.abs(), normal.z.abs());
    if x >= y && x >= z {
        0
    } else if y >= z {
        1
    } else {
        2
    }
}

/// `p` without coordinate `axis`; the remaining axes keep a right-handed order when the
/// dropped one is x or z and a left-handed one when it is y.
pub(super) fn project(p: Point3, axis: usize) -> [f64; 2] {
    match axis {
        0 => [p.y, p.z],
        1 => [p.x, p.z],
        _ => [p.x, p.y],
    }
}

fn inside(ring: &[[f64; 2]], q: [f64; 2]) -> bool {
    let mut result = false;
    let mut previous = ring.last().copied();
    for &a in ring {
        if let Some(b) = previous
            && (a[1] > q[1]) != (b[1] > q[1])
            && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
        previous = Some(a);
    }
    result
}

/// All points lie within `planarity` times the ring's size of the plane through its first
/// point.
fn planar(ring: &[Point3], planarity: f64) -> bool {
    let (Some(normal), Some(o)) = (cadkit_core::polygon::normal(ring), ring.first()) else {
        return false;
    };
    let mut lo = *o;
    let mut hi = *o;
    let mut off = 0f64;
    for p in ring {
        lo = Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        off = off.max(o.vector_to(*p).dot(normal).abs());
    }
    off <= planarity * lo.distance_to(hi)
}

/// Converts drawing coordinates to f32 offsets from an origin and tracks the error.
#[derive(Debug, Clone, Copy)]
pub(super) struct Encoder {
    pub origin: [f64; 3],
    pub components: usize,
    /// Fail when a coordinate's error exceeds this.
    pub limit: Option<f64>,
    pub max_error: f64,
}

impl Encoder {
    /// Appends `p` (x, y and, with three components, z) to `out`.
    pub fn push(&mut self, p: Point3, out: &mut Vec<f32>, vertex: usize) -> Result<(), SceneError> {
        for (value, start) in [p.x, p.y, p.z]
            .into_iter()
            .zip(self.origin)
            .take(self.components)
        {
            let Some((encoded, error)) = encode_relative(value, start) else {
                return Err(SceneError::Precision { vertex });
            };
            if self.limit.is_some_and(|limit| error > limit) {
                return Err(SceneError::Precision { vertex });
            }
            self.max_error = self.max_error.max(error);
            out.push(encoded);
        }
        Ok(())
    }
}

/// Segment buffers being filled.
#[derive(Debug, Default)]
pub(super) struct Segments {
    pub positions: Vec<f32>,
    pub colors: Vec<u8>,
    pub items: Vec<u32>,
}

impl Segments {
    /// Lengths to roll back to.
    pub fn mark(&self) -> [usize; 3] {
        [self.positions.len(), self.colors.len(), self.items.len()]
    }

    /// Drops what was appended since `mark`.
    pub fn rollback(&mut self, [positions, colors, items]: [usize; 3]) {
        self.positions.truncate(positions);
        self.colors.truncate(colors);
        self.items.truncate(items);
    }

    /// Appends the segments of `rings` for item `item`.
    pub fn push(
        &mut self,
        collected: &Collected,
        rings: &[Ring],
        color: [u8; 3],
        item: u32,
        encoder: &mut Encoder,
    ) -> Result<(), SceneError> {
        for ring in rings {
            let points = collected.points_of(ring);
            let wrap = if ring.closed && points.len() > 2 {
                points.first()
            } else {
                None
            };
            let ends = points.iter().skip(1).chain(wrap);
            for (a, b) in points.iter().zip(ends) {
                let vertex = self.items.len().saturating_mul(2);
                encoder.push(*a, &mut self.positions, vertex)?;
                encoder.push(*b, &mut self.positions, vertex.saturating_add(1))?;
                self.colors.extend(color);
                self.colors.extend(color);
                self.items.push(item);
            }
        }
        Ok(())
    }
}

/// Copies an item's walk path into `items` and returns its new index.
pub(super) fn push_item(items: &mut SceneItems, collected: &Collected, item: &Item) -> u32 {
    let index = u32::try_from(items.items.len()).unwrap_or(u32::MAX);
    let path = collected.paths.get(item.path.clone()).unwrap_or_default();
    let path_start = u32::try_from(items.paths.len()).unwrap_or(u32::MAX);
    items.paths.extend_from_slice(path);
    items.items.push(SceneItem {
        path_start,
        path_len: u32::try_from(path.len()).unwrap_or(u32::MAX),
        model_path_len: item.model_path_len,
        layer: item.layer,
        kind: item.kind,
        surface: item.surface,
    });
    index
}
