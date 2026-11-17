//! [`line_buffer`]: every curve of a model as flat line segments for a canvas or line
//! renderer, in the buffers of [`LineBuffer`].

use cadkit_core::Document;
use cadkit_core::ops::WalkOptions;

use super::collect::{self, Encoder, Ring, Segments, Settings, extend_bounds};
use super::surfaces::{check, range, relative};
use crate::SceneError;
use crate::buffers::{LineBuffer, LineReport};

/// Options of [`line_buffer`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LineOptions {
    /// Index into `Document::models`.
    pub model: usize,
    /// Traversal depth and work budget of the walk.
    pub walk: WalkOptions,
    /// Store x, y and z per end point; false (default) stores x and y (the XY projection).
    pub three_d: bool,
    /// Largest chordal error of flattened curves, in drawing units; `None` derives it from
    /// `Document::units` (0.1 mm, or 1e-4 drawing units when unitless).
    pub tolerance: Option<f64>,
    /// Fail with [`SceneError::Precision`] when a stored f32 coordinate is further than this
    /// from the drawing coordinate (drawing units). `None` (default) accepts any finite error.
    pub max_error: Option<f64>,
    /// Background the segments will be drawn on; colors too close to it become black or white
    /// as in the SVG export (`cadkit_core::svg::with_contrast`). `None` keeps colors as
    /// resolved. Default white.
    pub background: Option<[u8; 3]>,
    /// Most segments. Default 2,000,000; at most 2^28.
    pub max_segments: usize,
}

impl Default for LineOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl LineOptions {
    /// Model 0 in 2D with the defaults listed on each field.
    pub fn new() -> Self {
        Self {
            model: 0,
            walk: WalkOptions::new(),
            three_d: false,
            tolerance: None,
            max_error: None,
            background: Some([255; 3]),
            max_segments: 2_000_000,
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

    /// Sets [`Self::three_d`].
    #[must_use]
    pub fn with_three_d(mut self, three_d: bool) -> Self {
        self.three_d = three_d;
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

    /// Sets [`Self::background`].
    #[must_use]
    pub fn with_background(mut self, background: Option<[u8; 3]>) -> Self {
        self.background = background;
        self
    }

    /// Sets [`Self::max_segments`].
    #[must_use]
    pub fn with_max_segments(mut self, max_segments: usize) -> Self {
        self.max_segments = max_segments;
        self
    }
}

/// The curves of `options.model` as line segments.
///
/// The traversal, colors and curve flattening are those of [`super::plan_document`]: blocks
/// and arrays expanded with their placement, colors resolved like the SVG export, circles,
/// arcs, ellipses, splines and bulged polylines flattened within the tolerance. Polygons,
/// faces and mesh faces are drawn as their outlines. Text, hatches (fill and boundary),
/// points, images, viewports, unexpanded block references and unknown records are counted
/// in [`LineReport::skipped`]; linetype dash patterns are drawn solid.
///
/// Segments are grouped by layer ([`LineBuffer::layers`]) and stored as f32 offsets from
/// [`LineBuffer::origin`], the per-axis median of the model's points (see
/// [`super::surface_mesh`] for the precision rules). When the segment budget runs out
/// the rest of the model (in walk order) is left out and [`LineReport::truncated`] is set.
///
/// # Errors
///
/// [`SceneError::InvalidBudget`] for an option out of range; [`SceneError::Precision`] when a
/// coordinate exceeds [`LineOptions::max_error`] (`vertex` is the index of the end point).
pub fn line_buffer(doc: &Document, options: &LineOptions) -> Result<LineBuffer, SceneError> {
    check(options.tolerance, options.max_error, options.max_segments)?;
    let settings = Settings {
        model: options.model,
        walk: options.walk,
        faces: false,
        polyline_faces: false,
        planarity: 0.,
        wires: true,
        tolerance: options
            .tolerance
            .unwrap_or_else(|| super::default_tolerance(doc)),
        neutral: None,
        background: options.background,
        max_points: options.max_segments.saturating_mul(2).saturating_add(64),
    };
    let mut work = 0;
    let collected = collect::collect(doc, &settings, &mut work);
    let mut report = LineReport {
        flattened: collected.flattened,
        degenerate: collected.degenerate,
        skipped: collected.skipped.clone(),
        truncated: collected.truncated,
        ..LineReport::default()
    };
    let mut origin = collected.median().unwrap_or_default();
    if !options.three_d {
        origin[2] = 0.;
    }
    let mut encoder = Encoder {
        origin,
        components: if options.three_d { 3 } else { 2 },
        limit: options.max_error,
        max_error: 0.,
    };
    // The budget keeps a prefix of the walk, whatever the layers.
    let mut used = 0usize;
    let mut fits = vec![false; collected.items.len()];
    for (fit, item) in fits.iter_mut().zip(&collected.items) {
        let n = collected
            .rings_of(item)
            .iter()
            .map(Ring::segments)
            .fold(0usize, usize::saturating_add);
        match used.checked_add(n) {
            Some(total) if total <= options.max_segments => {
                used = total;
                *fit = true;
            }
            _ => {
                report.truncated = true;
                break;
            }
        }
    }
    let mut buffer = LineBuffer {
        origin,
        components: if options.three_d { 3 } else { 2 },
        ..LineBuffer::default()
    };
    // The box covers what was drawn, not what the budget left out.
    let mut bounds = None;
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); collected.layers.len()];
    for (index, item) in collected.items.iter().enumerate() {
        if fits.get(index) == Some(&true)
            && let Some(bucket) = buckets.get_mut(item.layer as usize)
        {
            bucket.push(index);
        }
    }
    let mut segments = Segments::default();
    for (bucket, name) in buckets.iter().zip(&collected.layers) {
        let first = segments.items.len();
        for item in bucket.iter().filter_map(|&k| collected.items.get(k)) {
            let index = u32::try_from(buffer.items.len()).unwrap_or(u32::MAX);
            let mark = segments.mark();
            if let Err(error) = segments.push(
                &collected,
                collected.rings_of(item),
                item.color,
                index,
                &mut encoder,
            ) {
                // Only coordinates beyond f32 range get here without an error budget.
                if options.max_error.is_some() {
                    return Err(error);
                }
                segments.rollback(mark);
                report.degenerate += 1;
                continue;
            }
            for ring in collected.rings_of(item) {
                extend_bounds(&mut bounds, collected.points_of(ring));
            }
            collect::push_item(&mut buffer.items, &collected, item);
            report.drawn += 1;
        }
        buffer
            .layers
            .push(range(name, 0, 0, first, segments.items.len()));
    }
    if let Some((mut lo, mut hi)) = bounds {
        if !options.three_d {
            lo[2] = 0.;
            hi[2] = 0.;
        }
        buffer.min = relative(lo, origin);
        buffer.max = relative(hi, origin);
    }
    report.segments = segments.items.len();
    buffer.segments = segments.positions;
    buffer.segment_colors = segments.colors;
    buffer.segment_items = segments.items;
    buffer.max_error = encoder.max_error;
    buffer.report = report;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadkit_core::{Block, Color, Entity, EntityKind, Layer, LengthUnit, Point3, Units, Vec3};

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    fn doc() -> Document {
        let mut doc = Document::new();
        doc.units = Units::new(LengthUnit::Meter);
        doc.layers.push(Layer::new("0"));
        doc.layers
            .push(Layer::new("walls").with_color(Color::rgb(200, 10, 10)));
        doc.blocks.push(Block::new("door").with_entities(vec![
            Entity::arc(p(0., 0., 0.), 1., 0., std::f64::consts::FRAC_PI_2),
            Entity::line(p(0., 0., 0.), p(1., 0., 0.)),
        ]));
        let model = doc.model_space_mut();
        model
            .entities
            .push(Entity::line(p(0., 0., 0.), p(5., 0., 3.)).on_layer("walls"));
        let mut white = Entity::circle(p(2., 2., 0.), 0.5);
        white.color = Color::rgb(255, 255, 255);
        model.entities.push(white);
        model
            .entities
            .push(Entity::text(p(1., 1., 0.), 0.2, "label"));
        model.entities.push(Entity::new(EntityKind::Hatch {
            loops: vec![],
            solid: true,
            pattern: None,
            pattern_scale: 1.,
            pattern_angle: 0.,
            normal: Vec3::Z,
        }));
        model.entities.push(Entity::new(EntityKind::Polygon {
            exterior: vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 0., 0.)],
            interiors: vec![],
        }));
        let mut door = Entity::insert("door", p(10., 0., 0.)).on_layer("walls");
        if let EntityKind::Insert { scale, .. } = &mut door.kind {
            *scale = Vec3::new(2., 2., 1.);
        }
        model.entities.push(door);
        doc
    }

    #[test]
    fn draws_curves_as_segments_and_reports_the_rest() {
        let doc = doc();
        let lines = line_buffer(&doc, &LineOptions::new()).unwrap();
        let r = &lines.report;
        // Wall, circle, polygon outline, door arc and door line.
        assert_eq!(r.drawn, 5);
        assert_eq!(
            (r.skipped.get("text"), r.skipped.get("hatch")),
            (Some(&1), Some(&1))
        );
        assert_eq!(r.flattened, 2);
        assert!(!r.truncated);
        assert_eq!(lines.components, 2);
        assert_eq!(lines.origin[2], 0.);
        assert_eq!(lines.segments.len(), r.segments * 4);
        assert_eq!(lines.segment_colors.len(), r.segments * 6);
        assert_eq!(lines.segment_items.len(), r.segments);
        // Layers in table order, contiguous, and every segment's item on its layer.
        assert_eq!(
            lines
                .layers
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>(),
            ["0", "walls"]
        );
        let mut next = 0;
        for (index, layer) in lines.layers.iter().enumerate() {
            assert_eq!(layer.first_segment, next);
            next += layer.segment_count;
            for s in layer.first_segment..layer.first_segment + layer.segment_count {
                let item = lines.items.items[lines.segment_items[s as usize] as usize];
                assert_eq!(item.layer as usize, index);
            }
        }
        assert_eq!(next as usize, r.segments);
        // The wall: first segment of "walls", red, ends in XY.
        let wall = lines.layers[1].first_segment as usize;
        let item = lines.segment_items[wall] as usize;
        assert_eq!(lines.items.path(item), Some(&[0][..]));
        assert_eq!(lines.segment_colors[wall * 6..wall * 6 + 3], [200, 10, 10]);
        let end = [0, 1].map(|k| lines.origin[k] + f64::from(lines.segments[wall * 4 + 2 + k]));
        assert!((end[0] - 5.).abs() < 1e-5 && end[1].abs() < 1e-5, "{end:?}");
        // White on the default white background becomes black, as in the SVG export.
        let circle = lines.layers[0].first_segment as usize;
        assert_eq!(
            lines.items.path(lines.segment_items[circle] as usize),
            Some(&[1][..])
        );
        assert_eq!(lines.segment_colors[circle * 6..circle * 6 + 3], [0, 0, 0]);
        // The door arc comes from the block, under the reference's path.
        let arcs: Vec<usize> = (0..lines.items.len())
            .filter(|&i| lines.items.items[i].kind == "arc")
            .collect();
        assert_eq!(arcs.len(), 1);
        assert_eq!(lines.items.path(arcs[0]), Some(&[5, 0][..]));
        assert_eq!(lines.items.items[arcs[0]].model_path_len, 1);
        let entity = doc
            .entity_at(0, &lines.items.path_indices(arcs[0]).unwrap())
            .unwrap();
        assert!(matches!(entity.kind, EntityKind::Arc { .. }));
        // Flattened within 0.1 mm: every arc point lies on the scaled circle of radius 2.
        for s in 0..lines.report.segments {
            if lines.segment_items[s] as usize == arcs[0] {
                let x = lines.origin[0] + f64::from(lines.segments[s * 4]) - 10.;
                let y = lines.origin[1] + f64::from(lines.segments[s * 4 + 1]);
                assert!((x.hypot(y) - 2.).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn three_d_and_budgets() {
        let doc = doc();
        let flat = line_buffer(&doc, &LineOptions::new()).unwrap();
        let deep = line_buffer(&doc, &LineOptions::new().with_three_d(true)).unwrap();
        assert_eq!(deep.components, 3);
        assert_eq!(deep.report.segments, flat.report.segments);
        let wall = deep.layers[1].first_segment as usize;
        let end = [0, 1, 2].map(|k| deep.origin[k] + f64::from(deep.segments[wall * 6 + 3 + k]));
        for (value, expected) in end.into_iter().zip([5., 0., 3.]) {
            assert!((value - expected).abs() < 1e-5, "{end:?}");
        }
        let short = line_buffer(&doc, &LineOptions::new().with_max_segments(5)).unwrap();
        assert!(short.report.truncated);
        // A prefix in walk order: the wall alone (the circle has more segments than fit).
        assert_eq!((short.report.drawn, short.report.segments), (1, 1));
        let walk = LineOptions::new().with_walk(WalkOptions::new().with_max_work(2));
        assert!(line_buffer(&doc, &walk).unwrap().report.truncated);
        assert!(line_buffer(&doc, &LineOptions::new().with_tolerance(-1.)).is_err());
        let strict = LineOptions::new().with_max_error(Some(1e-12));
        let mut far = Document::new();
        far.model_space_mut()
            .entities
            .push(Entity::line(p(500_000.1, 0., 0.), p(500_010.3, 0., 0.)));
        assert!(matches!(
            line_buffer(&far, &strict),
            Err(SceneError::Precision { .. })
        ));
        let missing = line_buffer(&doc, &LineOptions::new().with_model(3)).unwrap();
        assert_eq!(missing.report.segments, 0);
        assert!(missing.layers.iter().all(|l| l.segment_count == 0));
    }
}
