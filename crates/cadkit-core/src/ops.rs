//! Model traversal and derived properties: [`Document::walk`], [`Document::bbox`],
//! [`Entity::resolved_color`] and [`EntityKind::type_name`].

use std::collections::HashMap;
use std::rc::Rc;

use crate::aci;
use crate::geom::{BBox, Point3, Vec3};
use crate::geom_ops::{
    EllipseArc, Transform, bulge_to_arc, ellipse_arc, extend_bbox, nurbs_flatten,
};
use crate::model::{
    Attachment, Block, Color, Document, Entity, EntityKind, HAlign, HatchEdge, Layer, VAlign,
    Vertex,
};

/// Default nesting limit used by [`Document::bbox`] and the SVG renderer.
pub const DEFAULT_WALK_DEPTH: u32 = 32;

/// Default work budget of a walk, in abstract work units (see [`WalkOptions::max_work`]).
pub const DEFAULT_MAX_WORK: u64 = 20_000_000;

const BLACK: (u8, u8, u8) = (0, 0, 0);

/// State handed to the [`Document::walk`] callback together with each entity.
#[derive(Debug, Clone)]
pub struct WalkContext<'a> {
    /// Maps the entity's own coordinates to model-space coordinates (block placement,
    /// array offsets and nesting already composed).
    pub transform: Transform,
    /// Nesting depth: 0 for entities directly in the model, +1 per block / group level.
    pub depth: u32,
    /// Layer inherited from the inserting entity. Entities on layer `"0"` (or without a
    /// layer) inside a block take this layer, as AutoCAD does.
    pub inherited_layer: Option<&'a str>,
    /// Display color of the inserting entity, used to resolve `ByBlock` colors.
    pub block_color: Option<(u8, u8, u8)>,
    /// Layer name to color table shared by the walk, so color resolution needs no linear scan.
    layer_colors: Option<Rc<HashMap<&'a str, Color>>>,
}

impl<'a> WalkContext<'a> {
    /// Context of an entity that sits directly in a model.
    pub fn root() -> Self {
        Self {
            transform: Transform::identity(),
            depth: 0,
            inherited_layer: None,
            block_color: None,
            layer_colors: None,
        }
    }

    /// Effective layer name of `e` in this context (default layer `"0"`).
    pub fn layer_name<'x>(&'x self, e: &'x Entity) -> &'x str {
        match e.layer.as_deref() {
            None | Some("0") => self.inherited_layer.unwrap_or("0"),
            Some(name) => name,
        }
    }
}

/// Options of [`Document::bbox_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBoxOptions {
    /// Include `Image` entities in the box.
    pub include_images: bool,
    /// Work budget of the underlying walk (see [`WalkOptions::max_work`]); the box covers
    /// what was visited before the budget ran out.
    pub max_work: u64,
}

impl Default for BBoxOptions {
    fn default() -> Self {
        Self {
            include_images: true,
            max_work: DEFAULT_MAX_WORK,
        }
    }
}

/// Options of [`Document::walk_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkOptions {
    /// Deepest block / group nesting that is expanded.
    pub max_depth: u32,
    /// Expand `Insert` entities (and `Dimension` blocks). When false they are reported as-is.
    pub expand_inserts: bool,
    /// Work budget. Every visited entity costs 1 plus its size (vertices, spline samples,
    /// text bytes, hatch edges, mesh faces), every insert array cell costs 1. When the budget
    /// is used up the walk stops and [`WalkStats::truncated`] is set, so hostile nesting or
    /// giant arrays cannot hang the caller.
    pub max_work: u64,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_WALK_DEPTH,
            expand_inserts: true,
            max_work: DEFAULT_MAX_WORK,
        }
    }
}

/// Outcome of a walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WalkStats {
    /// The work budget ran out (or the callback asked to stop) before everything was visited.
    pub truncated: bool,
    /// Work units spent.
    pub work: u64,
}

/// Work units charged for the content of an entity (on top of 1 per entity).
fn entity_cost(kind: &EntityKind) -> u64 {
    let n = |len: usize| len as u64;
    match kind {
        EntityKind::Polyline { vertices, .. } => n(vertices.len()),
        EntityKind::Spline {
            control_points,
            fit_points,
            ..
        } => n(control_points.len().max(fit_points.len()))
            .saturating_mul(SPLINE_SAMPLES as u64)
            .min(100_000),
        EntityKind::Text { value, .. } => n(value.len()),
        EntityKind::MText { value, plain, .. } => n(value.len()) + n(plain.len()),
        EntityKind::Hatch { loops, .. } => loops
            .iter()
            .flat_map(|l| l.edges.iter())
            .map(|e| match e {
                HatchEdge::Polyline { vertices, .. } => 1 + n(vertices.len()),
                HatchEdge::Spline { control_points, .. } => {
                    1 + n(control_points.len())
                        .saturating_mul(SPLINE_SAMPLES as u64)
                        .min(100_000)
                }
                _ => 4,
            })
            .fold(0u64, u64::saturating_add),
        EntityKind::Dimension { points, .. } | EntityKind::Face { points, .. } => n(points.len()),
        EntityKind::Leader { vertices, .. } => n(vertices.len()),
        EntityKind::Mesh { vertices, faces } => {
            n(vertices.len())
                + faces
                    .iter()
                    .map(|f| n(f.len()) + 1)
                    .fold(0u64, u64::saturating_add)
        }
        _ => 0,
    }
}

struct Walker<'a, F> {
    blocks: HashMap<&'a str, &'a Block>,
    layers: HashMap<&'a str, &'a Layer>,
    colors: Rc<HashMap<&'a str, Color>>,
    opts: WalkOptions,
    work: u64,
    spent: u64,
    truncated: bool,
    stack: Vec<&'a str>,
    f: F,
}

impl<'a, F: FnMut(&'a Entity, &WalkContext<'a>) -> bool> Walker<'a, F> {
    fn layer_hidden(&self, name: &str) -> bool {
        self.layers
            .get(name)
            .is_some_and(|l| !l.visible || l.frozen)
    }

    fn color_of(&self, e: &Entity, ctx: &WalkContext<'a>) -> (u8, u8, u8) {
        let name = ctx.layer_name(e);
        resolve_color(e.color, || self.colors.get(name).copied(), ctx.block_color)
    }

    /// Spends `cost` work units; false (and truncation) when the budget cannot cover it.
    fn charge(&mut self, cost: u64) -> bool {
        if self.truncated {
            return false;
        }
        if cost > self.work {
            self.work = 0;
            self.truncated = true;
            return false;
        }
        self.work -= cost;
        self.spent += cost;
        true
    }

    fn emit(&mut self, e: &'a Entity, ctx: &WalkContext<'a>) {
        if !(self.f)(e, ctx) {
            self.truncated = true;
        }
    }

    fn visit_list(&mut self, list: &'a [Entity], ctx: &WalkContext<'a>) {
        for e in list {
            if self.truncated {
                return;
            }
            self.visit(e, ctx);
        }
    }

    fn visit(&mut self, e: &'a Entity, ctx: &WalkContext<'a>) {
        if !self.charge(1 + entity_cost(&e.kind)) {
            return;
        }
        let layer = ctx.layer_name(e);
        if !e.visible || self.layer_hidden(layer) {
            return;
        }
        match &e.kind {
            EntityKind::Insert {
                block,
                position,
                scale,
                rotation,
                normal,
                columns,
                rows,
                column_spacing,
                row_spacing,
            } if self.opts.expand_inserts => {
                let Some(def) = self.blocks.get(block.as_str()).copied() else {
                    self.emit(e, ctx);
                    return;
                };
                if ctx.depth >= self.opts.max_depth || self.stack.contains(&def.name.as_str()) {
                    self.emit(e, ctx);
                    return;
                }
                let child_layer: Option<&'a str> = match e.layer.as_deref() {
                    None | Some("0") => ctx.inherited_layer,
                    Some(l) => Some(l),
                };
                let block_color = Some(self.color_of(e, ctx));
                self.stack.push(def.name.as_str());
                'outer: for r in 0..(*rows).max(1) {
                    for c in 0..(*columns).max(1) {
                        if !self.charge(1) {
                            break 'outer;
                        }
                        let offset = Vec3::new(
                            f64::from(c) * column_spacing,
                            f64::from(r) * row_spacing,
                            0.0,
                        );
                        let local = Transform::from_insert_offset(
                            *position,
                            *scale,
                            *rotation,
                            *normal,
                            def.base_point,
                            offset,
                        );
                        let child = WalkContext {
                            transform: ctx.transform.compose(&local),
                            depth: ctx.depth + 1,
                            inherited_layer: child_layer,
                            block_color,
                            layer_colors: ctx.layer_colors.clone(),
                        };
                        self.visit_list(&def.entities, &child);
                    }
                }
                self.stack.pop();
            }
            EntityKind::Dimension {
                block: Some(name), ..
            } if self.opts.expand_inserts => match self.blocks.get(name.as_str()).copied() {
                Some(def)
                    if ctx.depth < self.opts.max_depth
                        && !self.stack.contains(&def.name.as_str()) =>
                {
                    let child_layer: Option<&'a str> = match e.layer.as_deref() {
                        None | Some("0") => ctx.inherited_layer,
                        Some(l) => Some(l),
                    };
                    let child = WalkContext {
                        transform: ctx.transform,
                        depth: ctx.depth + 1,
                        inherited_layer: child_layer,
                        block_color: Some(self.color_of(e, ctx)),
                        layer_colors: ctx.layer_colors.clone(),
                    };
                    self.stack.push(def.name.as_str());
                    self.visit_list(&def.entities, &child);
                    self.stack.pop();
                }
                _ => self.emit(e, ctx),
            },
            EntityKind::Group { children, .. } => {
                if ctx.depth < self.opts.max_depth {
                    let child = WalkContext {
                        depth: ctx.depth + 1,
                        ..ctx.clone()
                    };
                    self.visit_list(children, &child);
                }
            }
            _ => self.emit(e, ctx),
        }
    }
}

impl Document {
    /// Visits the drawable entities of `models[model_index]` with their model-space transform.
    ///
    /// - `Insert` entities are expanded through the block table (name lookup, arrays,
    ///   nesting up to `max_depth`); the callback sees the block's entities, not the insert.
    ///   An insert whose block is missing, that would recurse into a block already being
    ///   expanded (cycle), or that sits deeper than `max_depth` is reported itself.
    /// - `Dimension` entities that name an existing block are expanded the same way; others
    ///   are reported as-is.
    /// - `Group` children are visited with the group's transform; the group itself is not.
    /// - Entities with `visible == false` and entities on hidden or frozen layers are skipped
    ///   (together with everything below them).
    ///
    /// A missing model index visits nothing. The traversal is limited by the default work
    /// budget ([`DEFAULT_MAX_WORK`]) so malicious block nesting cannot hang the caller.
    pub fn walk<'a, F>(&'a self, model_index: usize, max_depth: u32, mut f: F)
    where
        F: FnMut(&'a Entity, &WalkContext<'a>),
    {
        let opts = WalkOptions {
            max_depth,
            ..WalkOptions::default()
        };
        self.walk_while(model_index, &opts, |e, c| {
            f(e, c);
            true
        });
    }

    /// [`Document::walk`] with explicit [`WalkOptions`]; reports whether the work budget cut
    /// the traversal short.
    pub fn walk_with<'a, F>(
        &'a self,
        model_index: usize,
        options: &WalkOptions,
        mut f: F,
    ) -> WalkStats
    where
        F: FnMut(&'a Entity, &WalkContext<'a>),
    {
        self.walk_while(model_index, options, |e, c| {
            f(e, c);
            true
        })
    }

    /// Like [`Document::walk_with`], but the callback returns `false` to stop the traversal
    /// (reported as `truncated`).
    pub fn walk_while<'a, F>(&'a self, model_index: usize, options: &WalkOptions, f: F) -> WalkStats
    where
        F: FnMut(&'a Entity, &WalkContext<'a>) -> bool,
    {
        let Some(model) = self.models.get(model_index) else {
            return WalkStats::default();
        };
        let mut blocks = HashMap::new();
        for b in &self.blocks {
            blocks.entry(b.name.as_str()).or_insert(b);
        }
        let mut layers = HashMap::new();
        for l in &self.layers {
            layers.entry(l.name.as_str()).or_insert(l);
        }
        let mut colors = HashMap::new();
        for l in &self.layers {
            colors.entry(l.name.as_str()).or_insert(l.color);
        }
        let colors = Rc::new(colors);
        let mut w = Walker {
            blocks,
            layers,
            colors: Rc::clone(&colors),
            opts: *options,
            work: options.max_work,
            spent: 0,
            truncated: false,
            stack: Vec::new(),
            f,
        };
        let root = WalkContext {
            layer_colors: Some(colors),
            ..WalkContext::root()
        };
        w.visit_list(&model.entities, &root);
        WalkStats {
            truncated: w.truncated,
            work: w.spent,
        }
    }

    /// Bounding box (model-space XYZ) of everything [`Document::walk`] visits in
    /// `models[model_index]`, or `None` when there is no geometry.
    ///
    /// Arcs, circles and ellipses use their exact extents, splines their sampled curve,
    /// polylines include bulge arcs. Text is approximated (glyph width 0.6 x height) because
    /// fonts are unknown; viewports and unknown records contribute nothing.
    pub fn bbox(&self, model_index: usize) -> Option<BBox> {
        self.bbox_with(model_index, &BBoxOptions::default())
    }

    /// [`Document::bbox`] with options; with `include_images: false` raster image frames are
    /// ignored (their stored extent can dwarf the drawing, e.g. DGN raster frames).
    pub fn bbox_with(&self, model_index: usize, options: &BBoxOptions) -> Option<BBox> {
        let mut bb = None;
        let walk = WalkOptions {
            max_work: options.max_work,
            ..WalkOptions::default()
        };
        self.walk_with(model_index, &walk, |e, ctx| {
            if !options.include_images && matches!(e.kind, EntityKind::Image { .. }) {
                return;
            }
            entity_extents(e, &ctx.transform, &mut bb);
        });
        bb
    }
}

fn display_rgb(c: Color) -> Option<(u8, u8, u8)> {
    match c {
        Color::Rgb { r, g, b } => Some((r, g, b)),
        Color::Aci { index: 0 } | Color::ByLayer | Color::ByBlock => None,
        Color::Aci { index: 7 } => Some(BLACK),
        Color::Aci { index } => Some(aci::to_rgb(index)),
    }
}

/// Shared color rule, see [`Entity::resolved_color`].
fn resolve_color(
    c: Color,
    layer_color: impl FnOnce() -> Option<Color>,
    block: Option<(u8, u8, u8)>,
) -> (u8, u8, u8) {
    if let Some(rgb) = display_rgb(c) {
        return rgb;
    }
    match c {
        Color::ByLayer => layer_color().and_then(display_rgb).unwrap_or(BLACK),
        _ => block.unwrap_or(BLACK),
    }
}

impl Entity {
    /// Display color as RGB, resolved without block context (`ByBlock` becomes black).
    ///
    /// Rules: `Rgb` as stored; `Aci` through the AutoCAD table, except ACI 7 (the
    /// "foreground" color, white on dark and black on light backgrounds) which is mapped to
    /// black to read on a white page; `ByLayer` uses the layer's color (layer `"0"` when the
    /// entity has none; a missing layer, or a layer color that is itself `ByLayer`/`ByBlock`,
    /// gives black); `ByBlock` gives black here and the inserting entity's color via
    /// [`Entity::resolved_color_in`].
    pub fn resolved_color(&self, doc: &Document) -> (u8, u8, u8) {
        self.resolved_color_in(doc, &WalkContext::root())
    }

    /// [`Entity::resolved_color`] inside a [`Document::walk`] callback: `ByBlock` resolves to
    /// the color of the inserting entity and layer `"0"` to the inherited layer.
    pub fn resolved_color_in(&self, doc: &Document, ctx: &WalkContext<'_>) -> (u8, u8, u8) {
        let name = ctx.layer_name(self);
        resolve_color(
            self.color,
            || match &ctx.layer_colors {
                Some(map) => map.get(name).copied(),
                None => doc.layers.iter().find(|l| l.name == name).map(|l| l.color),
            },
            ctx.block_color,
        )
    }
}

impl EntityKind {
    /// Stable lowercase name of the variant (matches the JSON `type` tag).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Point { .. } => "point",
            Self::Line { .. } => "line",
            Self::Polyline { .. } => "polyline",
            Self::Circle { .. } => "circle",
            Self::Arc { .. } => "arc",
            Self::Ellipse { .. } => "ellipse",
            Self::Spline { .. } => "spline",
            Self::Text { .. } => "text",
            Self::MText { .. } => "mtext",
            Self::Insert { .. } => "insert",
            Self::Hatch { .. } => "hatch",
            Self::Dimension { .. } => "dimension",
            Self::Face { .. } => "face",
            Self::Leader { .. } => "leader",
            Self::Image { .. } => "image",
            Self::Viewport { .. } => "viewport",
            Self::Mesh { .. } => "mesh",
            Self::Group { .. } => "group",
            Self::Unknown { .. } => "unknown",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Shared primitives (also used by the SVG renderer)
// ---------------------------------------------------------------------------------------------

/// Circle/arc around the world point `center` in the plane of `normal`, mapped through `tf`.
pub(crate) fn circle_arc(
    center: Point3,
    radius: f64,
    start: f64,
    end: f64,
    normal: Vec3,
    tf: &Transform,
) -> EllipseArc {
    EllipseArc::circle(Point3::default(), radius, start, end)
        .transformed(&tf.compose(&Transform::plane(center, normal)))
}

/// Ellipse mapped through `tf` (ellipse data is in WCS).
pub(crate) fn full_ellipse_arc(
    center: Point3,
    major: Vec3,
    ratio: f64,
    normal: Vec3,
    start: f64,
    end: f64,
    tf: &Transform,
) -> EllipseArc {
    ellipse_arc(center, major, ratio, normal, start, end).transformed(tf)
}

/// Arc of the segment `a -> b` if `a.bulge` makes it curved, mapped through `tf`.
///
/// The bulge turns counter-clockwise about `normal`, so the arc is solved in the plane
/// through `a` spanned by `arbitrary_axes(normal)`.
pub(crate) fn segment_arc(
    a: &Vertex,
    b: &Vertex,
    normal: Vec3,
    tf: &Transform,
) -> Option<EllipseArc> {
    let plane = Transform::plane(a.position, normal);
    let d = a.position.vector_to(b.position);
    let local_b = (d.dot(plane.x), d.dot(plane.y));
    let arc = bulge_to_arc((0.0, 0.0), local_b, a.bulge)?;
    let e = EllipseArc {
        center: Point3::new(arc.center.0, arc.center.1, 0.0),
        u: Vec3::new(arc.radius, 0.0, 0.0),
        v: Vec3::new(0.0, arc.radius, 0.0),
        t0: arc.start_angle,
        t1: arc.start_angle + arc.sweep,
    };
    Some(e.transformed(&tf.compose(&plane)))
}

/// Samples per knot span when splines are flattened.
pub(crate) const SPLINE_SAMPLES: usize = 16;

/// A spline as a polyline in model space: sampled NURBS, else fit points, else control points.
pub(crate) fn spline_points(
    degree: u32,
    knots: &[f64],
    control: &[Point3],
    weights: &[f64],
    fit: &[Point3],
    tf: &Transform,
) -> Vec<Point3> {
    let pts = if control.len() >= 2 {
        nurbs_flatten(degree, knots, control, weights, SPLINE_SAMPLES)
    } else if fit.len() >= 2 {
        fit.to_vec()
    } else {
        control.to_vec()
    };
    pts.into_iter().map(|p| tf.apply_point(p)).collect()
}

/// Rough text extents: `(x range, y range)` relative to the insertion point, before rotation.
fn text_box(
    chars: usize,
    height: f64,
    width_factor: f64,
    halign: HAlign,
    valign: VAlign,
) -> ((f64, f64), (f64, f64)) {
    let wf = if width_factor.is_finite() && width_factor > 0.0 {
        width_factor
    } else {
        1.0
    };
    let w = 0.6 * height * wf * chars.max(1) as f64;
    // Aligned/fit text without an end point starts at the baseline origin.
    let valign = if matches!(halign, HAlign::Aligned | HAlign::Fit) {
        VAlign::Baseline
    } else {
        valign
    };
    let x = match halign {
        HAlign::Left | HAlign::Aligned | HAlign::Fit => (0.0, w),
        HAlign::Right => (-w, 0.0),
        _ => (-w / 2.0, w / 2.0),
    };
    let y = match valign {
        VAlign::Baseline | VAlign::Bottom => (0.0, height),
        VAlign::Middle => (-height / 2.0, height / 2.0),
        VAlign::Top => (-height, 0.0),
    };
    (x, y)
}

fn extend_box(
    bb: &mut Option<BBox>,
    origin: Point3,
    rotation: f64,
    x: (f64, f64),
    y: (f64, f64),
    tf: &Transform,
) {
    let (s, c) = rotation.sin_cos();
    for px in [x.0, x.1] {
        for py in [y.0, y.1] {
            let p = Point3::new(
                origin.x + px * c - py * s,
                origin.y + px * s + py * c,
                origin.z,
            );
            extend_bbox(bb, tf.apply_point(p));
        }
    }
}

/// Lines of an MText entity (plain text preferred, format codes stripped by the reader).
pub(crate) fn mtext_text_lines(value: &str, plain: &str) -> Vec<String> {
    if !plain.is_empty() {
        plain
            .split('\n')
            .map(|s| s.trim_end_matches('\r').to_owned())
            .collect()
    } else {
        value.split("\\P").map(str::to_owned).collect()
    }
}

/// Horizontal / vertical anchor of an MText attachment point: (-1 left | 0 center | 1 right,
/// -1 top | 0 middle | 1 bottom).
pub(crate) fn attachment_anchor(a: Attachment) -> (i8, i8) {
    match a {
        Attachment::TopLeft => (-1, -1),
        Attachment::TopCenter => (0, -1),
        Attachment::TopRight => (1, -1),
        Attachment::MiddleLeft => (-1, 0),
        Attachment::MiddleCenter => (0, 0),
        Attachment::MiddleRight => (1, 0),
        Attachment::BottomLeft => (-1, 1),
        Attachment::BottomCenter => (0, 1),
        Attachment::BottomRight => (1, 1),
    }
}

/// Distance between MText baselines for a text height and line-spacing factor.
pub(crate) fn mtext_line_height(height: f64, line_spacing: f64) -> f64 {
    let f = if line_spacing.is_finite() && line_spacing > 0.0 {
        line_spacing
    } else {
        1.0
    };
    height * f * (5.0 / 3.0)
}

fn extend_hatch_edge(bb: &mut Option<BBox>, edge: &HatchEdge, normal: Vec3, tf: &Transform) {
    match edge {
        HatchEdge::Line { start, end } => {
            extend_bbox(bb, tf.apply_point(*start));
            extend_bbox(bb, tf.apply_point(*end));
        }
        HatchEdge::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        } => {
            circle_arc(*center, *radius, *start_angle, *end_angle, normal, tf).extend_bbox(bb);
        }
        HatchEdge::Ellipse {
            center,
            major_axis,
            ratio,
            start_param,
            end_param,
            ..
        } => {
            full_ellipse_arc(
                *center,
                *major_axis,
                *ratio,
                normal,
                *start_param,
                *end_param,
                tf,
            )
            .extend_bbox(bb);
        }
        HatchEdge::Spline {
            degree,
            knots,
            control_points,
            weights,
        } => {
            for p in spline_points(*degree, knots, control_points, weights, &[], tf) {
                extend_bbox(bb, p);
            }
        }
        HatchEdge::Polyline { vertices, closed } => {
            extend_polyline(bb, vertices, *closed, normal, tf)
        }
    }
}

fn extend_polyline(
    bb: &mut Option<BBox>,
    vertices: &[Vertex],
    closed: bool,
    normal: Vec3,
    tf: &Transform,
) {
    for v in vertices {
        extend_bbox(bb, tf.apply_point(v.position));
    }
    let n = vertices.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        if let (Some(a), Some(b)) = (vertices.get(i), vertices.get((i + 1) % n.max(1))) {
            if let Some(arc) = segment_arc(a, b, normal, tf) {
                arc.extend_bbox(bb);
            }
        }
    }
}

fn entity_extents(e: &Entity, tf: &Transform, bb: &mut Option<BBox>) {
    match &e.kind {
        EntityKind::Point { position } => extend_bbox(bb, tf.apply_point(*position)),
        EntityKind::Line { start, end } => {
            extend_bbox(bb, tf.apply_point(*start));
            extend_bbox(bb, tf.apply_point(*end));
        }
        EntityKind::Polyline {
            vertices,
            closed,
            normal,
        } => extend_polyline(bb, vertices, *closed, *normal, tf),
        EntityKind::Circle {
            center,
            radius,
            normal,
        } => {
            circle_arc(*center, *radius, 0.0, 0.0, *normal, tf).extend_bbox(bb);
        }
        EntityKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            normal,
        } => {
            circle_arc(*center, *radius, *start_angle, *end_angle, *normal, tf).extend_bbox(bb);
        }
        EntityKind::Ellipse {
            center,
            major_axis,
            ratio,
            start_param,
            end_param,
            normal,
        } => {
            full_ellipse_arc(
                *center,
                *major_axis,
                *ratio,
                *normal,
                *start_param,
                *end_param,
                tf,
            )
            .extend_bbox(bb);
        }
        EntityKind::Spline {
            degree,
            knots,
            control_points,
            weights,
            fit_points,
            ..
        } => {
            for p in spline_points(*degree, knots, control_points, weights, fit_points, tf) {
                extend_bbox(bb, p);
            }
        }
        EntityKind::Text {
            position,
            end_point: Some(end),
            height,
            normal,
            ..
        } => {
            // Aligned / fit text: the baseline runs from `position` to `end_point`.
            let dir = position.vector_to(*end);
            let up = normal
                .cross(dir)
                .normalized()
                .unwrap_or(Vec3::new(0.0, 1.0, 0.0))
                .scaled(*height);
            for p in [*position, *end] {
                extend_bbox(bb, tf.apply_point(p));
                extend_bbox(bb, tf.apply_point(p.translated(up)));
            }
        }
        EntityKind::Text {
            position,
            height,
            rotation,
            width_factor,
            value,
            halign,
            valign,
            normal,
            ..
        } => {
            let (x, y) = text_box(
                value.chars().count(),
                *height,
                *width_factor,
                *halign,
                *valign,
            );
            extend_box(
                bb,
                Point3::default(),
                *rotation,
                x,
                y,
                &tf.compose(&Transform::plane(*position, *normal)),
            );
        }
        EntityKind::MText {
            position,
            height,
            width,
            rotation,
            value,
            plain,
            attachment,
            line_spacing,
            normal,
            ..
        } => {
            let lines = mtext_text_lines(value, plain);
            let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1);
            let w = if *width > 0.0 && width.is_finite() {
                *width
            } else {
                0.6 * height * widest as f64
            };
            let h = mtext_line_height(*height, *line_spacing) * lines.len().max(1) as f64;
            let (ha, va) = attachment_anchor(*attachment);
            let x = match ha {
                -1 => (0.0, w),
                0 => (-w / 2.0, w / 2.0),
                _ => (-w, 0.0),
            };
            let y = match va {
                -1 => (-h, 0.0),
                0 => (-h / 2.0, h / 2.0),
                _ => (0.0, h),
            };
            extend_box(
                bb,
                Point3::default(),
                *rotation,
                x,
                y,
                &tf.compose(&Transform::plane(*position, *normal)),
            );
        }
        EntityKind::Insert { position, .. } => {
            extend_bbox(bb, tf.apply_point(*position));
        }
        EntityKind::Hatch { loops, normal, .. } => {
            for l in loops {
                for edge in &l.edges {
                    extend_hatch_edge(bb, edge, *normal, tf);
                }
            }
        }
        EntityKind::Dimension {
            points,
            text_position,
            ..
        } => {
            for p in points.iter().chain(text_position.iter()) {
                extend_bbox(bb, tf.apply_point(*p));
            }
        }
        EntityKind::Face { points, .. } => {
            for p in points {
                extend_bbox(bb, tf.apply_point(*p));
            }
        }
        EntityKind::Leader { vertices, .. } => {
            for p in vertices {
                extend_bbox(bb, tf.apply_point(*p));
            }
        }
        EntityKind::Image {
            position,
            u_vector,
            v_vector,
            ..
        } => {
            for (a, b) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let p = position.translated(u_vector.scaled(a).plus(v_vector.scaled(b)));
                extend_bbox(bb, tf.apply_point(p));
            }
        }
        EntityKind::Mesh { vertices, .. } => {
            for p in vertices {
                extend_bbox(bb, tf.apply_point(*p));
            }
        }
        EntityKind::Viewport { .. } | EntityKind::Group { .. } | EntityKind::Unknown { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;
    use crate::model::{Model, Units};

    fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Entity {
        Entity::new(EntityKind::Line {
            start: Point3::xy(x0, y0),
            end: Point3::xy(x1, y1),
        })
    }

    fn insert(block: &str, x: f64, y: f64) -> Entity {
        Entity::new(EntityKind::Insert {
            block: block.to_owned(),
            position: Point3::xy(x, y),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            normal: Vec3::Z,
            columns: 1,
            rows: 1,
            column_spacing: 0.0,
            row_spacing: 0.0,
        })
    }

    fn doc_with(models: Vec<Entity>, blocks: Vec<Block>) -> Document {
        Document {
            units: Units::default(),
            blocks,
            models: vec![Model {
                name: "Model".into(),
                entities: models,
                ..Model::default()
            }],
            ..Document::default()
        }
    }

    fn block(name: &str, entities: Vec<Entity>) -> Block {
        Block {
            name: name.to_owned(),
            entities,
            ..Block::default()
        }
    }

    fn collect(doc: &Document, depth: u32) -> Vec<(Point3, u32)> {
        let mut out = Vec::new();
        doc.walk(0, depth, |e, ctx| {
            if let EntityKind::Line { start, .. } = &e.kind {
                out.push((ctx.transform.apply_point(*start), ctx.depth));
            }
        });
        out
    }

    #[test]
    fn walk_expands_nested_inserts() {
        let inner = block("inner", vec![line(0.0, 0.0, 1.0, 0.0)]);
        let outer = block("outer", vec![insert("inner", 5.0, 0.0)]);
        let doc = doc_with(vec![insert("outer", 0.0, 10.0)], vec![inner, outer]);
        let hits = collect(&doc, 8);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, Point3::new(5.0, 10.0, 0.0));
        assert_eq!(hits[0].1, 2);
    }

    #[test]
    fn walk_depth_limit_reports_insert() {
        let inner = block("inner", vec![line(0.0, 0.0, 1.0, 0.0)]);
        let outer = block("outer", vec![insert("inner", 5.0, 0.0)]);
        let doc = doc_with(vec![insert("outer", 0.0, 0.0)], vec![inner, outer]);
        let mut inserts = 0;
        let mut lines = 0;
        doc.walk(0, 1, |e, _| match e.kind {
            EntityKind::Insert { .. } => inserts += 1,
            EntityKind::Line { .. } => lines += 1,
            _ => {}
        });
        assert_eq!((inserts, lines), (1, 0));
    }

    #[test]
    fn walk_survives_cycles() {
        let a = block("a", vec![line(0.0, 0.0, 1.0, 1.0), insert("b", 1.0, 0.0)]);
        let b = block("b", vec![insert("a", 1.0, 0.0)]);
        let selfref = block("s", vec![insert("s", 0.0, 0.0)]);
        let doc = doc_with(
            vec![insert("a", 0.0, 0.0), insert("s", 0.0, 0.0)],
            vec![a, b, selfref],
        );
        let mut n = 0;
        doc.walk(0, 64, |_, _| n += 1);
        // line of a, the unexpandable insert of a inside b, and the unexpandable self insert.
        assert_eq!(n, 3);
        assert!(doc.bbox(0).is_some());
    }

    #[test]
    fn walk_arrays_and_scale() {
        let blk = block("b", vec![line(0.0, 0.0, 1.0, 0.0)]);
        let mut ins = insert("b", 0.0, 0.0);
        ins.kind = EntityKind::Insert {
            block: "b".into(),
            position: Point3::xy(0.0, 0.0),
            scale: Vec3::new(2.0, 2.0, 1.0),
            rotation: PI / 2.0,
            normal: Vec3::Z,
            columns: 2,
            rows: 2,
            column_spacing: 10.0,
            row_spacing: 20.0,
        };
        let doc = doc_with(vec![ins], vec![blk]);
        let mut starts = Vec::new();
        doc.walk(0, 8, |e, ctx| {
            if let EntityKind::Line { start, .. } = &e.kind {
                starts.push(ctx.transform.apply_point(*start));
            }
        });
        assert_eq!(starts.len(), 4);
        // Spacing is in the rotated frame: column offset 10 along +Y, row offset 20 along -X.
        let has = |x: f64, y: f64| {
            starts
                .iter()
                .any(|p| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9)
        };
        assert!(has(0.0, 0.0) && has(0.0, 10.0) && has(-20.0, 0.0) && has(-20.0, 10.0));
    }

    #[test]
    fn walk_huge_array_is_bounded() {
        let blk = block("b", vec![line(0.0, 0.0, 1.0, 0.0)]);
        let mut ins = insert("b", 0.0, 0.0);
        if let EntityKind::Insert { columns, rows, .. } = &mut ins.kind {
            *columns = u32::MAX;
            *rows = u32::MAX;
        }
        let doc = doc_with(vec![ins], vec![blk]);
        let mut n = 0u64;
        doc.walk(0, 8, |_, _| n += 1);
        assert!(n <= DEFAULT_MAX_WORK);
    }

    #[test]
    fn walk_work_budget_counts_geometry_and_reports_truncation() {
        let big = Entity::new(EntityKind::Polyline {
            vertices: vec![Vertex::default(); 1000],
            closed: false,
            normal: Vec3::Z,
        });
        let doc = doc_with(vec![big.clone(), big.clone(), big], vec![]);
        let mut n = 0;
        let stats = doc.walk_with(
            0,
            &WalkOptions {
                max_work: 2_500,
                ..WalkOptions::default()
            },
            |_, _| n += 1,
        );
        assert_eq!(n, 2);
        assert!(stats.truncated);
        let stats = doc.walk_with(0, &WalkOptions::default(), |_, _| n += 1);
        assert!(!stats.truncated && stats.work == 3 * 1001);
        // The callback can stop the walk.
        let mut seen = 0;
        let stats = doc.walk_while(0, &WalkOptions::default(), |_, _| {
            seen += 1;
            false
        });
        assert_eq!(seen, 1);
        assert!(stats.truncated);
    }

    #[test]
    fn walk_groups_and_hidden_layers() {
        let mut hidden = line(0.0, 0.0, 9.0, 9.0);
        hidden.layer = Some("off".into());
        let group = Entity::new(EntityKind::Group {
            group_kind: crate::model::GroupKind::Cell,
            name: None,
            origin: None,
            children: vec![line(0.0, 0.0, 1.0, 1.0), hidden],
        });
        let mut doc = doc_with(vec![group], vec![]);
        doc.layers.push(Layer {
            name: "off".into(),
            visible: false,
            ..Layer::default()
        });
        let hits = collect(&doc, 8);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, 1);
        // frozen too
        doc.layers[0].visible = true;
        doc.layers[0].frozen = true;
        assert_eq!(collect(&doc, 8).len(), 1);
        doc.layers[0].frozen = false;
        assert_eq!(collect(&doc, 8).len(), 2);
    }

    #[test]
    fn walk_missing_model_and_block() {
        let doc = doc_with(vec![insert("nope", 0.0, 0.0)], vec![]);
        let mut n = 0;
        doc.walk(5, 8, |_, _| n += 1);
        assert_eq!(n, 0);
        doc.walk(0, 8, |_, _| n += 1);
        assert_eq!(n, 1);
    }

    #[test]
    fn dimension_block_expanded() {
        let blk = block("*D1", vec![line(0.0, 0.0, 3.0, 0.0)]);
        let dim = Entity::new(EntityKind::Dimension {
            dimension_type: crate::model::DimensionType::Linear,
            points: vec![Point3::xy(100.0, 100.0)],
            text_position: None,
            measurement: None,
            text: None,
            block: Some("*D1".into()),
            style: None,
        });
        let doc = doc_with(vec![dim], vec![blk]);
        let bb = doc.bbox(0).unwrap();
        assert_eq!(bb.max.x, 3.0);
    }

    #[test]
    fn bbox_of_shapes() {
        let circle = Entity::new(EntityKind::Circle {
            center: Point3::xy(5.0, 5.0),
            radius: 2.0,
            normal: Vec3::Z,
        });
        let poly = Entity::new(EntityKind::Polyline {
            vertices: vec![
                Vertex {
                    position: Point3::xy(0.0, 0.0),
                    bulge: 1.0,
                    ..Vertex::default()
                },
                Vertex::at(Point3::xy(2.0, 0.0)),
            ],
            closed: false,
            normal: Vec3::Z,
        });
        let doc = doc_with(vec![circle], vec![]);
        let bb = doc.bbox(0).unwrap();
        assert!((bb.min.x - 3.0).abs() < 1e-9 && (bb.max.y - 7.0).abs() < 1e-9);
        let doc = doc_with(vec![poly], vec![]);
        let bb = doc.bbox(0).unwrap();
        // Semicircle below the chord reaches y = -1.
        assert!((bb.min.y + 1.0).abs() < 1e-9 && (bb.max.x - 2.0).abs() < 1e-9);
        assert!(doc_with(vec![], vec![]).bbox(0).is_none());
        assert!(doc_with(vec![], vec![]).bbox(3).is_none());
    }

    #[test]
    fn flipped_normal_keeps_wcs_center_and_flips_angles() {
        let neg = Vec3::new(0.0, 0.0, -1.0);
        let circle = Entity::new(EntityKind::Circle {
            center: Point3::xy(5.0, 0.0),
            radius: 1.0,
            normal: neg,
        });
        let bb = doc_with(vec![circle], vec![]).bbox(0).unwrap();
        assert!((bb.min.x - 4.0).abs() < 1e-9 && (bb.max.x - 6.0).abs() < 1e-9);
        // Plane x axis is -X: an arc from 0 to 90 degrees runs from (c.x - r, c.y) to (c.x, c.y + r).
        let arc = Entity::new(EntityKind::Arc {
            center: Point3::xy(10.0, 10.0),
            radius: 2.0,
            start_angle: 0.0,
            end_angle: PI / 2.0,
            normal: neg,
        });
        let bb = doc_with(vec![arc], vec![]).bbox(0).unwrap();
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(
            near(bb.min.x, 8.0)
                && near(bb.max.x, 10.0)
                && near(bb.min.y, 10.0)
                && near(bb.max.y, 12.0),
            "{bb:?}"
        );
    }

    #[test]
    fn bulged_polyline_with_flipped_normal_bulges_the_other_way() {
        let mk = |normal: Vec3| {
            Entity::new(EntityKind::Polyline {
                vertices: vec![
                    Vertex {
                        position: Point3::xy(0.0, 0.0),
                        bulge: 1.0,
                        ..Vertex::default()
                    },
                    Vertex::at(Point3::xy(2.0, 0.0)),
                ],
                closed: false,
                normal,
            })
        };
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let up = doc_with(vec![mk(Vec3::Z)], vec![]).bbox(0).unwrap();
        assert!(near(up.min.y, -1.0) && near(up.max.y, 0.0));
        let down = doc_with(vec![mk(Vec3::new(0.0, 0.0, -1.0))], vec![])
            .bbox(0)
            .unwrap();
        assert!(near(down.min.y, 0.0) && near(down.max.y, 1.0), "{down:?}");
        assert!(near(down.min.x, 0.0) && near(down.max.x, 2.0));
    }

    #[test]
    fn bbox_with_can_ignore_images() {
        let poly = line(0.0, 0.0, 10.0, 10.0);
        let image = Entity::new(EntityKind::Image {
            path: None,
            position: Point3::xy(-50.0, -50.0),
            u_vector: Vec3::new(100.0, 0.0, 0.0),
            v_vector: Vec3::new(0.0, 100.0, 0.0),
            size_px: None,
        });
        let doc = doc_with(vec![poly, image.clone()], vec![]);
        assert!((doc.bbox(0).unwrap().max.x - 50.0).abs() < 1e-9);
        let bb = doc
            .bbox_with(
                0,
                &BBoxOptions {
                    include_images: false,
                    ..BBoxOptions::default()
                },
            )
            .unwrap();
        assert!((bb.max.x - 10.0).abs() < 1e-9);
        let only = doc_with(vec![image], vec![]);
        assert!(
            only.bbox_with(
                0,
                &BBoxOptions {
                    include_images: false,
                    ..BBoxOptions::default()
                }
            )
            .is_none()
        );
    }

    #[test]
    fn aligned_text_bbox_covers_baseline() {
        let t = Entity::new(EntityKind::Text {
            position: Point3::xy(0.0, 0.0),
            end_point: Some(Point3::xy(10.0, 0.0)),
            height: 2.0,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            value: "x".into(),
            style: None,
            halign: HAlign::Aligned,
            valign: VAlign::Baseline,
            normal: Vec3::Z,
        });
        let bb = doc_with(vec![t], vec![]).bbox(0).unwrap();
        assert!(
            (bb.max.x - 10.0).abs() < 1e-9
                && (bb.max.y - 2.0).abs() < 1e-9
                && bb.min.y.abs() < 1e-9
        );
    }

    #[test]
    fn insert_with_flipped_normal_uses_wcs_position() {
        let blk = block("b", vec![line(0.0, 0.0, 1.0, 0.0)]);
        let mut ins = insert("b", 10.0, 3.0);
        if let EntityKind::Insert { normal, .. } = &mut ins.kind {
            *normal = Vec3::new(0.0, 0.0, -1.0);
        }
        let doc = doc_with(vec![ins], vec![blk]);
        let mut pts = Vec::new();
        doc.walk(0, 8, |e, ctx| {
            if let EntityKind::Line { start, end } = &e.kind {
                pts.push((
                    ctx.transform.apply_point(*start),
                    ctx.transform.apply_point(*end),
                ));
            }
        });
        assert_eq!(pts.len(), 1);
        let (s, e) = pts[0];
        assert!((s.x - 10.0).abs() < 1e-9 && (s.y - 3.0).abs() < 1e-9);
        assert!((e.x - 9.0).abs() < 1e-9 && (e.y - 3.0).abs() < 1e-9);
    }

    #[test]
    fn colors() {
        let mut doc = doc_with(vec![], vec![]);
        doc.layers.push(Layer {
            name: "L".into(),
            color: Color::Aci { index: 1 },
            ..Layer::default()
        });
        doc.layers.push(Layer {
            name: "0".into(),
            color: Color::Rgb { r: 1, g: 2, b: 3 },
            ..Layer::default()
        });
        let mut e = Entity::new(EntityKind::Point {
            position: Point3::default(),
        });
        assert_eq!(e.resolved_color(&doc), (1, 2, 3));
        e.layer = Some("L".into());
        assert_eq!(e.resolved_color(&doc), (255, 0, 0));
        e.layer = Some("missing".into());
        assert_eq!(e.resolved_color(&doc), (0, 0, 0));
        e.color = Color::Aci { index: 5 };
        assert_eq!(e.resolved_color(&doc), (0, 0, 255));
        e.color = Color::Aci { index: 7 };
        assert_eq!(e.resolved_color(&doc), (0, 0, 0));
        e.color = Color::Rgb { r: 9, g: 8, b: 7 };
        assert_eq!(e.resolved_color(&doc), (9, 8, 7));
        e.color = Color::ByBlock;
        assert_eq!(e.resolved_color(&doc), (0, 0, 0));
    }

    #[test]
    fn byblock_and_layer_inheritance_through_insert() {
        let mut inner = line(0.0, 0.0, 1.0, 0.0);
        inner.color = Color::ByBlock;
        inner.layer = Some("0".into());
        let blk = block("b", vec![inner]);
        let mut ins = insert("b", 0.0, 0.0);
        ins.color = Color::Aci { index: 3 };
        ins.layer = Some("walls".into());
        let mut doc = doc_with(vec![ins], vec![blk]);
        doc.layers.push(Layer {
            name: "walls".into(),
            visible: true,
            ..Layer::default()
        });
        let mut seen = None;
        doc.walk(0, 8, |e, ctx| {
            seen = Some((e.resolved_color_in(&doc, ctx), ctx.layer_name(e).to_owned()));
        });
        assert_eq!(seen, Some(((0, 255, 0), "walls".to_owned())));
    }

    #[test]
    fn byblock_chain_uses_layer_of_insert() {
        let mut inner = line(0.0, 0.0, 1.0, 0.0);
        inner.color = Color::ByBlock;
        let blk = block("b", vec![inner]);
        let ins = insert("b", 0.0, 0.0); // ByLayer on layer "0"
        let mut doc = doc_with(vec![ins], vec![blk]);
        doc.layers.push(Layer {
            name: "0".into(),
            color: Color::Aci { index: 1 },
            visible: true,
            ..Layer::default()
        });
        let mut seen = None;
        doc.walk(0, 8, |e, ctx| seen = Some(e.resolved_color_in(&doc, ctx)));
        assert_eq!(seen, Some((255, 0, 0)));
    }

    #[test]
    fn type_names() {
        assert_eq!(line(0.0, 0.0, 1.0, 1.0).kind.type_name(), "line");
        assert_eq!(insert("a", 0.0, 0.0).kind.type_name(), "insert");
    }
}
