//! Bridge from a [`cadkit_core::Document`] model to [`PlanItem`]s (feature `document`).
//!
//! Every visible entity of one model is visited with blocks and arrays expanded and their
//! placement applied, then projected onto the XY plane. Lines, arcs and circles that stay
//! circular under the placement become analytic curves; curved entities that do not
//! (ellipses, splines, bulged polylines, tilted or non-uniformly scaled arcs) are flattened
//! into polylines within a chordal tolerance. Colors are resolved like the SVG export;
//! widths follow the lineweight. What this bridge does not draw is counted in
//! [`DocumentPlanReport`], never dropped silently: text, hatches and fills, points, images,
//! viewports, unknown records and linetype dash patterns (drawn solid).

use std::collections::BTreeMap;

use cadkit_core::ops::WalkOptions;
use cadkit_core::{Document, Entity, EntityKind, LengthUnit, Lineweight, Point3, Transform, Vec3};
use cadkit_geom::{Arc, Circle, Curve, LineSegment, Point2, Polyline};

use crate::{GroupId, ObjectId, PlanItem};

/// Options of [`plan_document`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct DocumentPlanOptions {
    /// Index into [`Document::models`].
    pub model: usize,
    /// Traversal depth and work budget.
    pub walk: WalkOptions,
    /// Stroke pixels per millimetre of lineweight. Default 96 / 25.4 (96 DPI).
    pub pixels_per_mm: f32,
    /// Narrowest stroke, also used for `ByLayer` / `ByBlock` / `Default` weights that resolve
    /// to nothing. Default 1 px.
    pub min_width_px: f32,
    /// Largest chordal error when flattening curves, in drawing units; `None` derives it from
    /// [`Document::units`] (0.1 mm, or 1e-4 drawing units when unitless).
    pub tolerance: Option<f64>,
    /// Background the strokes will be drawn on; colors too close to it become black or white
    /// as in the SVG export ([`cadkit_core::svg::with_contrast`]). `None` keeps colors as
    /// resolved. Default white.
    pub background: Option<[u8; 3]>,
}

impl Default for DocumentPlanOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentPlanOptions {
    /// Model 0, default walk, 96 DPI lineweights, 1 px minimum width.
    pub fn new() -> Self {
        Self {
            model: 0,
            walk: WalkOptions::new(),
            pixels_per_mm: 96. / 25.4,
            min_width_px: 1.,
            tolerance: None,
            background: Some([255; 3]),
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

    /// Sets [`Self::pixels_per_mm`].
    #[must_use]
    pub fn with_pixels_per_mm(mut self, pixels_per_mm: f32) -> Self {
        self.pixels_per_mm = pixels_per_mm;
        self
    }

    /// Sets [`Self::min_width_px`].
    #[must_use]
    pub fn with_min_width_px(mut self, min_width_px: f32) -> Self {
        self.min_width_px = min_width_px;
        self
    }

    /// Sets [`Self::background`].
    #[must_use]
    pub fn with_background(mut self, background: Option<[u8; 3]>) -> Self {
        self.background = background;
        self
    }

    /// Sets [`Self::tolerance`].
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = Some(tolerance);
        self
    }
}

/// What [`plan_document`] did not draw as stored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct DocumentPlanReport {
    /// Entities turned into items.
    pub drawn: usize,
    /// Of those, entities whose curves were flattened into polylines.
    pub flattened: usize,
    /// Of those, entities with a dashed linetype drawn as solid strokes.
    pub solid_linetypes: usize,
    /// Entities not drawn, by [`EntityKind::type_name`]; degenerate geometry counts under its
    /// own kind.
    pub skipped: BTreeMap<&'static str, usize>,
    /// The walk's work budget ran out before the model was finished.
    pub truncated: bool,
}

/// Items of one model, in drawing order, with their groups.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct DocumentPlan {
    /// `(ObjectId(n), item)` for the n-th drawn entity, in walk order.
    pub items: Vec<(ObjectId, PlanItem)>,
    /// Layer name of each group: `groups[g]` is the layer of `GroupId(g)`. Groups follow
    /// [`Document::layers`]; entities on a layer missing from the table get extra groups.
    pub groups: Vec<String>,
    /// What was not drawn as stored.
    pub report: DocumentPlanReport,
}

/// Converts the visible entities of `options.model` into plan items; see the
/// [module documentation](self).
pub fn plan_document(doc: &Document, options: &DocumentPlanOptions) -> DocumentPlan {
    let tolerance = options.tolerance.unwrap_or_else(|| default_tolerance(doc));
    let mut groups: Vec<String> = doc.layers.iter().map(|l| l.name.clone()).collect();
    let mut group_of: BTreeMap<String, u32> = BTreeMap::new();
    for (index, name) in groups.iter().enumerate() {
        group_of
            .entry(name.clone())
            .or_insert(u32::try_from(index).unwrap_or(u32::MAX));
    }
    let mut plan = DocumentPlan::default();
    let stats = doc.walk_with(options.model, &options.walk, |entity, ctx| {
        let kind = entity.kind.type_name();
        let Some((curves, flattened)) = curves(entity, &ctx.transform, tolerance) else {
            *plan.report.skipped.entry(kind).or_default() += 1;
            return;
        };
        let layer = ctx.layer_name(entity);
        let group = match group_of.get(layer) {
            Some(group) => *group,
            None => {
                let group = u32::try_from(groups.len()).unwrap_or(u32::MAX);
                groups.push(layer.to_owned());
                group_of.insert(layer.to_owned(), group);
                group
            }
        };
        let mut color = entity.resolved_color_in(doc, ctx);
        if let Some([r, g, b]) = options.background {
            color = cadkit_core::svg::with_contrast(color, (r, g, b));
        }
        let (r, g, b) = color;
        let width = width_px(doc, entity, layer, options);
        if dashed(doc, entity, layer) {
            plan.report.solid_linetypes += 1;
        }
        if flattened {
            plan.report.flattened += 1;
        }
        let id = ObjectId(plan.items.len() as u64);
        plan.items.push((
            id,
            PlanItem::new(GroupId(group), curves, [r, g, b, 255], width),
        ));
        plan.report.drawn += 1;
    });
    plan.report.truncated = stats.truncated;
    plan.groups = groups;
    plan
}

/// 0.1 mm in drawing units, or 1e-4 units when the scale is unknown.
fn default_tolerance(doc: &Document) -> f64 {
    let meters = doc.units.meters_per_unit.or(match doc.units.unit {
        LengthUnit::Millimeter => Some(1e-3),
        LengthUnit::Centimeter => Some(1e-2),
        LengthUnit::Meter => Some(1.),
        LengthUnit::Inch => Some(0.0254),
        LengthUnit::Foot => Some(0.3048),
        _ => None,
    });
    match meters {
        Some(m) if m.is_finite() && m > 0. => 1e-4 / m,
        _ => 1e-4,
    }
}

fn width_px(doc: &Document, entity: &Entity, layer: &str, options: &DocumentPlanOptions) -> f32 {
    let weight = match entity.lineweight {
        Lineweight::ByLayer => doc
            .layer(layer)
            .map_or(Lineweight::Default, |l| l.lineweight),
        other => other,
    };
    match weight {
        Lineweight::Millimeters(mm) if mm.is_finite() && mm > 0. => {
            (mm as f32 * options.pixels_per_mm).max(options.min_width_px)
        }
        _ => options.min_width_px,
    }
}

/// Whether the effective linetype has a dash pattern.
fn dashed(doc: &Document, entity: &Entity, layer: &str) -> bool {
    let name = match entity.linetype.as_deref() {
        None => doc.layer(layer).and_then(|l| l.linetype.as_deref()),
        Some(name) if name.eq_ignore_ascii_case("bylayer") => {
            doc.layer(layer).and_then(|l| l.linetype.as_deref())
        }
        Some(name) => Some(name),
    };
    name.is_some_and(|name| {
        doc.linetypes
            .iter()
            .any(|lt| lt.name.eq_ignore_ascii_case(name) && lt.pattern.len() > 1)
    })
}

/// Curves of `entity` placed by `transform`, and whether they were flattened; `None` for
/// kinds this bridge does not draw and for degenerate geometry.
fn curves(entity: &Entity, transform: &Transform, tolerance: f64) -> Option<(Vec<Curve>, bool)> {
    let place = |p: Point3| {
        let q = transform.apply_point(p);
        Point2::new(q.x, q.y)
    };
    match &entity.kind {
        EntityKind::Line { start, end } => {
            let (start, end) = (place(*start), place(*end));
            (finite(start) && finite(end) && start != end)
                .then(|| (vec![Curve::Line(LineSegment { start, end })], false))
        }
        EntityKind::Circle {
            center,
            radius,
            normal,
        } => match similarity(transform, *normal) {
            Some(scale) => {
                let (center, radius) = (place(*center), radius * scale);
                (finite(center) && radius.is_finite() && radius > 0.)
                    .then(|| (vec![Curve::Circle(Circle { center, radius })], false))
            }
            None => flattened(entity, transform, tolerance),
        },
        EntityKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            normal,
        } => match similarity(transform, *normal) {
            Some(scale) => {
                let point = |angle: f64| {
                    place(Point3::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                        center.z,
                    ))
                };
                let c = place(*center);
                let from = point(*start_angle);
                let mut sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
                if sweep == 0. {
                    sweep = std::f64::consts::TAU;
                }
                // A mirroring placement turns counter-clockwise arcs clockwise.
                let sign = if determinant(transform) < 0. { -1. } else { 1. };
                let radius = radius * scale;
                (finite(c) && finite(from) && radius.is_finite() && radius > 0.).then(|| {
                    let arc = Arc {
                        center: c,
                        radius,
                        start_angle: (from.y - c.y).atan2(from.x - c.x),
                        sweep_angle: sign * sweep,
                    };
                    (vec![Curve::Arc(arc)], false)
                })
            }
            None => flattened(entity, transform, tolerance),
        },
        EntityKind::Polyline {
            vertices, closed, ..
        } => {
            if vertices.iter().any(|v| v.bulge != 0.) {
                return flattened(entity, transform, tolerance);
            }
            polyline(vertices.iter().map(|v| place(v.position)), *closed)
                .map(|curve| (vec![curve], false))
        }
        EntityKind::Ellipse { .. } | EntityKind::Spline { .. } => {
            flattened(entity, transform, tolerance)
        }
        EntityKind::Face { points, .. } => {
            polyline(points.iter().map(|p| place(*p)), true).map(|curve| (vec![curve], false))
        }
        EntityKind::Leader { vertices, .. } => {
            polyline(vertices.iter().map(|p| place(*p)), false).map(|curve| (vec![curve], false))
        }
        EntityKind::Polygon {
            exterior,
            interiors,
        } => {
            let rings: Vec<Curve> = std::iter::once(exterior)
                .chain(interiors)
                .filter_map(|ring| polyline(ring.iter().map(|p| place(*p)), true))
                .collect();
            (!rings.is_empty()).then_some((rings, false))
        }
        _ => None,
    }
}

fn finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// A polyline without repeated consecutive points (including the closing one); `None` when
/// fewer than two distinct points remain.
fn polyline(points: impl Iterator<Item = Point2>, closed: bool) -> Option<Curve> {
    let mut out: Vec<Point2> = Vec::new();
    for point in points {
        if !finite(point) {
            return None;
        }
        if out.last() != Some(&point) {
            out.push(point);
        }
    }
    if closed && out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    (out.len() >= 2).then_some(Curve::Polyline(Polyline {
        points: out,
        closed,
    }))
}

/// The uniform XY scale of `transform` for a planar entity with `normal`, when circles stay
/// circles in the XY projection: `normal` along +Z and the placement a similarity in XY.
fn similarity(transform: &Transform, normal: Vec3) -> Option<f64> {
    let length = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
    // NaN fails both comparisons and is rejected with them.
    if length.is_nan() || length <= 0. || normal.z / length < 1. - 1e-12 {
        return None;
    }
    let (x, y) = (transform.x, transform.y);
    let (lx, ly) = (x.x.hypot(x.y), y.x.hypot(y.y));
    let tight = |v: f64, scale: f64| v.abs() <= 1e-9 * scale;
    (lx.is_finite()
        && lx > 0.
        && tight(lx - ly, lx)
        && tight(x.x * y.x + x.y * y.y, lx * lx)
        && tight(x.z, lx)
        && tight(y.z, lx))
    .then_some(lx)
}

fn determinant(transform: &Transform) -> f64 {
    transform.x.x * transform.y.y - transform.x.y * transform.y.x
}

/// Flattens one entity with [`Document::tessellate_curves`] and places the polyline.
fn flattened(entity: &Entity, transform: &Transform, tolerance: f64) -> Option<(Vec<Curve>, bool)> {
    let mut doc = Document::new();
    doc.model_space_mut().entities.push(entity.clone());
    doc.tessellate_curves(tolerance);
    let flat = doc.models.first()?.entities.first()?;
    let EntityKind::Polyline {
        vertices, closed, ..
    } = &flat.kind
    else {
        return None;
    };
    let place = |p: Point3| {
        let q = transform.apply_point(p);
        Point2::new(q.x, q.y)
    };
    polyline(vertices.iter().map(|v| place(v.position)), *closed).map(|curve| (vec![curve], true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadkit_core::{Block, Color, Layer, Linetype};

    fn doc() -> Document {
        let mut doc = Document::new();
        doc.units = cadkit_core::Units::new(LengthUnit::Meter);
        doc.layers.push(Layer::new("0"));
        doc.layers.push(
            Layer::new("walls")
                .with_color(Color::rgb(200, 10, 10))
                .with_lineweight(Lineweight::Millimeters(0.5))
                .with_linetype("dashed"),
        );
        doc.linetypes
            .push(Linetype::new("dashed", vec![0.5, -0.25]));
        doc.blocks.push(Block::new("door").with_entities(vec![
            Entity::arc(Point3::xy(0., 0.), 1., 0., std::f64::consts::FRAC_PI_2),
            Entity::line(Point3::xy(0., 0.), Point3::xy(1., 0.)),
        ]));
        let model = doc.model_space_mut();
        model
            .entities
            .push(Entity::line(Point3::xy(0., 0.), Point3::xy(5., 0.)).on_layer("walls"));
        model.entities.push(Entity::circle(Point3::xy(2., 2.), 0.5));
        model
            .entities
            .push(Entity::text(Point3::xy(1., 1.), 0.2, "label"));
        model
            .entities
            .push(Entity::line(Point3::xy(3., 3.), Point3::xy(3., 3.)));
        let mut mirrored = Entity::insert("door", Point3::xy(10., 0.));
        if let EntityKind::Insert { scale, .. } = &mut mirrored.kind {
            *scale = Vec3::new(-2., 2., 1.);
        }
        model.entities.push(mirrored);
        let mut stretched = Entity::insert("door", Point3::xy(20., 0.));
        if let EntityKind::Insert { scale, .. } = &mut stretched.kind {
            *scale = Vec3::new(3., 1., 1.);
        }
        model.entities.push(stretched);
        doc
    }

    #[test]
    fn plans_lines_arcs_and_blocks_and_reports_the_rest() {
        let plan = plan_document(&doc(), &DocumentPlanOptions::new());
        let report = &plan.report;
        // Wall, circle, and two inserts with an arc and a line each.
        assert_eq!(report.drawn, 6);
        assert_eq!(report.skipped.get("text"), Some(&1));
        assert_eq!(report.skipped.get("line"), Some(&1));
        assert_eq!(report.solid_linetypes, 1);
        // The non-uniformly scaled door arc is flattened, the mirrored one stays analytic.
        assert_eq!(report.flattened, 1);
        // Black on the default white background stays black; white would flip.
        let (_, circle) = &plan.items[1];
        assert_eq!(circle.color, [0, 0, 0, 255]);
        let mut white = doc();
        white.model_space_mut().entities[1].color = Color::rgb(255, 255, 255);
        let flipped = plan_document(&white, &DocumentPlanOptions::new());
        assert_eq!(flipped.items[1].1.color, [0, 0, 0, 255]);
        let kept = plan_document(&white, &DocumentPlanOptions::new().with_background(None));
        assert_eq!(kept.items[1].1.color, [255, 255, 255, 255]);
        assert!(!report.truncated);
        let (_, wall) = &plan.items[0];
        assert_eq!(wall.color, [200, 10, 10, 255]);
        assert!((wall.width_px - 0.5 * 96. / 25.4).abs() < 1e-6);
        assert_eq!(plan.groups[wall.group.0 as usize], "walls");
        let (_, mirrored) = &plan.items[2];
        let Curve::Arc(arc) = &mirrored.curves[0] else {
            panic!("{:?}", mirrored.curves);
        };
        // A quarter arc from +X to +Y, mirrored in X and scaled by 2: from -X to +Y, clockwise.
        assert_eq!(arc.center, Point2::new(10., 0.));
        assert!((arc.radius - 2.).abs() < 1e-12);
        assert!((arc.start_angle - std::f64::consts::PI).abs() < 1e-12);
        assert!((arc.sweep_angle + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let (_, stretched) = &plan.items[4];
        assert!(matches!(&stretched.curves[0], Curve::Polyline(p) if p.points.len() > 4));
    }

    #[test]
    fn hostile_documents_do_not_panic() {
        let mut doc = Document::new();
        let model = doc.model_space_mut();
        model
            .entities
            .push(Entity::circle(Point3::xy(f64::NAN, 0.), 1.));
        model.entities.push(Entity::circle(Point3::xy(0., 0.), -1.));
        model
            .entities
            .push(Entity::arc(Point3::xy(0., 0.), f64::INFINITY, 0., 1.));
        model
            .entities
            .push(Entity::polyline([Point3::xy(1., 1.); 3], true));
        let plan = plan_document(&doc, &DocumentPlanOptions::new().with_tolerance(1e-300));
        assert!(plan.items.iter().all(|(_, item)| !item.curves.is_empty()));
        assert_eq!(plan.report.skipped.get("polyline"), Some(&1));
        assert_eq!(plan.report.skipped.get("circle"), Some(&2));
        assert_eq!(plan.report.skipped.get("arc"), Some(&1));
        let missing = plan_document(&doc, &DocumentPlanOptions::new().with_model(9));
        assert_eq!(missing.report.drawn, 0);
    }
}
