//! 2D SVG rendering of a [`Document`] (top view, XY plane).
//!
//! Conventions and limits:
//! - The view looks down the Z axis; Y is flipped so the drawing is upright. Z is ignored.
//! - The `viewBox` is the model bounding box ([`Document::bbox`]) plus a 2 % margin, in
//!   drawing units; `width` comes from [`SvgOptions::width_px`] and `height` follows the
//!   aspect ratio.
//! - Every element carries `vector-effect="non-scaling-stroke"`, so [`SvgOptions::stroke_px`]
//!   is a screen width regardless of the zoom. Lineweights, linetypes (dashes) and
//!   polyline widths are ignored.
//! - One `<g data-layer="...">` per layer, in order of first use. Hidden / frozen layers and
//!   invisible entities are skipped; block references are expanded (see [`Document::walk`]),
//!   with `ByBlock` colors and layer `"0"` resolved through the inserting entity.
//! - All points are world coordinates. Circles, arcs, text and hatch arcs are built in the
//!   plane through their WCS point spanned by `arbitrary_axes(normal)`; angles run
//!   counter-clockwise about the normal from that plane's X axis.
//! - Face corners are drawn in the stored order (the model requires outline order).
//! - Colors with too little contrast against the background are replaced by the contrasting
//!   foreground (black on light, white on dark backgrounds): the WCAG contrast ratio
//!   `(L1 + 0.05) / (L2 + 0.05)` of stroke and background must be at least 1.25. This is
//!   what makes DGN drawings (palette index 0 is white, MicroStation's default background
//!   is black) readable on the default white page. A transparent background (`None`) is
//!   treated as white; unparsable CSS backgrounds are treated as white as well.
//! - Work and size are bounded: [`SvgOptions::max_work`] (see
//!   [`WalkOptions::max_work`](crate::ops::WalkOptions::max_work)) and
//!   [`SvgOptions::max_output_bytes`]. When either limit cuts the drawing short the root
//!   element carries `data-truncated="true"` and an XML comment says so.
//! - Output is deterministic: no hash-order or time dependence, numbers use a fixed
//!   precision derived from the drawing extent with trailing zeros removed.

use std::f64::consts::{PI, TAU};

use crate::geom::{Point3, Vec3};
use crate::geom_ops::{EllipseArc, Transform, ccw_sweep, conjugate_to_axes};
use crate::model::Value;
use crate::model::{Document, Entity, EntityKind, HAlign, HatchEdge, VAlign, Vertex};
use crate::ops::{
    BBoxOptions, DEFAULT_MAX_WORK, DEFAULT_WALK_DEPTH, WalkContext, WalkOptions, attachment_anchor,
    circle_arc, full_ellipse_arc, mtext_line_height, mtext_text_lines, segment_arc, spline_points,
};

/// SVG rendering options.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgOptions {
    /// Index into [`Document::models`] to render.
    pub model: usize,
    /// Output width in CSS pixels; height follows the drawing aspect ratio.
    pub width_px: f64,
    /// Stroke width in CSS pixels, independent of zoom.
    pub stroke_px: f64,
    /// Background color (CSS), `None` for transparent.
    pub background: Option<String>,
    /// Render text entities as `<text>`.
    pub text: bool,
    /// Expand block references (inserts) into their geometry.
    pub expand_blocks: bool,
    /// Draw raster image frames (in `<g data-kind="images">`). Images never influence the
    /// `viewBox` when the model has other geometry.
    pub images: bool,
    /// Work budget for the bounding box and the drawing walk, in abstract units (about one
    /// per vertex / segment / spline sample / text byte; see `ops::WalkOptions::max_work`).
    pub max_work: u64,
    /// Stop emitting geometry once this many bytes of SVG body have been produced.
    pub max_output_bytes: usize,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            model: 0,
            width_px: 1600.0,
            stroke_px: 1.0,
            background: Some("#ffffff".to_owned()),
            text: true,
            expand_blocks: true,
            images: true,
            max_work: DEFAULT_MAX_WORK,
            max_output_bytes: 256 << 20,
        }
    }
}

/// Number formatter: fixed decimals chosen from the drawing extent, trailing zeros removed.
struct Fmt {
    dec: usize,
}

impl Fmt {
    fn for_extent(extent: f64) -> Self {
        let dec = if extent.is_finite() && extent > 0.0 {
            (6.0 - extent.log10().floor()).clamp(0.0, 12.0)
        } else {
            3.0
        };
        Self { dec: dec as usize }
    }

    fn num(&self, v: f64) -> String {
        fmt_num(v, self.dec)
    }
}

fn fmt_num(v: f64, dec: usize) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let s = format!("{v:.dec$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    if s == "-0" { "0".to_owned() } else { s }
}

/// XML 1.0 `Char` production: tab, LF, CR, U+0020..=U+D7FF, U+E000..=U+FFFD, U+10000.. .
/// (Surrogates cannot occur in `char`; U+FFFE and U+FFFF are not allowed.)
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

/// Parses the CSS background colors we can reason about; everything else counts as white.
fn background_rgb(bg: Option<&str>) -> (u8, u8, u8) {
    let Some(b) = bg.map(|b| b.trim().to_ascii_lowercase()) else {
        return (255, 255, 255);
    };
    match b.as_str() {
        "black" => return (0, 0, 0),
        "white" => return (255, 255, 255),
        _ => {}
    }
    let Some(h) = b.strip_prefix('#') else {
        return (255, 255, 255);
    };
    let digits: Vec<u32> = h.chars().filter_map(|c| c.to_digit(16)).collect();
    if digits.len() != h.chars().count() {
        return (255, 255, 255);
    }
    let byte = |hi: Option<&u32>, lo: Option<&u32>| {
        u8::try_from(hi.copied().unwrap_or(0) * 16 + lo.copied().unwrap_or(0)).unwrap_or(0)
    };
    match digits.len() {
        3 | 4 => {
            let v = |i: usize| digits.get(i);
            (byte(v(0), v(0)), byte(v(1), v(1)), byte(v(2), v(2)))
        }
        6 | 8 => {
            let v = |i: usize| digits.get(i);
            (byte(v(0), v(1)), byte(v(2), v(3)), byte(v(4), v(5)))
        }
        _ => (255, 255, 255),
    }
}

/// WCAG relative luminance of an sRGB color.
fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// Minimum WCAG contrast ratio between a stroke and the background.
const MIN_CONTRAST: f64 = 1.25;

/// `color`, or black / white when it is too close to the background `bg`.
fn with_contrast(color: (u8, u8, u8), bg: (u8, u8, u8)) -> (u8, u8, u8) {
    let (a, b) = (luminance(color), luminance(bg));
    let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
    if ratio >= MIN_CONTRAST {
        color
    } else if b > 0.179 {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if is_xml_char(c) => out.push(c),
            _ => {}
        }
    }
    out
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Builds an SVG path `d` string in view coordinates.
struct PathBuilder<'f> {
    fmt: &'f Fmt,
    d: String,
    cur: Option<(f64, f64)>,
}

impl<'f> PathBuilder<'f> {
    fn new(fmt: &'f Fmt) -> Self {
        Self {
            fmt,
            d: String::new(),
            cur: None,
        }
    }

    fn pt(&self, p: (f64, f64)) -> String {
        format!("{} {}", self.fmt.num(p.0), self.fmt.num(p.1))
    }

    fn move_to(&mut self, p: (f64, f64)) {
        self.d.push('M');
        self.d.push_str(&self.pt(p));
        self.cur = Some(p);
    }

    fn line_to(&mut self, p: (f64, f64)) {
        if self.cur.is_none() {
            self.move_to(p);
            return;
        }
        self.d.push('L');
        self.d.push_str(&self.pt(p));
        self.cur = Some(p);
    }

    /// Moves to `p` when nothing is drawn yet, otherwise draws a line unless already there.
    fn connect(&mut self, p: (f64, f64)) {
        match self.cur {
            None => self.move_to(p),
            Some(c) => {
                if (c.0 - p.0).hypot(c.1 - p.1) > 1e-9 * (1.0 + p.0.abs() + p.1.abs()) {
                    self.line_to(p);
                }
            }
        }
    }

    fn close(&mut self) {
        if !self.d.is_empty() {
            self.d.push('Z');
        }
    }

    fn arc_cmd(&mut self, rx: f64, ry: f64, rot: f64, large: bool, sweep: bool, to: (f64, f64)) {
        self.d.push_str(&format!(
            "A{} {} {} {} {} {}",
            self.fmt.num(rx),
            self.fmt.num(ry),
            fmt_num(rot.to_degrees(), 3),
            u8::from(large),
            u8::from(sweep),
            self.pt(to)
        ));
        self.cur = Some(to);
    }

    /// Appends an elliptical arc given in view coordinates, connecting to the current point.
    fn arc(&mut self, a: &EllipseArc) {
        let p0 = a.point(a.t0);
        let p1 = a.point(a.t1);
        self.connect((p0.x, p0.y));
        let sweep = a.sweep();
        let (rx, ry, rot) = conjugate_to_axes((a.u.x, a.u.y), (a.v.x, a.v.y));
        let det = a.u.x * a.v.y - a.u.y * a.v.x;
        if !(rx.is_finite() && ry.is_finite() && rot.is_finite())
            || rx < 1e-12
            || ry < 1e-12
            || det.abs() < 1e-18
            || sweep.abs() < 1e-12
        {
            self.line_to((p1.x, p1.y));
            return;
        }
        // In y-down view coordinates a positive determinant means increasing parameter runs
        // clockwise on screen, which is SVG's positive-angle direction (sweep-flag 1).
        let flag = (sweep > 0.0) == (det > 0.0);
        if sweep.abs() >= TAU - 1e-9 {
            let mid = a.point(a.t0 + sweep / 2.0);
            self.arc_cmd(rx, ry, rot, false, flag, (mid.x, mid.y));
            self.arc_cmd(rx, ry, rot, false, flag, (p1.x, p1.y));
        } else {
            self.arc_cmd(rx, ry, rot, sweep.abs() > PI, flag, (p1.x, p1.y));
        }
    }

    fn polyline(&mut self, pts: &[(f64, f64)], closed: bool) {
        for (i, p) in pts.iter().enumerate() {
            if i == 0 {
                self.move_to(*p);
            } else {
                self.line_to(*p);
            }
        }
        if closed {
            self.close();
        }
    }

    /// Polyline with bulge arcs; `tf` maps vertex coordinates to view coordinates.
    fn bulge_polyline(&mut self, vs: &[Vertex], closed: bool, normal: Vec3, tf: &Transform) {
        let n = vs.len();
        if n == 0 {
            return;
        }
        let first = tf.apply_point(vs.first().map_or(Point3::default(), |v| v.position));
        self.move_to((first.x, first.y));
        let segs = if closed { n } else { n - 1 };
        for i in 0..segs {
            let (Some(a), Some(b)) = (vs.get(i), vs.get((i + 1) % n)) else {
                continue;
            };
            if let Some(arc) = segment_arc(a, b, normal, tf) {
                self.arc(&arc);
            } else {
                let p = tf.apply_point(b.position);
                self.line_to((p.x, p.y));
            }
        }
        if closed {
            self.close();
        }
    }
}

/// One `<g>` per layer, in order of first use.
#[derive(Default)]
struct Layers {
    index: std::collections::HashMap<String, usize>,
    items: Vec<(String, String)>,
    images: String,
    /// Bytes of body text produced so far.
    bytes: usize,
}

impl Layers {
    fn push_image(&mut self, text: &str) {
        self.bytes += text.len();
        self.images.push_str(text);
    }

    fn push(&mut self, name: &str, text: &str) {
        self.bytes += text.len();
        if let Some(&i) = self.index.get(name) {
            if let Some((_, body)) = self.items.get_mut(i) {
                body.push_str(text);
            }
            return;
        }
        self.index.insert(name.to_owned(), self.items.len());
        self.items.push((name.to_owned(), text.to_owned()));
    }
}

struct Renderer<'a> {
    doc: &'a Document,
    opts: &'a SvgOptions,
    fmt: Fmt,
    view: Transform,
    cross: f64,
    layers: Layers,
    background: (u8, u8, u8),
}

impl Renderer<'_> {
    fn stroke(color: (u8, u8, u8)) -> String {
        format!(
            " stroke=\"{}\" vector-effect=\"non-scaling-stroke\"",
            hex(color)
        )
    }

    fn emit_path(&mut self, layer: &str, d: &str, extra: &str) {
        if d.is_empty() {
            return;
        }
        let line = format!("<path d=\"{d}\"{extra}/>\n");
        self.layers.push(layer, &line);
    }

    fn stroke_path(&mut self, layer: &str, pb: &PathBuilder<'_>, color: (u8, u8, u8)) {
        let extra = Self::stroke(color);
        self.emit_path(layer, &pb.d, &extra);
    }

    fn render_entity(&mut self, e: &Entity, ctx: &WalkContext<'_>) {
        let layer = ctx.layer_name(e).to_owned();
        let color = with_contrast(e.resolved_color_in(self.doc, ctx), self.background);
        let tf = self.view.compose(&ctx.transform);
        let fmt = Fmt { dec: self.fmt.dec };
        let mut pb = PathBuilder::new(&fmt);
        match &e.kind {
            EntityKind::Point { position } => {
                let p = tf.apply_point(*position);
                self.cross_path(&mut pb, (p.x, p.y));
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Line { start, end } => {
                let (a, b) = (tf.apply_point(*start), tf.apply_point(*end));
                pb.move_to((a.x, a.y));
                pb.line_to((b.x, b.y));
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Polyline {
                vertices,
                closed,
                normal,
            } => {
                pb.bulge_polyline(vertices, *closed, *normal, &tf);
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Circle {
                center,
                radius,
                normal,
            } => {
                let arc = circle_arc(*center, *radius, 0.0, 0.0, *normal, &tf);
                pb.arc(&arc);
                pb.close();
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                normal,
            } => {
                let arc = circle_arc(*center, *radius, *start_angle, *end_angle, *normal, &tf);
                pb.arc(&arc);
                if arc.sweep().abs() >= TAU - 1e-9 {
                    pb.close();
                }
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Ellipse {
                center,
                major_axis,
                ratio,
                start_param,
                end_param,
                normal,
            } => {
                let arc = full_ellipse_arc(
                    *center,
                    *major_axis,
                    *ratio,
                    *normal,
                    *start_param,
                    *end_param,
                    &tf,
                );
                pb.arc(&arc);
                if arc.sweep().abs() >= TAU - 1e-9 {
                    pb.close();
                }
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Spline {
                degree,
                knots,
                control_points,
                weights,
                fit_points,
                closed,
            } => {
                let pts = spline_points(*degree, knots, control_points, weights, fit_points, &tf);
                let v: Vec<(f64, f64)> = pts.iter().map(|p| (p.x, p.y)).collect();
                pb.polyline(&v, *closed && v.len() > 2);
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Text {
                position,
                end_point: Some(end),
                height,
                width_factor,
                value,
                halign: halign @ (HAlign::Aligned | HAlign::Fit),
                normal,
                ..
            } => {
                if self.opts.text {
                    let fit = *halign == HAlign::Fit;
                    self.text_between(
                        &layer,
                        color,
                        &tf,
                        (*position, *end),
                        (*height, *width_factor),
                        *normal,
                        value,
                        fit,
                    );
                }
            }
            EntityKind::Text {
                position,
                height,
                rotation,
                value,
                halign,
                valign,
                normal,
                ..
            } => {
                if self.opts.text {
                    self.text(
                        &layer,
                        color,
                        &tf,
                        *position,
                        *height,
                        *rotation,
                        *normal,
                        TextBody::Single(value),
                        *halign,
                        *valign,
                    );
                }
            }
            EntityKind::MText {
                position,
                height,
                rotation,
                value,
                plain,
                attachment,
                line_spacing,
                normal,
                ..
            } => {
                if self.opts.text {
                    let (ha, va) = attachment_anchor(*attachment);
                    let halign = match ha {
                        -1 => HAlign::Left,
                        0 => HAlign::Center,
                        _ => HAlign::Right,
                    };
                    let valign = match va {
                        -1 => VAlign::Top,
                        0 => VAlign::Middle,
                        _ => VAlign::Bottom,
                    };
                    let lines = mtext_text_lines(value, plain);
                    let lh = mtext_line_height(*height, *line_spacing);
                    self.text(
                        &layer,
                        color,
                        &tf,
                        *position,
                        *height,
                        *rotation,
                        *normal,
                        TextBody::Multi(&lines, lh),
                        halign,
                        valign,
                    );
                }
            }
            EntityKind::Insert { position, .. } => {
                // Reached only when the block is missing, cyclic, too deep or expansion is off.
                let p = tf.apply_point(*position);
                self.cross_path(&mut pb, (p.x, p.y));
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Hatch {
                loops,
                solid,
                normal,
                ..
            } => {
                for l in loops {
                    hatch_loop(&mut pb, &l.edges, *normal, &tf);
                }
                let extra = if *solid {
                    format!(
                        " fill=\"{}\" fill-rule=\"evenodd\" stroke=\"none\"",
                        hex(color)
                    )
                } else {
                    format!(" fill-rule=\"evenodd\"{}", Self::stroke(color))
                };
                self.emit_path(&layer, &pb.d, &extra);
            }
            EntityKind::Dimension { points, .. } => {
                let v: Vec<(f64, f64)> = points
                    .iter()
                    .map(|p| tf.apply_point(*p))
                    .map(|p| (p.x, p.y))
                    .collect();
                if v.len() >= 2 {
                    pb.polyline(&v, false);
                    self.stroke_path(&layer, &pb, color);
                }
            }
            EntityKind::Face { points, filled } => {
                let mut v: Vec<(f64, f64)> = Vec::new();
                for p in points {
                    let q = tf.apply_point(*p);
                    if v.last() != Some(&(q.x, q.y)) {
                        v.push((q.x, q.y));
                    }
                }
                if v.len() >= 3 {
                    pb.polyline(&v, true);
                    let extra = if *filled {
                        format!(" fill=\"{}\"{}", hex(color), Self::stroke(color))
                    } else {
                        Self::stroke(color)
                    };
                    self.emit_path(&layer, &pb.d, &extra);
                }
            }
            EntityKind::Polygon {
                exterior,
                interiors,
            } => {
                for ring in std::iter::once(exterior).chain(interiors.iter()) {
                    let points: Vec<_> = ring
                        .iter()
                        .map(|p| tf.apply_point(*p))
                        .map(|p| (p.x, p.y))
                        .collect();
                    pb.polyline(&points, true);
                }
                self.emit_path(
                    &layer,
                    &pb.d,
                    &format!(
                        " fill=\"{}\" fill-rule=\"evenodd\"{}",
                        hex(color),
                        Self::stroke(color)
                    ),
                );
            }
            EntityKind::Leader { vertices, .. } => {
                let v: Vec<(f64, f64)> = vertices
                    .iter()
                    .map(|p| tf.apply_point(*p))
                    .map(|p| (p.x, p.y))
                    .collect();
                if v.len() >= 2 {
                    pb.polyline(&v, false);
                    self.stroke_path(&layer, &pb, color);
                }
            }
            EntityKind::Image {
                position,
                u_vector,
                v_vector,
                ..
            } => {
                if !self.opts.images {
                    return;
                }
                let corner = |a: f64, b: f64| {
                    let p = tf.apply_point(
                        position.translated(u_vector.scaled(a).plus(v_vector.scaled(b))),
                    );
                    (p.x, p.y)
                };
                pb.polyline(
                    &[
                        corner(0.0, 0.0),
                        corner(1.0, 0.0),
                        corner(1.0, 1.0),
                        corner(0.0, 1.0),
                    ],
                    true,
                );
                // A frame whose extent is shared (not the true footprint) is drawn faintly.
                let shared = matches!(
                    e.props.get("dgn.raster_extent_shared"),
                    Some(Value::Bool(true) | Value::Int(1))
                );
                let faint = if shared {
                    " stroke-opacity=\"0.25\""
                } else {
                    ""
                };
                let extra = format!("{}{faint} stroke-dasharray=\"4 3\"", Self::stroke(color));
                if !pb.d.is_empty() {
                    self.layers
                        .push_image(&format!("<path d=\"{}\"{extra}/>\n", pb.d));
                }
            }
            EntityKind::Mesh { vertices, faces } => {
                for f in faces {
                    let v: Vec<(f64, f64)> = f
                        .iter()
                        .filter_map(|&i| vertices.get(i as usize))
                        .map(|p| tf.apply_point(*p))
                        .map(|p| (p.x, p.y))
                        .collect();
                    if v.len() >= 2 {
                        for (i, p) in v.iter().enumerate() {
                            if i == 0 {
                                pb.move_to(*p);
                            } else {
                                pb.line_to(*p);
                            }
                        }
                        pb.close();
                    }
                }
                self.stroke_path(&layer, &pb, color);
            }
            EntityKind::Viewport { .. } | EntityKind::Group { .. } | EntityKind::Unknown { .. } => {
            }
        }
    }

    fn cross_path(&self, pb: &mut PathBuilder<'_>, p: (f64, f64)) {
        let s = self.cross;
        pb.move_to((p.0 - s, p.1));
        pb.line_to((p.0 + s, p.1));
        pb.move_to((p.0, p.1 - s));
        pb.line_to((p.0, p.1 + s));
    }

    /// Aligned / fit text along the baseline `ends.0 -> ends.1` (world points).
    /// Fit uses `textLength`; aligned scales the font size so the text spans the baseline.
    #[allow(clippy::too_many_arguments)]
    fn text_between(
        &mut self,
        layer: &str,
        color: (u8, u8, u8),
        tf: &Transform,
        ends: (Point3, Point3),
        size: (f64, f64),
        normal: Vec3,
        value: &str,
        fit: bool,
    ) {
        let (p0, p1) = (tf.apply_point(ends.0), tf.apply_point(ends.1));
        let len = (p1.x - p0.x).hypot(p1.y - p0.y);
        let dir_w = ends.0.vector_to(ends.1);
        let up_w = normal
            .cross(dir_w)
            .normalized()
            .unwrap_or(Vec3::new(0.0, 1.0, 0.0));
        let up = tf.apply_vector(up_w);
        let base = size.0 * up.x.hypot(up.y);
        let wf = if size.1.is_finite() && size.1 > 0.0 {
            size.1
        } else {
            1.0
        };
        let chars = value.chars().count().max(1) as f64;
        let natural = 0.6 * base * wf * chars;
        if !(len.is_finite() && len > 1e-12 && base.is_finite() && base > 0.0 && natural > 0.0) {
            return;
        }
        let font = if fit { base } else { base * len / natural };
        let angle = (p1.y - p0.y).atan2(p1.x - p0.x).to_degrees();
        let (x, y) = (self.fmt.num(p0.x), self.fmt.num(p0.y));
        let mut el = format!("<text x=\"{x}\" y=\"{y}\"");
        if angle.abs() > 1e-6 {
            el.push_str(&format!(
                " transform=\"rotate({} {x} {y})\"",
                fmt_num(angle, 3)
            ));
        }
        el.push_str(&format!(
            " font-size=\"{}\" text-anchor=\"start\"",
            self.fmt.num(font)
        ));
        if fit {
            el.push_str(&format!(
                " textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\"",
                self.fmt.num(len)
            ));
        }
        el.push_str(&format!(
            " fill=\"{}\">{}</text>\n",
            hex(color),
            escape(value)
        ));
        self.layers.push(layer, &el);
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        layer: &str,
        color: (u8, u8, u8),
        tf: &Transform,
        position: Point3,
        height: f64,
        rotation: f64,
        normal: Vec3,
        body: TextBody<'_>,
        halign: HAlign,
        valign: VAlign,
    ) {
        let t = tf.compose(&Transform::plane(position, normal));
        let p = t.apply_point(Point3::default());
        let (s, c) = rotation.sin_cos();
        let dir = t.apply_vector(Vec3::new(c, s, 0.0));
        let up = t.apply_vector(Vec3::new(-s, c, 0.0));
        let angle = dir.y.atan2(dir.x).to_degrees();
        let scale = up.x.hypot(up.y);
        let size = height * scale;
        if !(size.is_finite() && size > 0.0 && p.x.is_finite() && p.y.is_finite()) {
            return;
        }
        let anchor = match halign {
            HAlign::Left | HAlign::Aligned | HAlign::Fit => "start",
            HAlign::Right => "end",
            _ => "middle",
        };
        let baseline = match (halign, valign) {
            // Aligned/fit text without an end point sits on its baseline start.
            (HAlign::Aligned | HAlign::Fit, _) => None,
            (HAlign::Middle, _) | (_, VAlign::Middle) => Some("central"),
            (_, VAlign::Top) => Some("hanging"),
            _ => None,
        };
        let (x, y) = (self.fmt.num(p.x), self.fmt.num(p.y));
        let mut el = format!("<text x=\"{x}\" y=\"{y}\"");
        if angle.abs() > 1e-6 {
            el.push_str(&format!(
                " transform=\"rotate({} {x} {y})\"",
                fmt_num(angle, 3)
            ));
        }
        el.push_str(&format!(
            " font-size=\"{}\" text-anchor=\"{anchor}\"",
            self.fmt.num(size)
        ));
        if let Some(b) = baseline {
            el.push_str(&format!(" dominant-baseline=\"{b}\""));
        }
        el.push_str(&format!(" fill=\"{}\">", hex(color)));
        match body {
            TextBody::Single(v) => el.push_str(&escape(v)),
            TextBody::Multi(lines, lh) => {
                let lh = lh * scale;
                let n = lines.len().max(1) as f64;
                let first = match valign {
                    VAlign::Top => 0.0,
                    VAlign::Middle => -(n - 1.0) * lh / 2.0,
                    _ => -(n - 1.0) * lh,
                };
                for (i, line) in lines.iter().enumerate() {
                    let dy = if i == 0 { first } else { lh };
                    el.push_str(&format!(
                        "<tspan x=\"{x}\" dy=\"{}\">{}</tspan>",
                        self.fmt.num(dy),
                        escape(line)
                    ));
                }
            }
        }
        el.push_str("</text>\n");
        self.layers.push(layer, &el);
    }
}

enum TextBody<'a> {
    Single(&'a str),
    /// Lines and the baseline distance in drawing units.
    Multi(&'a [String], f64),
}

fn hatch_loop(pb: &mut PathBuilder<'_>, edges: &[HatchEdge], normal: Vec3, t: &Transform) {
    pb.cur = None;
    for edge in edges {
        match edge {
            HatchEdge::Line { start, end } => {
                let (a, b) = (t.apply_point(*start), t.apply_point(*end));
                pb.connect((a.x, a.y));
                pb.line_to((b.x, b.y));
            }
            HatchEdge::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ccw,
            } => {
                let mut arc =
                    EllipseArc::circle(Point3::default(), *radius, *start_angle, *end_angle);
                if !*ccw {
                    arc.t0 = *start_angle;
                    arc.t1 = *start_angle - ccw_sweep(*end_angle, *start_angle);
                }
                pb.arc(&arc.transformed(&t.compose(&Transform::plane(*center, normal))));
            }
            HatchEdge::Ellipse {
                center,
                major_axis,
                ratio,
                start_param,
                end_param,
                ccw,
            } => {
                let mut arc = crate::geom_ops::ellipse_arc(
                    *center,
                    *major_axis,
                    *ratio,
                    normal,
                    *start_param,
                    *end_param,
                );
                if !*ccw {
                    arc.t0 = *start_param;
                    arc.t1 = *start_param - ccw_sweep(*end_param, *start_param);
                }
                pb.arc(&arc.transformed(t));
            }
            HatchEdge::Spline {
                degree,
                knots,
                control_points,
                weights,
            } => {
                for (i, p) in spline_points(*degree, knots, control_points, weights, &[], t)
                    .iter()
                    .enumerate()
                {
                    if i == 0 {
                        pb.connect((p.x, p.y));
                    } else {
                        pb.line_to((p.x, p.y));
                    }
                }
            }
            HatchEdge::Polyline { vertices, closed } => {
                let n = vertices.len();
                for (i, v) in vertices.iter().enumerate() {
                    let p = t.apply_point(v.position);
                    if i == 0 {
                        pb.connect((p.x, p.y));
                    }
                    let last = i + 1 == n;
                    if last && !*closed {
                        break;
                    }
                    if let Some(next) = vertices.get((i + 1) % n) {
                        if let Some(arc) = segment_arc(v, next, normal, t) {
                            pb.arc(&arc);
                        } else {
                            let q = t.apply_point(next.position);
                            pb.line_to((q.x, q.y));
                        }
                    }
                }
            }
        }
    }
    pb.close();
}

/// Renders one model of `doc` as a standalone SVG document.
///
/// An empty model (or a model index out of range) yields a valid SVG with an empty 100x100
/// view box. See the module documentation for the drawing conventions.
pub fn render(doc: &Document, options: &SvgOptions) -> String {
    let width = if options.width_px.is_finite() && options.width_px > 0.0 {
        options.width_px
    } else {
        1600.0
    };
    let stroke = if options.stroke_px.is_finite() && options.stroke_px > 0.0 {
        options.stroke_px
    } else {
        1.0
    };
    let px = Fmt { dec: 2 };
    let bg = options.background.as_deref().filter(|b| !b.is_empty());

    // Fit to the vector geometry; image frames only count when nothing else is drawn.
    let fitted = doc
        .bbox_with(
            options.model,
            &BBoxOptions {
                include_images: false,
                max_work: options.max_work,
            },
        )
        .or_else(|| {
            doc.bbox_with(
                options.model,
                &BBoxOptions {
                    include_images: true,
                    max_work: options.max_work,
                },
            )
        });
    let Some(bb) = fitted else {
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{w}\" viewBox=\"0 0 100 100\">\n",
            w = px.num(width)
        );
        if let Some(b) = bg {
            out.push_str(&format!(
                "<rect x=\"0\" y=\"0\" width=\"100\" height=\"100\" fill=\"{}\"/>\n",
                escape(b)
            ));
        }
        out.push_str("</svg>\n");
        return out;
    };

    let extent = bb.width().max(bb.height());
    let margin = if extent > 0.0 { extent * 0.02 } else { 1.0 };
    let vx = bb.min.x - margin;
    let vy = -bb.max.y - margin;
    let vw = bb.width() + 2.0 * margin;
    let vh = bb.height() + 2.0 * margin;
    let height = (width * vh / vw).max(1.0);
    let fmt = Fmt::for_extent(vw.max(vh));

    let mut r = Renderer {
        doc,
        opts: options,
        fmt: Fmt { dec: fmt.dec },
        view: Transform::scaling(1.0, -1.0, 1.0),
        cross: vw.max(vh) * 0.004,
        layers: Layers::default(),
        background: background_rgb(options.background.as_deref()),
    };
    let walk_opts = WalkOptions {
        max_depth: DEFAULT_WALK_DEPTH,
        expand_inserts: options.expand_blocks,
        max_work: options.max_work,
    };
    let cap = options.max_output_bytes;
    let stats = doc.walk_while(options.model, &walk_opts, |e, ctx| {
        if r.layers.bytes > cap {
            return false;
        }
        r.render_entity(e, ctx);
        true
    });
    let truncated = stats.truncated;

    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\"{}>\n",
        px.num(width),
        px.num(height),
        fmt.num(vx),
        fmt.num(vy),
        fmt.num(vw),
        fmt.num(vh),
        if truncated {
            " data-truncated=\"true\""
        } else {
            ""
        }
    );
    if truncated {
        out.push_str("<!-- cadkit: drawing truncated, work or output size limit reached -->\n");
    }
    if let Some(b) = bg {
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
            fmt.num(vx),
            fmt.num(vy),
            fmt.num(vw),
            fmt.num(vh),
            escape(b)
        ));
    }
    for (name, body) in &r.layers.items {
        out.push_str(&format!(
            "<g data-layer=\"{}\" fill=\"none\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n",
            escape(name),
            fmt_num(stroke, 3)
        ));
        out.push_str(body);
        out.push_str("</g>\n");
    }
    if !r.layers.images.is_empty() {
        out.push_str(&format!(
            "<g data-kind=\"images\" fill=\"none\" stroke-width=\"{}\">\n",
            fmt_num(stroke, 3)
        ));
        out.push_str(&r.layers.images);
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Attachment, Block, Color, GroupKind, HatchLoop, Layer, Model, Vertex};

    fn doc(entities: Vec<Entity>) -> Document {
        Document {
            models: vec![Model {
                name: "Model".into(),
                entities,
                ..Model::default()
            }],
            ..Document::default()
        }
    }

    fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Entity {
        Entity::new(EntityKind::Line {
            start: Point3::xy(x0, y0),
            end: Point3::xy(x1, y1),
        })
    }

    fn svg(entities: Vec<Entity>) -> String {
        render(&doc(entities), &SvgOptions::default())
    }

    #[test]
    fn number_format_is_compact() {
        assert_eq!(fmt_num(1.000_000_000_1e-12, 6), "0");
        assert_eq!(fmt_num(-0.000_000_1, 3), "0");
        assert_eq!(fmt_num(2.5, 6), "2.5");
        assert_eq!(fmt_num(10.0, 4), "10");
        assert_eq!(fmt_num(f64::NAN, 4), "0");
        assert_eq!(fmt_num(100.0, 0), "100");
        assert_eq!(fmt_num(0.1 + 0.2, 6), "0.3");
    }

    #[test]
    fn empty_document_is_valid_svg() {
        let s = render(&Document::default(), &SvgOptions::default());
        assert!(s.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(s.contains("viewBox=\"0 0 100 100\""));
        assert!(s.trim_end().ends_with("</svg>"));
        let s = svg(vec![]);
        assert!(s.contains("<rect") && s.trim_end().ends_with("</svg>"));
        let o = SvgOptions {
            background: None,
            model: 9,
            ..SvgOptions::default()
        };
        assert!(!render(&Document::default(), &o).contains("<rect"));
    }

    #[test]
    fn line_structure_and_flip() {
        let s = svg(vec![line(0.0, 0.0, 10.0, 5.0)]);
        assert!(s.contains("viewBox=\"-0.2 -5.2 10.4 5.4\""), "{s}");
        assert!(s.contains("width=\"1600\" height=\"830.77\""), "{s}");
        assert!(s.contains("<g data-layer=\"0\""));
        assert!(s.contains("d=\"M0 0L10 -5\""), "{s}");
        assert!(s.contains("vector-effect=\"non-scaling-stroke\""));
        assert!(s.contains("stroke-width=\"1\""));
        assert!(s.contains("stroke=\"#000000\""));
        assert!(s.contains("fill=\"#ffffff\""));
    }

    #[test]
    fn options_applied() {
        let o = SvgOptions {
            width_px: 400.0,
            stroke_px: 2.5,
            background: None,
            ..SvgOptions::default()
        };
        let s = render(&doc(vec![line(0.0, 0.0, 10.0, 10.0)]), &o);
        assert!(s.contains("width=\"400\""));
        assert!(s.contains("stroke-width=\"2.5\""));
        assert!(!s.contains("<rect"));
        let bad = SvgOptions {
            width_px: f64::NAN,
            stroke_px: -1.0,
            ..SvgOptions::default()
        };
        assert!(render(&doc(vec![line(0.0, 0.0, 1.0, 1.0)]), &bad).contains("width=\"1600\""));
    }

    #[test]
    fn deterministic() {
        let ents = vec![
            line(0.0, 0.0, 3.0, 4.0),
            Entity::new(EntityKind::Circle {
                center: Point3::xy(1.0, 1.0),
                radius: 2.0,
                normal: Vec3::Z,
            }),
        ];
        assert_eq!(svg(ents.clone()), svg(ents));
    }

    #[test]
    fn layers_grouped_and_hidden_skipped() {
        let mut a = line(0.0, 0.0, 1.0, 1.0);
        a.layer = Some("A<&>".into());
        let mut b = line(0.0, 0.0, 2.0, 2.0);
        b.layer = Some("off".into());
        let mut c = line(1.0, 0.0, 2.0, 1.0);
        c.layer = Some("A<&>".into());
        let mut d = line(5.0, 5.0, 6.0, 6.0);
        d.visible = false;
        let mut dd = doc(vec![a, b, c, d]);
        dd.layers.push(Layer {
            name: "off".into(),
            visible: false,
            ..Layer::default()
        });
        dd.layers.push(Layer {
            name: "A<&>".into(),
            visible: true,
            color: Color::Aci { index: 1 },
            ..Layer::default()
        });
        let s = render(&dd, &SvgOptions::default());
        assert_eq!(s.matches("<g data-layer=").count(), 1, "{s}");
        assert!(s.contains("data-layer=\"A&lt;&amp;&gt;\""));
        assert_eq!(s.matches("<path").count(), 2);
        assert!(s.contains("stroke=\"#ff0000\""));
        // The invisible far-away line did not stretch the view box.
        assert!(s.contains("viewBox=\"-0.04"), "{s}");
    }

    #[test]
    fn circle_and_arc() {
        let s = svg(vec![Entity::new(EntityKind::Circle {
            center: Point3::xy(0.0, 0.0),
            radius: 5.0,
            normal: Vec3::Z,
        })]);
        assert_eq!(s.matches('A').count(), 2, "{s}");
        assert!(s.contains("A5 5 0 0 0 -5 0A5 5 0 0 0 5 0Z"), "{s}");
        assert!(s.contains("Z\""));
        // Quarter arc CCW from (5,0) to (0,5): in the flipped view it ends at (0,-5), sweep-flag 0.
        let s = svg(vec![Entity::new(EntityKind::Arc {
            center: Point3::xy(0.0, 0.0),
            radius: 5.0,
            start_angle: 0.0,
            end_angle: PI / 2.0,
            normal: Vec3::Z,
        })]);
        assert!(s.contains("d=\"M5 0A5 5 0 0 0 0 -5\""), "{s}");
        // Large arc flag for 270 degrees.
        let s = svg(vec![Entity::new(EntityKind::Arc {
            center: Point3::xy(0.0, 0.0),
            radius: 5.0,
            start_angle: 0.0,
            end_angle: 3.0 * PI / 2.0,
            normal: Vec3::Z,
        })]);
        assert!(s.contains("A5 5 0 1 0 0 5"), "{s}");
    }

    #[test]
    fn ellipse_partial_and_full() {
        let full = Entity::new(EntityKind::Ellipse {
            center: Point3::xy(0.0, 0.0),
            major_axis: Vec3::new(4.0, 0.0, 0.0),
            ratio: 0.5,
            start_param: 0.0,
            end_param: TAU,
            normal: Vec3::Z,
        });
        let s = svg(vec![full.clone()]);
        assert!(s.contains("A4 2 0"), "{s}");
        assert!(s.contains('Z'));
        let mut part = full;
        if let EntityKind::Ellipse { end_param, .. } = &mut part.kind {
            *end_param = PI;
        }
        let s = svg(vec![part]);
        assert_eq!(s.matches('A').count(), 1);
        assert!(!s.contains("Z\""));
    }

    #[test]
    fn polyline_bulge_and_closed() {
        let mk = |normal: Vec3| {
            Entity::new(EntityKind::Polyline {
                vertices: vec![
                    Vertex {
                        position: Point3::xy(0.0, 0.0),
                        bulge: 1.0,
                        ..Vertex::default()
                    },
                    Vertex::at(Point3::xy(2.0, 0.0)),
                    Vertex::at(Point3::xy(2.0, 2.0)),
                ],
                closed: true,
                normal,
            })
        };
        // Flipped normal: the bulge goes up (world +Y), i.e. up on screen too, so the first
        // arc ends at the same point but with the opposite sweep flag.
        let flipped = svg(vec![mk(Vec3::new(0.0, 0.0, -1.0))]);
        assert!(
            flipped.contains("M0 0A1 1 180 0 1 2 0L2 -2L0 0Z"),
            "{flipped}"
        );
        let s = svg(vec![mk(Vec3::Z)]);
        assert!(s.contains("M0 0A1 1 0 0 0 2 0L2 -2L0 0Z"), "{s}");
    }

    #[test]
    fn spline_is_flattened() {
        let sp = Entity::new(EntityKind::Spline {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point3::xy(0.0, 0.0),
                Point3::xy(5.0, 10.0),
                Point3::xy(10.0, 0.0),
            ],
            weights: vec![],
            fit_points: vec![],
            closed: false,
        });
        let s = svg(vec![sp]);
        assert_eq!(s.matches('L').count(), 16, "{s}");
        assert!(s.contains("M0 0L"));
    }

    #[test]
    fn text_attributes_and_escaping() {
        let t = Entity::new(EntityKind::Text {
            position: Point3::xy(1.0, 2.0),
            end_point: None,
            height: 3.0,
            rotation: PI / 2.0,
            width_factor: 1.0,
            oblique: 0.0,
            value: "a<b & \"c\"".into(),
            style: None,
            halign: HAlign::Center,
            valign: VAlign::Baseline,
            normal: Vec3::Z,
        });
        let s = svg(vec![t.clone()]);
        assert!(s.contains("<text x=\"1\" y=\"-2\" transform=\"rotate(-90 1 -2)\" font-size=\"3\" text-anchor=\"middle\""), "{s}");
        assert!(s.contains(">a&lt;b &amp; &quot;c&quot;</text>"), "{s}");
        let o = SvgOptions {
            text: false,
            ..SvgOptions::default()
        };
        assert!(!render(&doc(vec![t, line(0.0, 0.0, 1.0, 1.0)]), &o).contains("<text"));
    }

    #[test]
    fn low_contrast_colors_flip_to_the_foreground() {
        let mut white = line(0.0, 0.0, 10.0, 10.0);
        white.color = Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        let mut red = line(0.0, 0.0, 5.0, 5.0);
        red.color = Color::Rgb { r: 255, g: 0, b: 0 };
        let mut yellow = line(0.0, 0.0, 1.0, 1.0);
        yellow.color = Color::Aci { index: 2 };
        let s = svg(vec![white.clone(), red.clone(), yellow]);
        assert!(
            !s.contains("stroke=\"#ffffff\"") && !s.contains("stroke=\"#ffff00\""),
            "{s}"
        );
        assert!(s.contains("stroke=\"#ff0000\""));
        assert_eq!(s.matches("stroke=\"#000000\"").count(), 2);
        // Transparent background counts as white.
        let o = SvgOptions {
            background: None,
            ..SvgOptions::default()
        };
        assert!(!render(&doc(vec![white.clone()]), &o).contains("stroke=\"#ffffff\""));
        // Dark background: black flips to white, white stays.
        let o = SvgOptions {
            background: Some("#101010".into()),
            ..SvgOptions::default()
        };
        let mut black = line(0.0, 0.0, 1.0, 1.0);
        black.color = Color::Rgb { r: 0, g: 0, b: 0 };
        let s = render(&doc(vec![black, white]), &o);
        assert!(
            s.contains("stroke=\"#ffffff\"") && !s.contains("stroke=\"#000000\""),
            "{s}"
        );
        assert_eq!(background_rgb(Some("#fff")), (255, 255, 255));
        assert_eq!(background_rgb(Some("#102030")), (16, 32, 48));
        assert_eq!(background_rgb(Some("black")), (0, 0, 0));
        assert_eq!(background_rgb(Some("papayawhip")), (255, 255, 255));
    }

    #[test]
    fn work_and_size_limits_mark_truncation() {
        let ents: Vec<Entity> = (0..50).map(|i| line(0.0, 0.0, f64::from(i), 1.0)).collect();
        let full = svg(ents.clone());
        assert!(!full.contains("data-truncated"));
        let o = SvgOptions {
            max_work: 20,
            ..SvgOptions::default()
        };
        let s = render(&doc(ents.clone()), &o);
        assert!(
            s.contains("data-truncated=\"true\"") && s.contains("<!-- cadkit: drawing truncated"),
            "{s}"
        );
        assert!(s.matches("<path").count() < 50);
        let o = SvgOptions {
            max_output_bytes: 300,
            ..SvgOptions::default()
        };
        let s = render(&doc(ents), &o);
        assert!(s.contains("data-truncated=\"true\""));
        assert!(s.matches("<path").count() < 50 && s.contains("<path"));
    }

    #[test]
    fn aligned_text_without_end_point_starts_at_the_anchor() {
        let t = Entity::new(EntityKind::Text {
            position: Point3::xy(1.0, 2.0),
            end_point: None,
            height: 1.0,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            value: "x".into(),
            style: None,
            halign: HAlign::Fit,
            valign: VAlign::Top,
            normal: Vec3::Z,
        });
        let s = svg(vec![t]);
        assert!(
            s.contains("text-anchor=\"start\"") && !s.contains("dominant-baseline"),
            "{s}"
        );
    }

    #[test]
    fn escape_drops_xml_invalid_characters() {
        assert_eq!(
            escape("a\u{FFFE}b\u{FFFF}c\u{1}d\u{7}e\u{FFFD}"),
            "abcde\u{FFFD}"
        );
        assert_eq!(escape("t\tn\nr\r\u{10000}"), "t\tn\nr\r\u{10000}");
    }

    #[test]
    fn viewbox_follows_vectors_not_images() {
        let mut image = Entity::new(EntityKind::Image {
            path: None,
            position: Point3::xy(-20.0, -20.0),
            u_vector: Vec3::new(50.0, 0.0, 0.0),
            v_vector: Vec3::new(0.0, 50.0, 0.0),
            size_px: None,
        });
        image
            .props
            .insert("dgn.raster_extent_shared".into(), Value::Bool(true));
        let s = svg(vec![line(0.0, 0.0, 10.0, 10.0), image.clone()]);
        assert!(s.contains("viewBox=\"-0.2 -10.2 10.4 10.4\""), "{s}");
        assert!(s.contains("<g data-kind=\"images\""));
        assert!(s.contains("stroke-opacity=\"0.25\""));
        assert!(!s.contains("data-layer=\"images\""));
        let o = SvgOptions {
            images: false,
            ..SvgOptions::default()
        };
        let s = render(&doc(vec![line(0.0, 0.0, 10.0, 10.0), image.clone()]), &o);
        assert!(!s.contains("data-kind"));
        // Only an image: fall back to its extent.
        let s = svg(vec![image]);
        assert!(s.contains("viewBox=\"-21"), "{s}");
    }

    #[test]
    fn aligned_and_fit_text_follow_baseline() {
        let mk = |halign: HAlign| {
            Entity::new(EntityKind::Text {
                position: Point3::xy(0.0, 0.0),
                end_point: Some(Point3::xy(0.0, 12.0)),
                height: 2.0,
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                value: "abcd".into(),
                style: None,
                halign,
                valign: VAlign::Baseline,
                normal: Vec3::Z,
            })
        };
        let fit = svg(vec![mk(HAlign::Fit)]);
        assert!(
            fit.contains("rotate(-90 0 0)")
                && fit.contains("textLength=\"12\"")
                && fit.contains("lengthAdjust=\"spacingAndGlyphs\""),
            "{fit}"
        );
        assert!(fit.contains("font-size=\"2\""), "{fit}");
        // Aligned: natural width is 0.6 * 2 * 4 = 4.8, so the font grows by 12 / 4.8 = 2.5.
        let aligned = svg(vec![mk(HAlign::Aligned)]);
        assert!(
            aligned.contains("font-size=\"5\"") && !aligned.contains("textLength"),
            "{aligned}"
        );
    }

    #[test]
    fn mtext_lines_become_tspans() {
        let t = Entity::new(EntityKind::MText {
            position: Point3::xy(0.0, 0.0),
            height: 3.0,
            width: 0.0,
            rotation: 0.0,
            value: "one\\Ptwo".into(),
            plain: "one\ntwo".into(),
            style: None,
            attachment: Attachment::TopLeft,
            line_spacing: 1.0,
            normal: Vec3::Z,
        });
        let s = svg(vec![t]);
        assert_eq!(s.matches("<tspan").count(), 2, "{s}");
        assert!(s.contains("dominant-baseline=\"hanging\"") && s.contains("text-anchor=\"start\""));
        assert!(s.contains(">one</tspan>") && s.contains(">two</tspan>"));
    }

    #[test]
    fn insert_expanded_or_marked() {
        let blk = Block {
            name: "B".into(),
            entities: vec![line(0.0, 0.0, 1.0, 0.0)],
            ..Block::default()
        };
        let ins = Entity::new(EntityKind::Insert {
            block: "B".into(),
            position: Point3::xy(10.0, 10.0),
            scale: Vec3::new(2.0, 2.0, 1.0),
            rotation: 0.0,
            normal: Vec3::Z,
            columns: 2,
            rows: 1,
            column_spacing: 5.0,
            row_spacing: 0.0,
        });
        let mut d = doc(vec![ins]);
        d.blocks.push(blk);
        let s = render(&d, &SvgOptions::default());
        assert!(s.contains("M10 -10L12 -10"), "{s}");
        assert!(s.contains("M15 -10L17 -10"), "{s}");
        let o = SvgOptions {
            expand_blocks: false,
            ..SvgOptions::default()
        };
        let s = render(&d, &o);
        assert!(!s.contains("L12 -10"));
        assert_eq!(s.matches("<path").count(), 1);
    }

    #[test]
    fn insert_cycle_terminates() {
        let a = Block {
            name: "A".into(),
            entities: vec![
                line(0.0, 0.0, 1.0, 1.0),
                Entity::new(EntityKind::Insert {
                    block: "A".into(),
                    position: Point3::xy(1.0, 1.0),
                    scale: Vec3::new(1.0, 1.0, 1.0),
                    rotation: 0.0,
                    normal: Vec3::Z,
                    columns: 1,
                    rows: 1,
                    column_spacing: 0.0,
                    row_spacing: 0.0,
                }),
            ],
            ..Block::default()
        };
        let ins = Entity::new(EntityKind::Insert {
            block: "A".into(),
            position: Point3::xy(0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            normal: Vec3::Z,
            columns: 1,
            rows: 1,
            column_spacing: 0.0,
            row_spacing: 0.0,
        });
        let mut d = doc(vec![ins]);
        d.blocks.push(a);
        let s = render(&d, &SvgOptions::default());
        assert!(s.contains("<path"));
    }

    #[test]
    fn group_children_rendered() {
        let g = Entity::new(EntityKind::Group {
            group_kind: GroupKind::Cell,
            name: None,
            origin: None,
            children: vec![line(0.0, 0.0, 1.0, 1.0), line(1.0, 1.0, 2.0, 0.0)],
        });
        assert_eq!(svg(vec![g]).matches("<path").count(), 2);
    }

    #[test]
    fn hatch_solid_and_pattern() {
        let square = HatchLoop {
            edges: vec![
                HatchEdge::Line {
                    start: Point3::xy(0.0, 0.0),
                    end: Point3::xy(4.0, 0.0),
                },
                HatchEdge::Line {
                    start: Point3::xy(4.0, 0.0),
                    end: Point3::xy(4.0, 4.0),
                },
                HatchEdge::Line {
                    start: Point3::xy(4.0, 4.0),
                    end: Point3::xy(0.0, 4.0),
                },
                HatchEdge::Line {
                    start: Point3::xy(0.0, 4.0),
                    end: Point3::xy(0.0, 0.0),
                },
            ],
            external: true,
        };
        let hole = HatchLoop {
            edges: vec![HatchEdge::Arc {
                center: Point3::xy(2.0, 2.0),
                radius: 1.0,
                start_angle: 0.0,
                end_angle: TAU,
                ccw: true,
            }],
            external: false,
        };
        let mk = |solid: bool| {
            Entity::new(EntityKind::Hatch {
                loops: vec![square.clone(), hole.clone()],
                solid,
                pattern: if solid { None } else { Some("ANSI31".into()) },
                pattern_scale: 1.0,
                pattern_angle: 0.0,
                normal: Vec3::Z,
            })
        };
        let s = svg(vec![mk(true)]);
        assert!(
            s.contains("fill-rule=\"evenodd\"") && s.contains("fill=\"#000000\""),
            "{s}"
        );
        assert!(
            s.contains("M0 0L4 0L4 -4L0 -4L0 0Z") || s.contains("M0 0L4 0L4 -4L0 -4Z"),
            "{s}"
        );
        assert_eq!(s.matches("<path").count(), 1);
        let s = svg(vec![mk(false)]);
        assert!(
            s.contains("fill-rule=\"evenodd\"")
                && s.contains("vector-effect")
                && !s.contains("fill=\"#000000\""),
            "{s}"
        );
    }

    #[test]
    fn face_leader_dimension_image_mesh_point() {
        let face = Entity::new(EntityKind::Face {
            points: vec![
                Point3::xy(0.0, 0.0),
                Point3::xy(1.0, 0.0),
                Point3::xy(1.0, 1.0),
                Point3::xy(1.0, 1.0),
            ],
            filled: true,
        });
        let leader = Entity::new(EntityKind::Leader {
            vertices: vec![Point3::xy(0.0, 0.0), Point3::xy(2.0, 2.0)],
            arrowhead: true,
        });
        let dim = Entity::new(EntityKind::Dimension {
            dimension_type: crate::model::DimensionType::Linear,
            points: vec![Point3::xy(0.0, 0.0), Point3::xy(3.0, 0.0)],
            text_position: None,
            measurement: None,
            text: None,
            block: Some("missing".into()),
            style: None,
        });
        let image = Entity::new(EntityKind::Image {
            path: None,
            position: Point3::xy(0.0, 0.0),
            u_vector: Vec3::new(2.0, 0.0, 0.0),
            v_vector: Vec3::new(0.0, 1.0, 0.0),
            size_px: None,
        });
        let mesh = Entity::new(EntityKind::Mesh {
            vertices: vec![
                Point3::xy(0.0, 0.0),
                Point3::xy(1.0, 0.0),
                Point3::xy(0.0, 1.0),
            ],
            faces: vec![vec![0, 1, 2]],
        });
        let point = Entity::new(EntityKind::Point {
            position: Point3::xy(5.0, 5.0),
        });
        let vp = Entity::new(EntityKind::Viewport {
            center: Point3::default(),
            width: 1.0,
            height: 1.0,
            view_center: Point3::default(),
            view_height: 1.0,
        });
        let unk = Entity::new(EntityKind::Unknown {
            type_name: "dwg.X".into(),
        });
        let s = svg(vec![face, leader, dim, image, mesh, point, vp, unk]);
        assert_eq!(s.matches("<path").count(), 6, "{s}");
        assert!(s.contains("stroke-dasharray=\"4 3\""));
        assert!(s.contains("M0 0L1 0L1 -1Z"));
    }

    #[test]
    fn non_finite_geometry_does_not_break_output() {
        let s = svg(vec![
            line(f64::NAN, 0.0, 1.0, f64::INFINITY),
            line(0.0, 0.0, 1.0, 1.0),
        ]);
        assert!(!s.contains("NaN") && !s.contains("inf"), "{s}");
    }
}
