//! Persistent, chunked 2D store of lines, polylines, arcs and circles for GPU drawing.
//!
//! Objects are assigned to a square chunk by the center of their bounds. A chunk is the unit
//! of GPU buffers and culling: its instances are encoded once ([`crate::layout`]) relative to
//! the chunk center in f32 and reused until an object of the chunk changes. Every frame checks
//! the first-order rounding error of that representation against
//! [`Plan2dOptions::screen_error_budget_px`]; a chunk that exceeds it is returned to the
//! caller's CPU path for that frame instead of being drawn imprecisely.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cadkit_geom::{Bounds2, Curve, GeometryTolerance, Point2};

use crate::layout::{
    END_JOINT, INSTANCE_BYTES, KIND_ARC, START_JOINT, pack_direction, premultiply,
};
use crate::{GroupId, ObjectId};

/// Chunk buffer slots and content generations are unique in the process, so two stores
/// that share one renderer never mistake each other's buffers for current.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Tuning of a [`Plan2d`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Plan2dOptions {
    /// Edge of the square chunks, in model units. Default 32.
    pub chunk_size: f64,
    /// Margin around the canvas, in logical pixels, within which a chunk's strokes are drawn:
    /// half the widest stroke plus antialiasing. Default 4.
    pub stroke_margin_px: f64,
    /// Margin around the canvas, in logical pixels, within which objects marked
    /// [`PlanItem::label`] are reported, because their caller-drawn labels may reach beyond
    /// their bounds. Default 256.
    pub label_margin_px: f64,
    /// Largest accepted f32 representation error of a drawn chunk, in logical pixels.
    /// Default 0.125.
    pub screen_error_budget_px: f64,
    /// Tolerance used to validate curves before they are encoded for the GPU.
    pub tolerance: GeometryTolerance,
}

impl Default for Plan2dOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl Plan2dOptions {
    /// The default options.
    pub fn new() -> Self {
        Self {
            chunk_size: 32.,
            stroke_margin_px: 4.,
            label_margin_px: 256.,
            screen_error_budget_px: 0.125,
            tolerance: GeometryTolerance::default(),
        }
    }

    /// Sets [`Self::chunk_size`].
    #[must_use]
    pub fn with_chunk_size(mut self, chunk_size: f64) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Sets [`Self::stroke_margin_px`].
    #[must_use]
    pub fn with_stroke_margin_px(mut self, margin: f64) -> Self {
        self.stroke_margin_px = margin;
        self
    }

    /// Sets [`Self::label_margin_px`].
    #[must_use]
    pub fn with_label_margin_px(mut self, margin: f64) -> Self {
        self.label_margin_px = margin;
        self
    }

    /// Sets [`Self::screen_error_budget_px`].
    #[must_use]
    pub fn with_screen_error_budget_px(mut self, budget: f64) -> Self {
        self.screen_error_budget_px = budget;
        self
    }

    /// Sets [`Self::tolerance`].
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: GeometryTolerance) -> Self {
        self.tolerance = tolerance;
        self
    }
}

/// How one object is drawn.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PlanItem {
    /// Group of the object; see [`GroupId`].
    pub group: GroupId,
    /// Curves in model units. Lines, arcs, circles and polylines without repeated
    /// consecutive points are drawn on the GPU; an object with any other curve, or with a
    /// curve that fails validation, is returned in [`PlanFrame::cpu`] for the caller to draw.
    pub curves: Vec<Curve>,
    /// Unmultiplied sRGBA color.
    pub color: [u8; 4],
    /// Stroke width in logical pixels.
    pub width_px: f32,
    /// Bounds used for chunk assignment, culling and [`Plan2d::near`]; they may be larger
    /// than the curves (labels, snap points). `None`: the object has no known extent and is
    /// returned in [`PlanFrame::cpu`] every frame.
    pub bounds: Option<Bounds2>,
    /// The caller draws a label for this object on the CPU; see [`PlanFrame::labels`].
    pub label: bool,
}

impl PlanItem {
    /// An unlabelled item whose bounds are those of its curves ([`curve_bounds`]).
    pub fn new(group: GroupId, curves: Vec<Curve>, color: [u8; 4], width_px: f32) -> Self {
        let bounds = curve_bounds(&curves);
        Self {
            group,
            curves,
            color,
            width_px,
            bounds,
            label: false,
        }
    }

    /// Sets [`Self::bounds`].
    #[must_use]
    pub fn with_bounds(mut self, bounds: Option<Bounds2>) -> Self {
        self.bounds = bounds;
        self
    }

    /// Sets [`Self::label`].
    #[must_use]
    pub fn with_label(mut self, label: bool) -> Self {
        self.label = label;
        self
    }
}

/// Union of the bounds of `curves`; `None` when there are none or a curve has no bounds.
pub fn curve_bounds(curves: &[Curve]) -> Option<Bounds2> {
    curves.iter().try_fold(None, |bounds, curve| {
        Some(Some(merge_bounds(bounds, curve.bounds().ok()?)))
    })?
}

fn merge_bounds(first: Option<Bounds2>, second: Bounds2) -> Bounds2 {
    let Some(first) = first else {
        return second;
    };
    Bounds2 {
        min: Point2::new(first.min.x.min(second.min.x), first.min.y.min(second.min.y)),
        max: Point2::new(first.max.x.max(second.max.x), first.max.y.max(second.max.y)),
    }
}

/// The camera and canvas of one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PlanView {
    /// Model point at the canvas center.
    pub center: Point2,
    /// Logical pixels per model unit.
    pub zoom: f64,
    /// Canvas width and height in logical pixels.
    pub size_px: [f64; 2],
    /// Largest absolute window coordinate of the canvas, in logical pixels. The final pixel
    /// position adds it in f32, so it enters the error bound.
    pub window_extent_px: f64,
    /// Physical pixels per logical pixel.
    pub pixels_per_point: f64,
}

impl PlanView {
    /// A view of a `size_px` canvas centered on `center`, whose far corner lies
    /// `window_extent_px` from the window origin.
    pub fn new(center: Point2, zoom: f64, size_px: [f64; 2], window_extent_px: f64) -> Self {
        Self {
            center,
            zoom,
            size_px,
            window_extent_px,
            pixels_per_point: 1.,
        }
    }

    /// Sets [`Self::pixels_per_point`].
    #[must_use]
    pub fn with_pixels_per_point(mut self, pixels_per_point: f64) -> Self {
        self.pixels_per_point = pixels_per_point;
        self
    }
}

/// One chunk to draw on the GPU this frame.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChunkDraw {
    /// Process-unique buffer slot of the chunk.
    pub slot: u64,
    /// Content generation; a renderer re-uploads [`Self::bytes`] when it changes.
    pub generation: u64,
    /// Encoded instances ([`crate::layout`]).
    pub bytes: Arc<[u8]>,
    /// Chunk origin relative to the canvas center, in physical pixels, y down.
    pub offset: [f64; 2],
    /// Physical pixels per model unit.
    pub scale: f64,
    /// Instance ranges to draw; objects in the frame's overlay are cut out.
    pub ranges: Vec<Range<u32>>,
}

/// What to draw in one frame.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PlanFrame {
    /// Chunks for the GPU, in drawing order.
    pub draws: Vec<ChunkDraw>,
    /// Objects the caller draws completely on the CPU: unbounded objects, curves the GPU does
    /// not represent, chunks over the error budget or without prepared instances, and visible
    /// overlay objects.
    pub cpu: BTreeSet<ObjectId>,
    /// Labelled objects within the label margin whose strokes are on the GPU or off-canvas;
    /// the caller draws only their labels. Disjoint from [`Self::cpu`].
    pub labels: BTreeSet<ObjectId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ChunkKey {
    group: GroupId,
    x: i64,
    y: i64,
}

#[derive(Debug)]
struct Entry {
    item: PlanItem,
    /// Every curve is drawn on the GPU exactly as the CPU path would draw it.
    gpu: bool,
    chunk: Option<ChunkKey>,
}

#[derive(Debug)]
struct Chunk {
    origin: Point2,
    /// Drawing order is id order.
    ids: BTreeSet<ObjectId>,
    labelled: BTreeSet<ObjectId>,
    cpu: BTreeSet<ObjectId>,
    /// Union of the members' bounds; a superset after a removal until [`Plan2d::prepare`].
    bounds: Option<Bounds2>,
    stale_bounds: bool,
    slot: u64,
    generation: u64,
    instances: Option<Arc<Instances>>,
}

/// GPU instances of a chunk; each object owns one contiguous range.
#[derive(Debug)]
struct Instances {
    bytes: Arc<[u8]>,
    ranges: HashMap<ObjectId, Range<u32>>,
    count: u32,
    /// Largest absolute f32 local coordinate or radius extent, in model units.
    extent: f64,
}

/// Persistent chunked 2D store; see the [module documentation](self).
#[derive(Debug, Default)]
pub struct Plan2d {
    options: Plan2dOptions,
    entries: HashMap<ObjectId, Entry>,
    /// Group and grid order is GPU drawing order, so overlapping strokes keep their stacking.
    chunks: BTreeMap<ChunkKey, Chunk>,
    /// Objects without bounds; drawn by the caller every frame.
    always: BTreeSet<ObjectId>,
}

impl Plan2d {
    /// An empty store.
    pub fn new(options: Plan2dOptions) -> Self {
        Self {
            options,
            ..Self::default()
        }
    }

    /// The options of this store.
    pub fn options(&self) -> &Plan2dOptions {
        &self.options
    }

    /// Number of objects.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no objects.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The item stored for `id`.
    pub fn get(&self, id: ObjectId) -> Option<&PlanItem> {
        self.entries.get(&id).map(|entry| &entry.item)
    }

    /// Inserts or replaces the object `id`. Its old and new chunks are re-encoded on the
    /// next [`Self::prepare`] that sees them.
    pub fn upsert(&mut self, id: ObjectId, item: PlanItem) {
        self.remove(id);
        let tolerance = self.options.tolerance;
        let gpu = item.curves.iter().all(|curve| gpu_curve(curve, &tolerance));
        let mut entry = Entry {
            item,
            gpu,
            chunk: None,
        };
        match entry.item.bounds {
            Some(bounds) => {
                let size = self.options.chunk_size;
                let center = Point2::new(
                    bounds.min.x * 0.5 + bounds.max.x * 0.5,
                    bounds.min.y * 0.5 + bounds.max.y * 0.5,
                );
                let key = ChunkKey {
                    group: entry.item.group,
                    x: (center.x / size).floor() as i64,
                    y: (center.y / size).floor() as i64,
                };
                let chunk = self.chunks.entry(key).or_insert_with(|| Chunk {
                    origin: Point2::new((key.x as f64 + 0.5) * size, (key.y as f64 + 0.5) * size),
                    ids: BTreeSet::new(),
                    labelled: BTreeSet::new(),
                    cpu: BTreeSet::new(),
                    bounds: None,
                    stale_bounds: false,
                    slot: next_id(),
                    generation: 0,
                    instances: None,
                });
                chunk.ids.insert(id);
                if entry.item.label {
                    chunk.labelled.insert(id);
                }
                if !entry.gpu {
                    chunk.cpu.insert(id);
                }
                chunk.bounds = Some(merge_bounds(chunk.bounds, bounds));
                chunk.instances = None;
                chunk.generation = next_id();
                entry.chunk = Some(key);
            }
            None => {
                self.always.insert(id);
            }
        }
        self.entries.insert(id, entry);
    }

    /// Removes the object `id`; `false` when it was not stored.
    pub fn remove(&mut self, id: ObjectId) -> bool {
        let Some(entry) = self.entries.remove(&id) else {
            return false;
        };
        self.always.remove(&id);
        if let Some(key) = entry.chunk
            && let Some(chunk) = self.chunks.get_mut(&key)
        {
            chunk.ids.remove(&id);
            chunk.labelled.remove(&id);
            chunk.cpu.remove(&id);
            if chunk.ids.is_empty() {
                self.chunks.remove(&key);
            } else {
                chunk.stale_bounds = true;
                chunk.instances = None;
                chunk.generation = next_id();
            }
        }
        true
    }

    /// Removes every object.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.chunks.clear();
        self.always.clear();
    }

    /// Objects whose bounds may intersect `area` in a shown group, in id order, plus every
    /// object without bounds.
    pub fn near(&self, area: Bounds2, group_shown: impl Fn(GroupId) -> bool) -> BTreeSet<ObjectId> {
        let hit = |bounds: Option<Bounds2>| {
            bounds.is_none_or(|b| {
                b.max.x >= area.min.x
                    && b.min.x <= area.max.x
                    && b.max.y >= area.min.y
                    && b.min.y <= area.max.y
            })
        };
        let mut result: BTreeSet<ObjectId> = self
            .always
            .iter()
            .copied()
            .filter(|id| {
                self.entries
                    .get(id)
                    .is_some_and(|e| group_shown(e.item.group))
            })
            .collect();
        for (key, chunk) in &self.chunks {
            if !group_shown(key.group) || !hit(chunk.bounds) {
                continue;
            }
            result.extend(
                chunk
                    .ids
                    .iter()
                    .copied()
                    .filter(|id| self.entries.get(id).is_some_and(|e| hit(e.item.bounds))),
            );
        }
        result
    }

    /// Tightens chunk bounds after removals and encodes the missing instances of chunks
    /// visible in `view`. Call it before [`Self::frame`]; a chunk without instances is
    /// returned to the CPU path.
    pub fn prepare(&mut self, view: &PlanView, group_shown: impl Fn(GroupId) -> bool) {
        let entries = &self.entries;
        for chunk in self.chunks.values_mut().filter(|chunk| chunk.stale_bounds) {
            chunk.bounds = chunk
                .ids
                .iter()
                .filter_map(|id| entries.get(id).and_then(|entry| entry.item.bounds))
                .reduce(|a, b| merge_bounds(Some(a), b));
            chunk.stale_bounds = false;
        }
        let cull = Cull::new(view);
        let margin = self.options.stroke_margin_px;
        for (key, chunk) in &mut self.chunks {
            if chunk.instances.is_none()
                && group_shown(key.group)
                && cull.intersects(chunk.bounds, margin)
            {
                chunk.instances = Some(Arc::new(encode_chunk(entries, chunk)));
            }
        }
    }

    /// Splits this frame between the GPU and the caller's CPU path. `overlay` objects
    /// (selection, previews) are cut out of the GPU ranges and returned in
    /// [`PlanFrame::cpu`] when visible, so the caller can draw them highlighted. With
    /// `gpu == false` every visible object goes to the CPU path.
    pub fn frame(
        &self,
        view: &PlanView,
        group_shown: impl Fn(GroupId) -> bool,
        overlay: &BTreeSet<ObjectId>,
        gpu: bool,
    ) -> PlanFrame {
        let cull = Cull::new(view);
        let label_margin = self.options.label_margin_px;
        let stroke_margin = self.options.stroke_margin_px;
        let mut hidden: HashMap<ChunkKey, Vec<ObjectId>> = HashMap::new();
        for id in overlay {
            if let Some(key) = self.entries.get(id).and_then(|entry| entry.chunk) {
                hidden.entry(key).or_default().push(*id);
            }
        }
        let visible = |id: &ObjectId| {
            cull.intersects(
                self.entries.get(id).and_then(|e| e.item.bounds),
                label_margin,
            )
        };
        let mut plan = PlanFrame::default();
        for (key, chunk) in &self.chunks {
            if !group_shown(key.group) || !cull.intersects(chunk.bounds, label_margin) {
                continue;
            }
            let hidden = hidden.get(key);
            if !cull.intersects(chunk.bounds, stroke_margin) {
                // Strokes are off-canvas; only labels that reach in are drawn.
                plan.labels.extend(
                    chunk
                        .labelled
                        .iter()
                        .chain(hidden.into_iter().flatten())
                        .copied()
                        .filter(|id| visible(id)),
                );
                continue;
            }
            let draw = match (&chunk.instances, gpu) {
                (Some(instances), true) => chunk_draw(
                    chunk,
                    instances,
                    view,
                    self.options.screen_error_budget_px,
                    hidden,
                ),
                _ => None,
            };
            match draw {
                Some(draw) => {
                    plan.draws.extend(draw);
                    plan.labels
                        .extend(chunk.labelled.iter().copied().filter(|id| visible(id)));
                    plan.cpu.extend(chunk.cpu.iter().copied());
                }
                // No GPU this frame, no instances yet, or over the error budget.
                None => {
                    plan.cpu
                        .extend(chunk.ids.iter().copied().filter(|id| visible(id)));
                }
            }
            plan.cpu.extend(
                hidden
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|id| visible(id)),
            );
        }
        plan.cpu.extend(self.always.iter().copied().filter(|id| {
            self.entries
                .get(id)
                .is_some_and(|e| group_shown(e.item.group))
        }));
        for id in &plan.cpu {
            plan.labels.remove(id);
        }
        plan
    }
}

/// Visible model rectangle of a view; no culling when the view is degenerate.
struct Cull(Option<(Point2, [f64; 2], f64)>);

impl Cull {
    fn new(view: &PlanView) -> Self {
        let half = [view.size_px[0] / 2., view.size_px[1] / 2.];
        let valid = view.center.x.is_finite()
            && view.center.y.is_finite()
            && view.zoom.is_finite()
            && view.zoom > 0.
            && half.iter().all(|h| h.is_finite() && *h > 0.);
        // The canvas center need not lie on the pixel grid; keep one pixel of slack.
        Self(valid.then_some((view.center, [half[0] + 1., half[1] + 1.], view.zoom)))
    }

    /// Whether `bounds` meet the canvas grown by `margin` logical pixels. Objects without
    /// bounds always do.
    fn intersects(&self, bounds: Option<Bounds2>, margin: f64) -> bool {
        let (Some((center, half, zoom)), Some(bounds)) = (self.0, bounds) else {
            return true;
        };
        let reach = |half: f64| (half + margin) / zoom;
        let (x, y) = (reach(half[0]), reach(half[1]));
        // Rounding of far coordinates: two ulps of slack.
        let slack = |value: f64| value.abs() * 4. * f64::EPSILON;
        !(bounds.max.x < center.x - x - slack(center.x)
            || bounds.min.x > center.x + x + slack(center.x)
            || bounds.max.y < center.y - y - slack(center.y)
            || bounds.min.y > center.y + y + slack(center.y))
    }
}

/// GPU draw of a chunk: `None` when the f32 error exceeds the budget (CPU path),
/// `Some(None)` when nothing is left to draw.
fn chunk_draw(
    chunk: &Chunk,
    instances: &Arc<Instances>,
    view: &PlanView,
    budget_px: f64,
    hidden: Option<&Vec<ObjectId>>,
) -> Option<Option<ChunkDraw>> {
    let ppp = view.pixels_per_point;
    let scale = view.zoom * ppp;
    let offset = [
        (chunk.origin.x - view.center.x) * scale,
        (view.center.y - chunk.origin.y) * scale,
    ];
    // First-order rounding bound of the f32 local coordinate, the scale product, the chunk
    // offset and the addition of the window position; must stay within the budget.
    let frame = view.window_extent_px * ppp;
    let bound = (8. * instances.extent * scale
        + 2. * (offset[0].abs().max(offset[1].abs()) + frame)
        + 2. * frame)
        * 2f64.powi(-23);
    if !bound.is_finite() || !offset.iter().all(|v| v.is_finite()) {
        return None;
    }
    if bound > budget_px * ppp {
        return None;
    }
    let mut skipped: Vec<Range<u32>> = hidden
        .into_iter()
        .flatten()
        .filter_map(|id| instances.ranges.get(id).cloned())
        .collect();
    skipped.sort_by_key(|range| range.start);
    let mut ranges = Vec::new();
    let mut next = 0;
    for range in skipped {
        if range.start > next {
            ranges.push(next..range.start);
        }
        next = next.max(range.end);
    }
    if next < instances.count {
        ranges.push(next..instances.count);
    }
    if ranges.is_empty() {
        return Some(None);
    }
    Some(Some(ChunkDraw {
        slot: chunk.slot,
        generation: chunk.generation,
        bytes: instances.bytes.clone(),
        offset,
        scale,
        ranges,
    }))
}

/// Whether the GPU draws `curve` analytically, exactly where a CPU path that validates
/// curves would draw it.
fn gpu_curve(curve: &Curve, tolerance: &GeometryTolerance) -> bool {
    if curve.validate(tolerance).is_err() {
        return false;
    }
    match curve {
        Curve::Line(_) | Curve::Arc(_) | Curve::Circle(_) => true,
        // Repeated consecutive points (including the closing one) are invalid on the CPU
        // path; such an object stays there.
        Curve::Polyline(polyline) => {
            polyline.points.len() >= 2
                && polyline
                    .points
                    .windows(2)
                    .all(|pair| pair.first() != pair.last())
                && (!polyline.closed || polyline.points.first() != polyline.points.last())
        }
        _ => false,
    }
}

fn encode_chunk(entries: &HashMap<ObjectId, Entry>, chunk: &Chunk) -> Instances {
    let mut writer = Writer {
        bytes: Vec::new(),
        origin: chunk.origin,
        extent: 0.,
    };
    let mut ranges = HashMap::new();
    for id in &chunk.ids {
        let Some(entry) = entries.get(id) else {
            continue;
        };
        if !entry.gpu {
            continue;
        }
        let start = writer.count();
        let color = premultiply(entry.item.color);
        for curve in &entry.item.curves {
            writer.curve(curve, color, entry.item.width_px);
        }
        ranges.insert(*id, start..writer.count());
    }
    Instances {
        count: writer.count(),
        extent: writer.extent,
        bytes: writer.bytes.into(),
        ranges,
    }
}

/// Encodes curves relative to a chunk origin.
struct Writer {
    bytes: Vec<u8>,
    origin: Point2,
    extent: f64,
}

impl Writer {
    fn count(&self) -> u32 {
        u32::try_from(self.bytes.len() / INSTANCE_BYTES).unwrap_or(u32::MAX)
    }

    fn local(&mut self, point: Point2) -> [f32; 2] {
        let local = [point.x - self.origin.x, point.y - self.origin.y];
        self.extent = self.extent.max(local[0].abs()).max(local[1].abs());
        [local[0] as f32, local[1] as f32]
    }

    fn push(
        &mut self,
        p0: [f32; 2],
        p1: [f32; 2],
        extra: [u32; 2],
        color: [u8; 4],
        width: f32,
        flags: u32,
    ) {
        for value in [p0[0], p0[1], p1[0], p1[1]] {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.bytes.extend_from_slice(&extra[0].to_le_bytes());
        self.bytes.extend_from_slice(&extra[1].to_le_bytes());
        self.bytes.extend_from_slice(&color);
        self.bytes.extend_from_slice(&width.to_le_bytes());
        self.bytes.extend_from_slice(&flags.to_le_bytes());
    }

    fn curve(&mut self, curve: &Curve, color: [u8; 4], width: f32) {
        match curve {
            Curve::Line(line) => self.polyline(&[line.start, line.end], false, color, width),
            Curve::Polyline(polyline) => {
                self.polyline(&polyline.points, polyline.closed, color, width)
            }
            Curve::Arc(arc) => {
                let (start, sweep) = if arc.sweep_angle < 0. {
                    (arc.start_angle + arc.sweep_angle, -arc.sweep_angle)
                } else {
                    (arc.start_angle, arc.sweep_angle)
                };
                self.arc(arc.center, arc.radius, start, sweep, false, color, width)
            }
            Curve::Circle(circle) => {
                self.arc(circle.center, circle.radius, 0., TAU, true, color, width)
            }
            _ => {}
        }
    }

    /// Interior corners are split along their bisector: every pixel falls into one segment
    /// and the mitre shape is kept.
    fn polyline(&mut self, points: &[Point2], closed: bool, color: [u8; 4], width: f32) {
        let count = points.len();
        if count < 2 {
            return;
        }
        let local: Vec<[f32; 2]> = points.iter().map(|p| self.local(*p)).collect();
        let segments = if closed { count } else { count - 1 };
        let directions: Vec<(f64, f64)> = (0..segments)
            .map(|i| match (points.get(i), points.get((i + 1) % count)) {
                (Some(a), Some(b)) => {
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    let length = dx.hypot(dy);
                    (dx / length, dy / length)
                }
                _ => (0., 0.),
            })
            .collect();
        let direction = |i: usize| directions.get(i).copied().unwrap_or((0., 0.));
        // `joint(i)` is the corner at the start of segment i; open ends and reversals have none.
        let joint = |i: usize| -> Option<u32> {
            if !closed && (i == 0 || i == count - 1) {
                return None;
            }
            let (a, b) = (
                direction((i + segments - 1) % segments),
                direction(i % segments),
            );
            let (x, y) = (a.0 + b.0, a.1 + b.1);
            let length = x.hypot(y);
            (length > 1e-6).then(|| pack_direction(x / length, y / length))
        };
        for i in 0..segments {
            let (Some(p0), Some(p1)) = (local.get(i), local.get((i + 1) % count)) else {
                continue;
            };
            let start = joint(i);
            let end = joint((i + 1) % count);
            let flags = start.map_or(0, |_| START_JOINT) | end.map_or(0, |_| END_JOINT);
            self.push(
                *p0,
                *p1,
                [start.unwrap_or(0), end.unwrap_or(0)],
                color,
                width,
                flags,
            );
        }
    }

    /// Counter-clockwise pieces of at most a quarter turn; neighbours share the f32 boundary.
    #[allow(clippy::too_many_arguments)]
    fn arc(
        &mut self,
        center: Point2,
        radius: f64,
        start: f64,
        sweep: f64,
        full: bool,
        color: [u8; 4],
        width: f32,
    ) {
        let local = self.local(center);
        self.extent = self
            .extent
            .max((center.x - self.origin.x).abs() + radius)
            .max((center.y - self.origin.y).abs() + radius);
        let pieces = (sweep / FRAC_PI_2).ceil().max(1.) as usize;
        let boundary = |k: usize| -> f32 {
            if full && k == pieces {
                start as f32
            } else {
                (start + sweep * k as f64 / pieces as f64) as f32
            }
        };
        for k in 0..pieces {
            let flags = KIND_ARC
                | if full || k > 0 { START_JOINT } else { 0 }
                | if full || k + 1 < pieces { END_JOINT } else { 0 };
            self.push(
                local,
                [radius as f32, 0.],
                [boundary(k).to_bits(), boundary(k + 1).to_bits()],
                color,
                width,
                flags,
            );
        }
    }
}

#[cfg(test)]
mod tests;
