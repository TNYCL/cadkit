//! 2D SVG rendering of a [`Document`] (top view, XY plane).
//!
//! Conventions and limits:
//! - The view looks down the Z axis; Y is flipped so the drawing is upright. Z is ignored.
//! - The `viewBox` is the model bounding box ([`Document::bbox`]) plus a 2 % margin, in
//!   drawing units; `width` comes from [`SvgOptions::width_px`] and `height` follows the
//!   aspect ratio.
//! - Every element carries `vector-effect="non-scaling-stroke"`, so [`SvgOptions::stroke_px`]
//!   is a screen width regardless of the zoom. Explicit [`Lineweight::Millimeters`] values scale
//!   the stroke proportionally around the CAD `0.25 mm` baseline, and [`Linetype`] dash patterns
//!   emit screen-space `stroke-dasharray` attributes. Polyline vertex widths are ignored.
//! - Non-solid [`EntityKind::Hatch`] entities emit `<defs><pattern>` line fills, [`EntityKind::Leader`]
//!   entities with `arrowhead = true` render tip arrowheads, constrained [`EntityKind::MText`] boxes
//!   word-wrap by width, oblique [`EntityKind::Text`] entities apply `skewX`, and visible entity
//!   [`Attribute`](crate::model::Attribute) values with display geometry render as `<text>`.
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
//! - [`render_bounded`] instead rejects incomplete output. Its byte cap includes the
//!   complete UTF-8 SVG and limits intermediate SVG buffers before they grow.
//! - Output is deterministic: no hash-order or time dependence, numbers use a fixed
//!   precision derived from the drawing extent with trailing zeros removed.
//!
//! Options for interactive viewers. All are off by default, and with default options the
//! output is byte-for-byte what earlier releases wrote:
//! - [`SvgOptions::origin`] writes every coordinate relative to a local origin and states it
//!   on the root element (`data-origin="ox oy"`; SVG point `(sx, sy)` is drawing point
//!   `(ox + sx, oy - sy)`), so map coordinates survive the 32-bit floats browsers use. The
//!   number precision follows the drawing extent either way.
//! - [`SvgOptions::entity_refs`] tags each element with `data-e="i.j"`, the path of the
//!   model entity it belongs to (geometry from a block carries the insert's path), records
//!   colours replaced for contrast as `data-c`, and image frames' path and pixel size.
//!   A page can then highlight or hide an entity with a CSS selector. It costs about 9 bytes
//!   plus the path digits per element: measured +8.6 % on `cadkit-sample-site.dxf`,
//!   +16.7 % on `cadkit-sample-contours.dxf`, +5.1 % to +32 % on public DWG/DGN samples.
//! - [`SvgOptions::point_px`] draws points as dots of a fixed pixel diameter instead of
//!   crosses whose arms are 0.4 % of the drawing extents.
//! - [`SvgOptions::cap_height_ratio`] and [`SvgOptions::font_family`] size text by its cap
//!   height (CAD text height) instead of using it as the em size, and pick the font.
//! - [`SvgOptions::hatch_native_spacing`] spaces pattern hatches by their pattern scale
//!   instead of a fraction of the drawing extents.
//! - [`SvgOptions::layout_viewports`] draws a paper layout's viewports as clipped windows onto
//!   model space: `<g id="cadkit-model" data-model="m">` in `<defs>` (with its own
//!   `data-origin` when an origin is set, `data-e` paths into model `m`), one
//!   `<g clip-path="..." data-scale="s"><use href="#cadkit-model" transform="matrix(..)"/></g>`
//!   per viewport (`s`: model units per sheet unit) and a frame `<path>`, all inside
//!   `<g data-kind="viewports">` before the sheet's own layers. The sheet extents include the
//!   viewport rectangles.

use std::borrow::Cow;
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

mod buffer;
use buffer::{Budget, Buffer};

#[cfg(test)]
mod bounded_tests;

use crate::geom::{BBox, Point3, Vec3};
use crate::geom_ops::{EllipseArc, Transform, ccw_sweep, conjugate_to_axes};
use crate::model::{
    Block, Color, Document, Entity, EntityKind, HAlign, HatchEdge, Layer, Linetype, Lineweight,
    ModelKind, VAlign, Value, Vertex,
};
use crate::ops::{
    BBoxOptions, DEFAULT_MAX_WORK, DEFAULT_WALK_DEPTH, WalkContext, WalkOptions, attachment_anchor,
    circle_arc, full_ellipse_arc, mtext_line_height, mtext_text_lines, segment_arc, spline_points,
};

/// SVG rendering options.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[non_exhaustive]
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
    /// [`render_bounded`] uses this as the complete UTF-8 output and temporary-buffer
    /// budget instead, returning an error on exhaustion rather than partial output.
    pub max_output_bytes: usize,
    /// Point that emitted coordinates are written relative to (see [`SvgOrigin`]).
    /// Default [`SvgOrigin::None`]: coordinates are written as they are.
    pub origin: SvgOrigin,
    /// Tag every drawn element with the model entity it belongs to: `data-e="i.j.k"`,
    /// the entity's [`WalkContext::path`] cut to [`WalkContext::model_path_len`] and joined
    /// with dots (resolve it with [`Document::entity_at`]). Geometry drawn from a block
    /// carries the path of the outermost insert. Elements whose colour was replaced for
    /// contrast also carry the resolved original colour as `data-c="#rrggbb"`, and image
    /// frames carry `data-image-path` (the stored path) and `data-image-px="w h"` (pixel
    /// size) when known. Default `false`.
    pub entity_refs: bool,
    /// Draw `Point` entities as round dots of this diameter in CSS pixels (a zero-length
    /// sub-path with round caps and a non-scaling stroke of this width) instead of crosses
    /// sized from the drawing extents. Values that are not finite and positive keep the
    /// crosses; values above 1000 are clamped. Default `None`.
    pub point_px: Option<f64>,
    /// Ratio of cap height to font size of the display font. CAD text height is the cap
    /// height, so `font-size` becomes `height / ratio` (about `0.7` suits common sans-serif
    /// fonts). Applies to text, fit text and attribute text; aligned text keeps the size that
    /// makes it span its baseline. Values outside `0.05..=20` (or not finite) are ignored.
    /// Default `None`: `font-size` is the CAD height.
    pub cap_height_ratio: Option<f64>,
    /// CSS `font-family` written on the root element, inherited by all text. An empty string
    /// writes nothing. Default `None`.
    pub font_family: Option<String>,
    /// Space non-solid hatch pattern lines by the pattern's own spacing (`3.175` drawing units
    /// times the pattern scale) instead of clamping the spacing to 0.8 %..8 % of the drawing
    /// extents. The spacing is raised where needed so that at most 500 lines cross the larger
    /// side of the hatch's own extents (and to at least 1e-5 of the drawing extents).
    /// Default `false`.
    pub hatch_native_spacing: bool,
    /// When the rendered model is a paper layout (any kind other than
    /// [`ModelKind::Model`](crate::model::ModelKind::Model)), draw each of its
    /// `Viewport` entities as a window onto the first model of kind `Model`: the model is
    /// rendered once into `<defs>` and shown in each viewport's sheet rectangle, clipped,
    /// scaled by `height / view_height` and centred on `view_center`, with a frame. The first
    /// viewport is skipped when it shows the sheet itself (`view_height == height` and
    /// `view_center == center`, as in DWG/DXF). Default `false`.
    pub layout_viewports: bool,
}

/// Where emitted SVG coordinates are measured from ([`SvgOptions::origin`]).
///
/// Browsers process SVG coordinates in 32-bit floats, which cannot hold map coordinates
/// (`x ≈ 500 000`, `y ≈ 4 000 000`) to the precision of a drawing a few units across.
/// With an origin `(ox, oy)` in drawing units, every coordinate (paths, text positions,
/// rotation centres, the background rectangle, the `viewBox`, image frames) is written
/// relative to it, and the root element states it as `data-origin="ox oy"` (shortest
/// round-trip decimal form). An SVG user-space point `(sx, sy)` is the drawing point
/// `(ox + sx, oy - sy)`: X is shifted, Y is shifted and flipped. Hatch pattern tiles are
/// pattern-local and unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SvgOrigin {
    /// Coordinates are written as they are (`sx = x`, `sy = -y`); no `data-origin`.
    #[default]
    None,
    /// A round number near the centre of the drawing extents: the centre rounded to a
    /// multiple of the power of ten at or above the larger extent (`0 0` for a drawing
    /// around the origin or without geometry).
    Auto,
    /// An explicit origin in drawing units. A non-finite coordinate falls back to [`Self::Auto`].
    Explicit {
        /// Origin X in drawing units.
        x: f64,
        /// Origin Y in drawing units.
        y: f64,
    },
}

impl SvgOptions {
    /// Creates default SVG options (white background, `1600px` wide, `1px` stroke).
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects the model index in [`Document::models`].
    pub fn with_model(mut self, model: usize) -> Self {
        self.model = model;
        self
    }

    /// Sets [`SvgOptions::width_px`].
    pub fn with_width_px(mut self, width_px: f64) -> Self {
        self.width_px = width_px;
        self
    }

    /// Sets [`SvgOptions::stroke_px`].
    pub fn with_stroke_px(mut self, stroke_px: f64) -> Self {
        self.stroke_px = stroke_px;
        self
    }

    /// Sets a CSS background color (or `None` for transparent).
    pub fn with_background(mut self, background: Option<impl Into<String>>) -> Self {
        self.background = background.map(Into::into);
        self
    }

    /// Convenience preset for a transparent background (`background = None`).
    pub fn transparent(mut self) -> Self {
        self.background = None;
        self
    }

    /// Convenience preset for a dark CAD viewport background (`#12161c`).
    pub fn dark(mut self) -> Self {
        self.background = Some("#12161c".to_owned());
        self
    }

    /// Toggles text rendering.
    pub fn with_text(mut self, text: bool) -> Self {
        self.text = text;
        self
    }

    /// Toggles block-reference (`Insert`) expansion.
    pub fn with_expand_blocks(mut self, expand_blocks: bool) -> Self {
        self.expand_blocks = expand_blocks;
        self
    }

    /// Toggles raster image frame rendering.
    pub fn with_images(mut self, images: bool) -> Self {
        self.images = images;
        self
    }

    /// Sets [`SvgOptions::max_work`].
    pub fn with_max_work(mut self, max_work: u64) -> Self {
        self.max_work = max_work;
        self
    }

    /// Sets [`SvgOptions::max_output_bytes`].
    pub fn with_max_output_bytes(mut self, max_output_bytes: usize) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }

    /// Sets [`SvgOptions::origin`].
    pub fn with_origin(mut self, origin: SvgOrigin) -> Self {
        self.origin = origin;
        self
    }

    /// Toggles [`SvgOptions::entity_refs`].
    pub fn with_entity_refs(mut self, entity_refs: bool) -> Self {
        self.entity_refs = entity_refs;
        self
    }

    /// Sets [`SvgOptions::point_px`] (dot diameter in CSS pixels, `None` for crosses).
    pub fn with_point_px(mut self, point_px: Option<f64>) -> Self {
        self.point_px = point_px;
        self
    }

    /// Sets [`SvgOptions::cap_height_ratio`].
    pub fn with_cap_height_ratio(mut self, cap_height_ratio: Option<f64>) -> Self {
        self.cap_height_ratio = cap_height_ratio;
        self
    }

    /// Sets [`SvgOptions::font_family`] (a CSS font-family list, `None` for none).
    pub fn with_font_family(mut self, font_family: Option<impl Into<String>>) -> Self {
        self.font_family = font_family.map(Into::into);
        self
    }

    /// Toggles [`SvgOptions::hatch_native_spacing`].
    pub fn with_hatch_native_spacing(mut self, hatch_native_spacing: bool) -> Self {
        self.hatch_native_spacing = hatch_native_spacing;
        self
    }

    /// Toggles [`SvgOptions::layout_viewports`].
    pub fn with_layout_viewports(mut self, layout_viewports: bool) -> Self {
        self.layout_viewports = layout_viewports;
        self
    }
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
            origin: SvgOrigin::None,
            entity_refs: false,
            point_px: None,
            cap_height_ratio: None,
            font_family: None,
            hatch_native_spacing: false,
            layout_viewports: false,
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

/// The round number near `centre` used by [`SvgOrigin::Auto`]: a multiple of the power of
/// ten at or above `extent`, decimal-exact so that `data-origin` prints it as written.
fn auto_origin_axis(centre: f64, extent: f64) -> f64 {
    if !centre.is_finite() {
        return 0.0;
    }
    let exp = if extent.is_finite() && extent > 0.0 {
        extent.log10().ceil().clamp(-12.0, 300.0)
    } else {
        0.0
    };
    let step = 10f64.powf(exp);
    let snapped = (centre / step).round() * step;
    if !snapped.is_finite() {
        return 0.0;
    }
    // A step below 1 is not exact in binary; reparse the decimal so the value is the one printed.
    let decimals = (-exp).max(0.0) as usize;
    let value = format!("{snapped:.decimals$}")
        .parse::<f64>()
        .unwrap_or(0.0);
    if value == 0.0 { 0.0 } else { value }
}

/// The origin [`SvgOptions::origin`] asks for, given the extents to render; `None` when the
/// coordinates are written as they are.
fn resolve_origin(origin: SvgOrigin, bb: Option<&BBox>) -> Option<(f64, f64)> {
    match origin {
        SvgOrigin::None => None,
        SvgOrigin::Explicit { x, y } if x.is_finite() && y.is_finite() => Some((x, y)),
        _ => Some(bb.map_or((0.0, 0.0), |bb| {
            let extent = bb.width().max(bb.height());
            let c = bb.center();
            (auto_origin_axis(c.x, extent), auto_origin_axis(c.y, extent))
        })),
    }
}

/// Number formatted with about `digits` significant digits (scale factors, which are not
/// tied to the drawing extent).
fn fmt_sig(v: f64, digits: i32) -> String {
    let dec = if v.is_finite() && v != 0.0 {
        (f64::from(digits) - 1.0 - v.abs().log10().floor()).clamp(0.0, 17.0)
    } else {
        0.0
    };
    fmt_num(v, dec as usize)
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
        "white" | "transparent" | "none" => return (255, 255, 255),
        "gray" | "grey" => return (128, 128, 128),
        "dimgray" | "dimgrey" => return (105, 105, 105),
        "darkgray" | "darkgrey" => return (169, 169, 169),
        "silver" => return (192, 192, 192),
        "lightgray" | "lightgrey" => return (211, 211, 211),
        "navy" => return (0, 0, 128),
        "midnightblue" => return (25, 25, 112),
        "darkslategray" | "darkslategrey" => return (47, 79, 79),
        _ => {}
    }
    if let Some(inner) = b
        .strip_prefix("rgb(")
        .or_else(|| b.strip_prefix("rgba("))
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<u8> = inner
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .take(3)
            .filter_map(|s| s.parse::<f64>().ok())
            .map(|v| v.round().clamp(0.0, 255.0) as u8)
            .collect();
        if let [r, g, bl] = parts.as_slice() {
            return (*r, *g, *bl);
        }
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

/// Most pattern lines across a hatch with [`SvgOptions::hatch_native_spacing`].
const MAX_HATCH_LINES: f64 = 500.0;

/// Minimum WCAG contrast ratio between a stroke and the background.
const MIN_CONTRAST: f64 = 1.25;

/// `color`, or black / white when its WCAG contrast ratio against the background `bg` is
/// below 1.25 (black on light, white on dark backgrounds). The SVG export applies it to every
/// stroke and fill; renderers that draw on a known background can apply the same rule.
pub fn with_contrast(color: (u8, u8, u8), bg: (u8, u8, u8)) -> (u8, u8, u8) {
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

#[cfg(test)]
fn escape(s: &str) -> String {
    escape_bounded(s, &Budget::default()).into_string()
}

fn escape_bounded(s: &str, budget: &Budget) -> Buffer {
    let mut out = budget.buffer();
    for c in s.chars() {
        if budget.exceeded() {
            break;
        }
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
    d: Buffer,
    cur: Option<(f64, f64)>,
}

impl<'f> PathBuilder<'f> {
    fn new(fmt: &'f Fmt, budget: &Budget) -> Self {
        Self {
            fmt,
            d: budget.buffer(),
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
            if self.d.exceeded() {
                break;
            }
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
            if self.d.exceeded() {
                break;
            }
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
    budget: Budget,
    stroke_px: f64,
}

impl Layers {
    fn push_image(&mut self, text: &str) {
        if !self.reserve(text.len()) {
            return;
        }
        self.images.push_str(text);
    }

    fn reserve(&mut self, bytes: usize) -> bool {
        let next = self.bytes.saturating_add(bytes);
        if !self.budget.allows(next) {
            return false;
        }
        self.bytes = next;
        true
    }

    fn push(&mut self, name: &str, text: &str) {
        if !self.reserve(text.len()) || !self.budget.allows(name.len()) {
            return;
        }
        if let Some(&i) = self.index.get(name) {
            if let Some((_, body)) = self.items.get_mut(i) {
                body.push_str(text);
            }
            return;
        }
        if !self.budget.is_unlimited() {
            let opening = layer_open(name, self.stroke_px, &self.budget);
            if !self.reserve(opening.len().saturating_add("</g>\n".len())) {
                return;
            }
        }
        self.index.insert(name.to_owned(), self.items.len());
        self.items.push((name.to_owned(), text.to_owned()));
    }
}

fn layer_open(name: &str, stroke: f64, budget: &Budget) -> Buffer {
    budget.format(format_args!(
        "<g data-layer=\"{}\" fill=\"none\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n",
        escape_bounded(name, budget), fmt_num(stroke, 3)
    ))
}

fn wrap_mtext_lines(lines: Vec<String>, width: f64, height: f64) -> Vec<String> {
    if !(width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
        return lines;
    }
    let max_chars = ((width / (0.6 * height)).floor() as usize).max(1);
    let mut wrapped = Vec::with_capacity(lines.len());
    for line in lines {
        if line.chars().count() <= max_chars {
            wrapped.push(line);
            continue;
        }
        let mut current = String::new();
        let mut cur_len = 0usize;
        for word in line.split_whitespace() {
            let wlen = word.chars().count();
            if cur_len == 0 {
                current.push_str(word);
                cur_len = wlen;
            } else if cur_len + 1 + wlen <= max_chars {
                current.push(' ');
                current.push_str(word);
                cur_len += 1 + wlen;
            } else {
                wrapped.push(std::mem::take(&mut current));
                current.push_str(word);
                cur_len = wlen;
            }
        }
        if !current.is_empty() || line.is_empty() {
            wrapped.push(current);
        }
    }
    wrapped
}

fn dash_array_px(
    pattern: &[f64],
    px_per_unit: f64,
    stroke_px: f64,
    budget: &Budget,
) -> Option<String> {
    if !budget.allows(pattern.len().saturating_mul(2 * std::mem::size_of::<f64>())) {
        return None;
    }
    if pattern.is_empty() || !pattern.iter().any(|&v| v.is_finite() && v < -1e-12) {
        return None;
    }
    let unit_cycle: f64 = pattern
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .map(f64::abs)
        .sum();
    if unit_cycle <= 1e-12 {
        return None;
    }
    let raw_cycle_px = unit_cycle * px_per_unit;
    let min_cycle = 6.0 * stroke_px;
    let max_cycle = 160.0 * stroke_px;
    let scale = if raw_cycle_px.is_finite() && raw_cycle_px > 0.0 {
        raw_cycle_px.clamp(min_cycle, max_cycle) / unit_cycle
    } else {
        12.0 * stroke_px / unit_cycle
    };

    let mut pairs: Vec<f64> = Vec::with_capacity(pattern.len().saturating_mul(2));
    let mut expect_dash = true;
    for &seg in pattern {
        if !seg.is_finite() {
            continue;
        }
        if seg >= -1e-12 {
            let len = if seg <= 1e-12 {
                stroke_px.max(0.5)
            } else {
                (seg * scale).max(stroke_px)
            };
            if expect_dash {
                pairs.push(len);
                expect_dash = false;
            } else {
                pairs.push(stroke_px);
                pairs.push(len);
            }
        } else {
            let gap = ((-seg) * scale).max(stroke_px);
            if !expect_dash {
                pairs.push(gap);
                expect_dash = true;
            } else if let Some(last) = pairs.last_mut() {
                *last += gap;
            } else {
                pairs.push(0.0);
                pairs.push(gap);
            }
        }
    }
    if pairs.len() < 2 {
        return None;
    }
    let mut formatted = budget.buffer();
    for (i, value) in pairs.into_iter().enumerate() {
        if budget.exceeded() {
            break;
        }
        if i > 0 {
            formatted.push(' ');
        }
        formatted.push_str(&fmt_num(value, 2));
    }
    Some(formatted.into_string())
}

struct Renderer<'a> {
    doc: &'a Document,
    opts: &'a SvgOptions,
    include_hidden_layers: bool,
    fmt: Fmt,
    view: Transform,
    cross: f64,
    extent: f64,
    px_per_unit: f64,
    stroke_px: f64,
    layers: Layers,
    defs: Vec<String>,
    hatch_cache: HashMap<String, String>,
    layer_map: HashMap<&'a str, &'a Layer>,
    linetype_map: HashMap<String, &'a Linetype>,
    block_map: HashMap<&'a str, &'a Block>,
    background: (u8, u8, u8),
    budget: Budget,
    attribute_work: u64,
    work_exceeded: bool,
    /// ` data-e="..."` of the entity being drawn, empty unless [`SvgOptions::entity_refs`].
    cur_ref: String,
    /// ` data-c="#rrggbb"` when the colour being drawn replaced the entity's own for contrast.
    cur_orig: String,
    /// Valid [`SvgOptions::point_px`].
    point_px: Option<f64>,
    /// Valid [`SvgOptions::cap_height_ratio`], else 1.
    cap_ratio: f64,
}

/// Placement of one rendered space: the selected model, or model space shown in viewports.
struct SpaceView {
    /// Local origin subtracted from drawing coordinates.
    origin: Option<(f64, f64)>,
    /// Extent in drawing units that sets number precision, point crosses and hatch clamps.
    extent: f64,
    /// CSS pixels per drawing unit, for screen-space dash patterns.
    px_per_unit: f64,
}

impl<'a> Renderer<'a> {
    /// `None` when the linetype table alone exceeds the output budget.
    fn new(
        doc: &'a Document,
        opts: &'a SvgOptions,
        include_hidden_layers: bool,
        budget: &Budget,
        stroke: f64,
        space: &SpaceView,
    ) -> Option<Self> {
        let mut layer_map = HashMap::with_capacity(doc.layers.len());
        for l in &doc.layers {
            layer_map.entry(l.name.as_str()).or_insert(l);
        }
        let mut linetype_map = HashMap::with_capacity(doc.linetypes.len());
        let mut linetype_bytes = 0usize;
        for lt in &doc.linetypes {
            linetype_bytes = linetype_bytes.saturating_add(lt.name.len());
            if !budget.allows(linetype_bytes) {
                return None;
            }
            linetype_map
                .entry(lt.name.to_ascii_lowercase())
                .or_insert(lt);
        }
        let mut block_map = HashMap::with_capacity(doc.blocks.len());
        for b in &doc.blocks {
            block_map.entry(b.name.as_str()).or_insert(b);
        }
        let flip = Transform::scaling(1.0, -1.0, 1.0);
        let view = match space.origin {
            None => flip,
            Some((ox, oy)) => flip.compose(&Transform::translation(Vec3::new(-ox, -oy, 0.0))),
        };
        Some(Self {
            doc,
            opts,
            include_hidden_layers,
            fmt: Fmt::for_extent(space.extent),
            view,
            cross: space.extent * 0.004,
            extent: space.extent,
            px_per_unit: space.px_per_unit,
            stroke_px: stroke,
            layers: Layers {
                budget: budget.clone(),
                stroke_px: stroke,
                ..Layers::default()
            },
            defs: Vec::new(),
            hatch_cache: HashMap::new(),
            layer_map,
            linetype_map,
            block_map,
            background: background_rgb(opts.background.as_deref()),
            budget: budget.clone(),
            attribute_work: opts.max_work,
            work_exceeded: false,
            cur_ref: String::new(),
            cur_orig: String::new(),
            point_px: opts
                .point_px
                .filter(|d| d.is_finite() && *d > 0.0)
                .map(|d| d.min(1000.0)),
            cap_ratio: opts
                .cap_height_ratio
                .filter(|r| r.is_finite() && (0.05..=20.0).contains(r))
                .unwrap_or(1.0),
        })
    }

    /// Draws `models[model]` with a work budget of `max_work`, stopping once the body exceeds
    /// `cap` bytes. Returns whether the drawing was cut short and the walk's work.
    fn draw(&mut self, model: usize, max_work: u64, cap: usize) -> (bool, u64) {
        self.attribute_work = max_work;
        let walk_opts = WalkOptions {
            max_depth: DEFAULT_WALK_DEPTH,
            expand_inserts: self.opts.expand_blocks,
            max_work,
        };
        let doc = self.doc;
        let budget = self.budget.clone();
        let include_hidden_layers = self.include_hidden_layers;
        let visit = |e: &'a Entity, ctx: &WalkContext<'a>| {
            if self.layers.bytes > cap || budget.exceeded() || self.work_exceeded {
                return false;
            }
            self.render_entity(e, ctx);
            true
        };
        let stats = if include_hidden_layers {
            doc.walk_all_layers_while(model, &walk_opts, visit)
        } else {
            doc.walk_while(model, &walk_opts, visit)
        };
        let mut truncated = stats.truncated;
        if !truncated
            && !budget.exceeded()
            && self.opts.expand_blocks
            && self.opts.text
            && !doc.blocks.is_empty()
            && let Some(m) = doc.models.get(model)
        {
            let mut stack = Vec::new();
            let mut path = Vec::new();
            self.render_expanded_insert_attributes(
                &m.entities,
                &WalkContext::root(),
                &mut stack,
                cap,
                &mut path,
                false,
            );
            if self.layers.bytes > cap || budget.exceeded() {
                truncated = true;
            }
        }
        (truncated || self.work_exceeded, stats.work)
    }

    /// Sets the `data-e` reference of the entity drawn next (a no-op unless enabled).
    fn set_ref(&mut self, path: &[usize]) {
        self.cur_ref.clear();
        if !self.opts.entity_refs || !self.budget.allows(path.len().saturating_mul(21)) {
            return;
        }
        self.cur_ref.push_str(" data-e=\"");
        for (i, index) in path.iter().enumerate() {
            if i > 0 {
                self.cur_ref.push('.');
            }
            self.cur_ref.push_str(&index.to_string());
        }
        self.cur_ref.push('"');
    }

    /// Contrast-adjusted `color`, remembering the original for `data-c` when it was replaced.
    fn display_color(&mut self, color: (u8, u8, u8)) -> (u8, u8, u8) {
        let shown = with_contrast(color, self.background);
        self.cur_orig.clear();
        if self.opts.entity_refs && shown != color {
            self.cur_orig.push_str(" data-c=\"");
            self.cur_orig.push_str(&hex(color));
            self.cur_orig.push('"');
        }
        shown
    }

    /// Reference attributes written right after an element name (empty by default).
    fn refs(&self) -> String {
        if self.cur_ref.is_empty() && self.cur_orig.is_empty() {
            return String::new();
        }
        self.budget
            .format(format_args!("{}{}", self.cur_ref, self.cur_orig))
            .into_string()
    }

    fn stroke_base(color: (u8, u8, u8)) -> String {
        format!(
            " stroke=\"{}\" vector-effect=\"non-scaling-stroke\"",
            hex(color)
        )
    }

    fn resolved_stroke_width(&self, e: &Entity, ctx: &WalkContext<'_>) -> Option<f64> {
        let mm = match e.lineweight {
            Lineweight::Millimeters(mm) => Some(mm),
            Lineweight::ByLayer => {
                let lname = ctx.layer_name(e);
                match self.layer_map.get(lname).map(|l| l.lineweight) {
                    Some(Lineweight::Millimeters(mm)) => Some(mm),
                    _ => None,
                }
            }
            Lineweight::ByBlock | Lineweight::Default => None,
        }?;
        if !(mm.is_finite() && mm >= 0.0) {
            return None;
        }
        let w = if mm <= 1e-6 {
            0.5 * self.stroke_px
        } else {
            (self.stroke_px * (mm / 0.25)).clamp(0.4 * self.stroke_px, 6.0 * self.stroke_px)
        };
        if (w - self.stroke_px).abs() > 1e-3 {
            Some(w)
        } else {
            None
        }
    }

    fn resolved_dasharray(&self, e: &Entity, ctx: &WalkContext<'_>) -> Option<String> {
        let raw_name = match e.linetype.as_deref() {
            Some(name) if !name.is_empty() && !name.eq_ignore_ascii_case("BYLAYER") => Some(name),
            _ => self
                .layer_map
                .get(ctx.layer_name(e))
                .and_then(|l| l.linetype.as_deref()),
        }?;
        if raw_name.eq_ignore_ascii_case("CONTINUOUS") || raw_name.eq_ignore_ascii_case("BYBLOCK") {
            return None;
        }
        if !self.budget.allows(raw_name.len()) {
            return None;
        }
        let lt = self.linetype_map.get(&raw_name.to_ascii_lowercase())?;
        dash_array_px(&lt.pattern, self.px_per_unit, self.stroke_px, &self.budget)
    }

    fn stroke_attrs(&self, e: &Entity, ctx: &WalkContext<'_>, color: (u8, u8, u8)) -> String {
        let mut s = self
            .budget
            .format(format_args!("{}", Self::stroke_base(color)));
        if let Some(w) = self.resolved_stroke_width(e, ctx) {
            s.push_str(
                &self
                    .budget
                    .format(format_args!(" stroke-width=\"{}\"", fmt_num(w, 3))),
            );
        }
        if let Some(dash) = self.resolved_dasharray(e, ctx) {
            s.push_str(
                &self
                    .budget
                    .format(format_args!(" stroke-dasharray=\"{dash}\"")),
            );
        }
        s.into_string()
    }

    /// `own_extent` is the larger side of the hatch's extents, in drawing units.
    fn hatch_pattern_id(
        &mut self,
        pattern: Option<&str>,
        (pattern_scale, pattern_angle): (f64, f64),
        own_extent: f64,
        color: (u8, u8, u8),
    ) -> String {
        if !self.budget.allows(pattern.map_or(0, str::len)) {
            return String::new();
        }
        let pat_upper = pattern.unwrap_or("ANSI31").to_ascii_uppercase();
        let base_angle = if pat_upper.contains("31")
            || pat_upper.contains("32")
            || pat_upper.contains("33")
            || pat_upper.contains("37")
            || pat_upper.contains("SLASH")
        {
            PI / 4.0
        } else {
            0.0
        };
        let cross_hatch = pat_upper.contains("37")
            || pat_upper.contains("NET")
            || pat_upper.contains("CROSS")
            || pat_upper.contains("SQUARE");
        let total_deg = -(base_angle
            + if pattern_angle.is_finite() {
                pattern_angle
            } else {
                0.0
            })
        .to_degrees();
        let scale = if pattern_scale.is_finite() && pattern_scale > 0.0 {
            pattern_scale
        } else {
            1.0
        };
        let step = if self.opts.hatch_native_spacing {
            // Bound the tile count: at most MAX_HATCH_LINES lines across the hatch, and never
            // finer than the number formatter can tell apart.
            let own = if own_extent.is_finite() && own_extent > 0.0 {
                own_extent
            } else {
                self.extent
            };
            let floor = (own / MAX_HATCH_LINES).max(self.extent * 1e-5).max(1e-9);
            let native = 3.175 * scale;
            if native.is_finite() {
                native.max(floor)
            } else {
                self.extent.max(floor)
            }
        } else {
            let min_step = (self.extent * 0.008).max(1e-6);
            let max_step = (self.extent * 0.08).max(min_step);
            (3.175 * scale).clamp(min_step, max_step)
        };
        let step_s = self.fmt.num(step);
        let deg_s = fmt_num(total_deg, 2);
        let c_hex = hex(color);
        let sw_s = fmt_num((self.stroke_px * 0.75).max(0.5), 3);

        let key = self
            .budget
            .format(format_args!("{c_hex}:{step_s}:{deg_s}:{cross_hatch}"));
        if let Some(existing) = self.hatch_cache.get(&*key) {
            return existing.clone();
        }
        let id = self
            .budget
            .format(format_args!("cadkit-hatch-{}", self.defs.len() + 1));
        let rot_attr = if total_deg.abs() > 1e-3 {
            self.budget
                .format(format_args!(" patternTransform=\"rotate({deg_s})\""))
        } else {
            self.budget.buffer()
        };
        let mut def = self.budget.format(format_args!(
            "<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{step_s}\" height=\"{step_s}\"{rot_attr}>"
        ));
        def.push_str(&self.budget.format(format_args!(
            "<line x1=\"0\" y1=\"0\" x2=\"{step_s}\" y2=\"0\" stroke=\"{c_hex}\" stroke-width=\"{sw_s}\" vector-effect=\"non-scaling-stroke\"/>"
        )));
        if cross_hatch {
            def.push_str(&self.budget.format(format_args!(
                "<line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"{step_s}\" stroke=\"{c_hex}\" stroke-width=\"{sw_s}\" vector-effect=\"non-scaling-stroke\"/>"
            )));
        }
        def.push_str("</pattern>\n");
        if self.layers.reserve(def.len()) {
            self.defs.push(def.into_string());
            self.hatch_cache.insert(key.into_string(), id.to_string());
        }
        id.into_string()
    }

    fn emit_path(&mut self, layer: &str, d: &str, extra: &str) {
        if d.is_empty() {
            return;
        }
        let refs = self.refs();
        let line = self
            .budget
            .format(format_args!("<path{refs} d=\"{d}\"{extra}/>\n"));
        self.layers.push(layer, &line);
    }

    fn stroke_path(
        &mut self,
        layer: &str,
        pb: &PathBuilder<'_>,
        e: &Entity,
        ctx: &WalkContext<'_>,
        color: (u8, u8, u8),
    ) {
        let extra = self.stroke_attrs(e, ctx, color);
        self.emit_path(layer, &pb.d, &extra);
    }

    fn layer_hidden(&self, name: &str) -> bool {
        !self.include_hidden_layers
            && self
                .layer_map
                .get(name)
                .is_some_and(|l| !l.visible || l.frozen)
    }

    fn render_attributes(&mut self, e: &Entity, ctx: &WalkContext<'_>) {
        if !self.opts.text || e.attributes.is_empty() {
            return;
        }
        let tf = self.view.compose(&ctx.transform);
        let owner_color = e.resolved_color_in(self.doc, ctx);
        for attr in &e.attributes {
            if self.budget.exceeded() || self.work_exceeded {
                break;
            }
            // Attributes are outside the document walk's entity accounting.
            let cost = match &attr.value {
                Value::Text(value) => 1_u64.saturating_add(value.len() as u64),
                _ => 1,
            };
            if !self.charge_attributes(cost) {
                break;
            }
            if attr.invisible {
                continue;
            }
            let (Some(pos), Some(disp)) = (attr.position, attr.display.as_ref()) else {
                continue;
            };
            if !(disp.height.is_finite() && disp.height > 0.0) {
                continue;
            }
            let text_val: Cow<'_, str> = match &attr.value {
                Value::Text(s) if !s.is_empty() => Cow::Borrowed(s.as_str()),
                Value::Int(n) => Cow::Owned(n.to_string()),
                Value::Float(f) if f.is_finite() => Cow::Owned(fmt_num(*f, 6)),
                Value::Bool(b) => Cow::Borrowed(if *b { "true" } else { "false" }),
                _ => continue,
            };
            let attr_layer = attr.layer.as_deref().unwrap_or_else(|| ctx.layer_name(e));
            if !self.budget.allows(attr_layer.len()) {
                break;
            }
            if self.layer_hidden(attr_layer) {
                continue;
            }
            // The attribute record's own color wins; otherwise the owner's, through the
            // attribute's own layer when the owner's color is ByLayer.
            let attr_color = if let Some(own) = attr.color {
                let mut probe = Entity::new(EntityKind::Point { position: pos });
                probe.layer = Some(attr_layer.to_owned());
                probe.color = own;
                probe.resolved_color_in(self.doc, ctx)
            } else if attr.layer.is_some() && e.color == Color::ByLayer {
                let lc = self
                    .layer_map
                    .get(attr_layer)
                    .map(|l| l.color)
                    .unwrap_or(Color::ByLayer);
                let mut probe = Entity::new(EntityKind::Point { position: pos });
                probe.layer = Some(attr_layer.to_owned());
                probe.color = lc;
                probe.resolved_color(self.doc)
            } else {
                owner_color
            };
            let attr_color = self.display_color(attr_color);
            self.text(
                attr_layer,
                attr_color,
                &tf,
                pos,
                disp.height,
                disp.rotation,
                0.0,
                Vec3::Z,
                TextBody::Single(&text_val),
                disp.halign,
                disp.valign,
            );
        }
    }

    /// `path` is the model path of `list`'s container; it stops growing once `in_block`, so
    /// attributes drawn from a block carry the outermost insert's path (as in the walk).
    fn render_expanded_insert_attributes(
        &mut self,
        list: &'a [Entity],
        ctx: &WalkContext<'a>,
        stack: &mut Vec<&'a str>,
        cap: usize,
        path: &mut Vec<usize>,
        in_block: bool,
    ) {
        let mark = path.len();
        for (index, e) in list.iter().enumerate() {
            if self.layers.bytes > cap || self.budget.exceeded() || !self.charge_attributes(1) {
                return;
            }
            path.truncate(mark);
            if !in_block && self.opts.entity_refs {
                path.push(index);
            }
            let layer = ctx.layer_name(e);
            if !e.visible || self.layer_hidden(layer) {
                continue;
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
                } => {
                    let Some(def) = self.block_map.get(block.as_str()).copied() else {
                        continue;
                    };
                    if ctx.depth >= DEFAULT_WALK_DEPTH || stack.contains(&def.name.as_str()) {
                        continue;
                    }
                    // This Insert was expanded by `walk_while`, so `render_entity` did not see it.
                    self.set_ref(path);
                    self.render_attributes(e, ctx);
                    let child_layer: Option<&'a str> = match e.layer.as_deref() {
                        None | Some("0") => ctx.inherited_layer,
                        Some(l) => Some(l),
                    };
                    let block_color = Some(e.resolved_color_in(self.doc, ctx));
                    stack.push(def.name.as_str());
                    for r in 0..(*rows).max(1) {
                        for c in 0..(*columns).max(1) {
                            if self.layers.bytes > cap
                                || self.budget.exceeded()
                                || !self.charge_attributes(1)
                            {
                                return;
                            }
                            let offset = Vec3::new(
                                f64::from(c) * *column_spacing,
                                f64::from(r) * *row_spacing,
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
                            let mut child = ctx.clone();
                            child.transform = ctx.transform.compose(&local);
                            child.depth = ctx.depth + 1;
                            child.inherited_layer = child_layer;
                            child.block_color = block_color;
                            self.render_expanded_insert_attributes(
                                &def.entities,
                                &child,
                                stack,
                                cap,
                                path,
                                true,
                            );
                        }
                    }
                    stack.pop();
                }
                EntityKind::Group { children, .. } if ctx.depth < DEFAULT_WALK_DEPTH => {
                    let mut child = ctx.clone();
                    child.depth = ctx.depth + 1;
                    self.render_expanded_insert_attributes(
                        children, &child, stack, cap, path, in_block,
                    );
                }
                _ => {}
            }
        }
        path.truncate(mark);
    }

    fn render_entity(&mut self, e: &Entity, ctx: &WalkContext<'_>) {
        if !self.budget.allows(ctx.layer_name(e).len()) {
            return;
        }
        let layer = ctx.layer_name(e).to_owned();
        if self.opts.entity_refs {
            let path = ctx.path();
            self.set_ref(path.get(..ctx.model_path_len()).unwrap_or(&path));
        }
        let color = self.display_color(e.resolved_color_in(self.doc, ctx));
        let tf = self.view.compose(&ctx.transform);
        let fmt = Fmt { dec: self.fmt.dec };
        let mut pb = PathBuilder::new(&fmt, &self.budget);
        match &e.kind {
            EntityKind::Point { position } => {
                let p = tf.apply_point(*position);
                if let Some(diameter) = self.point_px {
                    // A zero-length sub-path with round caps draws a dot of the stroke width.
                    pb.move_to((p.x, p.y));
                    pb.d.push_str("h0");
                    let extra = self.budget.format(format_args!(
                        "{} stroke-width=\"{}\" stroke-linecap=\"round\"",
                        Self::stroke_base(color),
                        fmt_num(diameter, 3)
                    ));
                    self.emit_path(&layer, &pb.d, &extra);
                } else {
                    self.cross_path(&mut pb, (p.x, p.y));
                    self.stroke_path(&layer, &pb, e, ctx, color);
                }
            }
            EntityKind::Line { start, end } => {
                let (a, b) = (tf.apply_point(*start), tf.apply_point(*end));
                pb.move_to((a.x, a.y));
                pb.line_to((b.x, b.y));
                self.stroke_path(&layer, &pb, e, ctx, color);
            }
            EntityKind::Polyline {
                vertices,
                closed,
                normal,
            } => {
                pb.bulge_polyline(vertices, *closed, *normal, &tf);
                self.stroke_path(&layer, &pb, e, ctx, color);
            }
            EntityKind::Circle {
                center,
                radius,
                normal,
            } => {
                let arc = circle_arc(*center, *radius, 0.0, 0.0, *normal, &tf);
                pb.arc(&arc);
                pb.close();
                self.stroke_path(&layer, &pb, e, ctx, color);
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
                self.stroke_path(&layer, &pb, e, ctx, color);
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
                self.stroke_path(&layer, &pb, e, ctx, color);
            }
            EntityKind::Spline {
                degree,
                knots,
                control_points,
                weights,
                fit_points,
                closed,
            } => {
                let pts = spline_points(
                    *degree,
                    knots,
                    control_points,
                    weights,
                    fit_points,
                    *closed,
                    &tf,
                );
                let v: Vec<(f64, f64)> = pts.iter().map(|p| (p.x, p.y)).collect();
                pb.polyline(&v, *closed && v.len() > 2);
                self.stroke_path(&layer, &pb, e, ctx, color);
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
                oblique,
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
                        *oblique,
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
                width,
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
                    let lines = wrap_mtext_lines(mtext_text_lines(value, plain), *width, *height);
                    let lh = mtext_line_height(*height, *line_spacing);
                    self.text(
                        &layer,
                        color,
                        &tf,
                        *position,
                        *height,
                        *rotation,
                        0.0,
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
                self.stroke_path(&layer, &pb, e, ctx, color);
            }
            EntityKind::Hatch {
                loops,
                solid,
                pattern,
                pattern_scale,
                pattern_angle,
                normal,
            } => {
                for l in loops {
                    if self.budget.exceeded() {
                        break;
                    }
                    hatch_loop(&mut pb, &l.edges, *normal, &tf);
                }
                let extra = if *solid {
                    self.budget.format(format_args!(
                        " fill=\"{}\" fill-rule=\"evenodd\" stroke=\"none\"",
                        hex(color)
                    ))
                } else {
                    let own_extent = if self.opts.hatch_native_spacing {
                        e.bbox_in(&ctx.transform)
                            .map_or(self.extent, |b| b.width().max(b.height()))
                    } else {
                        self.extent
                    };
                    let pid = self.hatch_pattern_id(
                        pattern.as_deref(),
                        (*pattern_scale, *pattern_angle),
                        own_extent,
                        color,
                    );
                    self.budget.format(format_args!(
                        " fill=\"url(#{pid})\" fill-rule=\"evenodd\"{}",
                        self.stroke_attrs(e, ctx, color)
                    ))
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
                    self.stroke_path(&layer, &pb, e, ctx, color);
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
                    let stroke = self.stroke_attrs(e, ctx, color);
                    let extra = if *filled {
                        self.budget
                            .format(format_args!(" fill=\"{}\"{stroke}", hex(color)))
                    } else {
                        self.budget.format(format_args!("{stroke}"))
                    };
                    self.emit_path(&layer, &pb.d, &extra);
                }
            }
            EntityKind::Polygon {
                exterior,
                interiors,
            } => {
                for ring in std::iter::once(exterior).chain(interiors.iter()) {
                    if self.budget.exceeded() {
                        break;
                    }
                    let points: Vec<_> = ring
                        .iter()
                        .map(|p| tf.apply_point(*p))
                        .map(|p| (p.x, p.y))
                        .collect();
                    pb.polyline(&points, true);
                }
                let stroke = self.stroke_attrs(e, ctx, color);
                self.emit_path(
                    &layer,
                    &pb.d,
                    &self.budget.format(format_args!(
                        " fill=\"{}\" fill-rule=\"evenodd\"{stroke}",
                        hex(color)
                    )),
                );
            }
            EntityKind::Leader {
                vertices,
                arrowhead,
            } => {
                let v: Vec<(f64, f64)> = vertices
                    .iter()
                    .map(|p| tf.apply_point(*p))
                    .map(|p| (p.x, p.y))
                    .collect();
                if v.len() >= 2 {
                    pb.polyline(&v, false);
                    if *arrowhead && let (Some(&tip), Some(&from)) = (v.first(), v.get(1)) {
                        let (dx, dy) = (from.0 - tip.0, from.1 - tip.1);
                        let seg_len = dx.hypot(dy);
                        if seg_len > 1e-9 {
                            let alen = (self.cross * 2.5).min(seg_len * 0.45);
                            let (ux, uy) = (dx / seg_len, dy / seg_len);
                            let (px, py) = (-uy, ux);
                            let hw = alen * 0.35;
                            let w1 = (tip.0 + ux * alen + px * hw, tip.1 + uy * alen + py * hw);
                            let w2 = (tip.0 + ux * alen - px * hw, tip.1 + uy * alen - py * hw);
                            pb.move_to(w1);
                            pb.line_to(tip);
                            pb.line_to(w2);
                        }
                    }
                    self.stroke_path(&layer, &pb, e, ctx, color);
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
                let extra = self.budget.format(format_args!(
                    "{}{faint} stroke-dasharray=\"4 3\"",
                    Self::stroke_base(color)
                ));
                let mut refs = self.budget.format(format_args!("{}", self.refs()));
                if self.opts.entity_refs {
                    if let EntityKind::Image {
                        path: Some(src), ..
                    } = &e.kind
                        && !src.is_empty()
                    {
                        refs.push_str(&self.budget.format(format_args!(
                            " data-image-path=\"{}\"",
                            escape_bounded(src, &self.budget)
                        )));
                    }
                    if let EntityKind::Image {
                        size_px: Some([w, h]),
                        ..
                    } = &e.kind
                    {
                        refs.push_str(
                            &self
                                .budget
                                .format(format_args!(" data-image-px=\"{w} {h}\"")),
                        );
                    }
                }
                if !pb.d.is_empty() {
                    self.layers.push_image(
                        &self
                            .budget
                            .format(format_args!("<path{refs} d=\"{}\"{extra}/>\n", pb.d)),
                    );
                }
            }
            EntityKind::Mesh { vertices, faces } => {
                for f in faces {
                    if self.budget.exceeded() {
                        break;
                    }
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
                self.stroke_path(&layer, &pb, e, ctx, color);
            }
            EntityKind::Viewport { .. } | EntityKind::Group { .. } | EntityKind::Unknown { .. } => {
            }
        }
        self.render_attributes(e, ctx);
    }

    fn cross_path(&self, pb: &mut PathBuilder<'_>, p: (f64, f64)) {
        let s = self.cross;
        pb.move_to((p.0 - s, p.1));
        pb.line_to((p.0 + s, p.1));
        pb.move_to((p.0, p.1 - s));
        pb.line_to((p.0, p.1 + s));
    }

    fn charge_attributes(&mut self, cost: u64) -> bool {
        if self.budget.is_unlimited() {
            return true;
        }
        if let Some(remaining) = self.attribute_work.checked_sub(cost) {
            self.attribute_work = remaining;
            true
        } else {
            self.work_exceeded = true;
            false
        }
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
        // Fit text keeps its height (as a cap height); aligned text is sized by its length.
        let font = if fit {
            base / self.cap_ratio
        } else {
            base * len / natural
        };
        let angle = (p1.y - p0.y).atan2(p1.x - p0.x).to_degrees();
        let (x, y) = (self.fmt.num(p0.x), self.fmt.num(p0.y));
        let refs = self.refs();
        let mut el = self
            .budget
            .format(format_args!("<text{refs} x=\"{x}\" y=\"{y}\""));
        if angle.abs() > 1e-6 {
            el.push_str(&self.budget.format(format_args!(
                " transform=\"rotate({} {x} {y})\"",
                fmt_num(angle, 3)
            )));
        }
        el.push_str(&self.budget.format(format_args!(
            " font-size=\"{}\" text-anchor=\"start\"",
            self.fmt.num(font)
        )));
        if fit {
            el.push_str(&self.budget.format(format_args!(
                " textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\"",
                self.fmt.num(len)
            )));
        }
        el.push_str(&self.budget.format(format_args!(
            " fill=\"{}\">{}</text>\n",
            hex(color),
            escape_bounded(value, &self.budget)
        )));
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
        oblique: f64,
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
        let skew_deg = if oblique.is_finite() && oblique.abs() > 1e-6 {
            Some((-oblique.to_degrees()).clamp(-75.0, 75.0))
        } else {
            None
        };
        let refs = self.refs();
        let mut el = self
            .budget
            .format(format_args!("<text{refs} x=\"{x}\" y=\"{y}\""));
        match (angle.abs() > 1e-6, skew_deg) {
            (true, None) => {
                el.push_str(&self.budget.format(format_args!(
                    " transform=\"rotate({} {x} {y})\"",
                    fmt_num(angle, 3)
                )));
            }
            (has_rot, Some(sk)) => {
                let nx = self.fmt.num(-p.x);
                let ny = self.fmt.num(-p.y);
                let rot_part = if has_rot {
                    self.budget
                        .format(format_args!("rotate({} {x} {y}) ", fmt_num(angle, 3)))
                } else {
                    self.budget.buffer()
                };
                el.push_str(&self.budget.format(format_args!(
                    " transform=\"{rot_part}translate({x} {y}) skewX({}) translate({nx} {ny})\"",
                    fmt_num(sk, 3)
                )));
            }
            (false, None) => {}
        }
        el.push_str(&self.budget.format(format_args!(
            " font-size=\"{}\" text-anchor=\"{anchor}\"",
            self.fmt.num(size / self.cap_ratio)
        )));
        if let Some(b) = baseline {
            el.push_str(
                &self
                    .budget
                    .format(format_args!(" dominant-baseline=\"{b}\"")),
            );
        }
        el.push_str(
            &self
                .budget
                .format(format_args!(" fill=\"{}\">", hex(color))),
        );
        match body {
            TextBody::Single(v) => el.push_str(&escape_bounded(v, &self.budget)),
            TextBody::Multi(lines, lh) => {
                let lh = lh * scale;
                let n = lines.len().max(1) as f64;
                let first = match valign {
                    VAlign::Top => 0.0,
                    VAlign::Middle => -(n - 1.0) * lh / 2.0,
                    _ => -(n - 1.0) * lh,
                };
                for (i, line) in lines.iter().enumerate() {
                    if self.budget.exceeded() {
                        break;
                    }
                    let dy = if i == 0 { first } else { lh };
                    el.push_str(&self.budget.format(format_args!(
                        "<tspan x=\"{x}\" dy=\"{}\">{}</tspan>",
                        self.fmt.num(dy),
                        escape_bounded(line, &self.budget)
                    )));
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
        if pb.d.exceeded() {
            break;
        }
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
                for (i, p) in spline_points(*degree, knots, control_points, weights, &[], false, t)
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
    render_internal(doc, options, false, &Budget::default()).0
}

/// Like [`render`], but also emits `<g data-layer="...">` groups for layers marked invisible
/// (`visible == false`) or frozen (`frozen == true`).
pub fn render_all_layers(doc: &Document, options: &SvgOptions) -> String {
    render_internal(doc, options, true, &Budget::default()).0
}

/// Renders a complete SVG within `max_output_bytes` UTF-8 bytes, including markup.
/// Temporary SVG strings use the same cap. Returns `LimitExceeded` instead of a
/// partial drawing if output or traversal work exceeds the configured budget.
/// This bounds serialization, not the memory already owned by `doc`.
/// Derived text/geometry scratch is checked conservatively against the same cap;
/// an oversized temporary can be rejected even if its final SVG would be smaller.
pub fn render_bounded(
    doc: &Document,
    options: &SvgOptions,
    include_hidden_layers: bool,
) -> crate::Result<String> {
    let budget = Budget::new(options.max_output_bytes);
    let walk = WalkOptions {
        max_work: options.max_work,
        expand_inserts: options.expand_blocks,
        ..WalkOptions::default()
    };
    // A bounding-box walk can return None after exhausting its budget. Check
    // completeness first so that case cannot turn into a successful empty SVG.
    let check = |entity: &Entity, _: &WalkContext<'_>| check_temporary_inputs(entity, &budget);
    let mut stats = if include_hidden_layers {
        doc.walk_all_layers_while(options.model, &walk, check)
    } else {
        doc.walk_while(options.model, &walk, check)
    };
    // Model space shown through layout viewports is drawn too.
    if !stats.truncated
        && !budget.exceeded()
        && let Some(source) =
            layout_plan(doc, options, include_hidden_layers).and_then(|plan| plan.source)
    {
        stats = if include_hidden_layers {
            doc.walk_all_layers_while(source, &walk, check)
        } else {
            doc.walk_while(source, &walk, check)
        };
    }
    if budget.exceeded() {
        return Err(crate::Error::limit(
            crate::LimitKind::Output,
            "SVG temporary buffer budget exceeded".into(),
        ));
    }
    if stats.truncated {
        return Err(crate::Error::limit(
            crate::LimitKind::Work,
            "SVG work budget exceeded".into(),
        ));
    }
    let (svg, truncated) = render_internal(doc, options, include_hidden_layers, &budget);
    if budget.exceeded() {
        return Err(crate::Error::limit(
            crate::LimitKind::Output,
            "SVG output budget exceeded".into(),
        ));
    }
    if truncated {
        return Err(crate::Error::limit(
            crate::LimitKind::Work,
            "SVG work budget exceeded".into(),
        ));
    }
    Ok(svg)
}

// Bound derived geometry/text scratch before the bounding-box pass allocates it.
// Output paths and XML escaping have their own incremental checks.
fn check_temporary_inputs(entity: &Entity, budget: &Budget) -> bool {
    let points = |count: usize| budget.allows(count.saturating_mul(std::mem::size_of::<Point3>()));
    match &entity.kind {
        EntityKind::Spline {
            control_points,
            fit_points,
            ..
        } => {
            if !points(
                control_points
                    .len()
                    .max(fit_points.len())
                    .saturating_mul(16),
            ) {
                return false;
            }
        }
        EntityKind::Dimension { points: values, .. } | EntityKind::Face { points: values, .. } => {
            if !points(values.len()) {
                return false;
            }
        }
        EntityKind::Leader { vertices, .. } => {
            if !points(vertices.len()) {
                return false;
            }
        }
        EntityKind::Polygon {
            exterior,
            interiors,
        } => {
            if !points(exterior.len()) || interiors.iter().any(|ring| !points(ring.len())) {
                return false;
            }
        }
        EntityKind::Mesh { faces, .. } => {
            if faces.iter().any(|face| !points(face.len())) {
                return false;
            }
        }
        EntityKind::Hatch { loops, .. } => {
            for edge in loops.iter().flat_map(|item| &item.edges) {
                if let HatchEdge::Spline { control_points, .. } = edge
                    && !points(control_points.len().saturating_mul(16))
                {
                    return false;
                }
            }
        }
        EntityKind::Text { value, .. } => {
            if !budget.allows(value.len()) {
                return false;
            }
        }
        EntityKind::MText { value, plain, .. } => {
            let text = if plain.is_empty() { value } else { plain };
            // One String per paragraph/word is a conservative bound for wrapping.
            let parts = text
                .split_whitespace()
                .count()
                .saturating_add(if plain.is_empty() {
                    text.matches("\\P").count()
                } else {
                    text.bytes().filter(|&b| b == b'\n').count()
                })
                .saturating_add(1);
            if !budget.allows(
                text.len()
                    .saturating_add(parts.saturating_mul(std::mem::size_of::<String>())),
            ) {
                return false;
            }
        }
        _ => {}
    }
    !budget.exceeded()
}

fn render_internal(
    doc: &Document,
    options: &SvgOptions,
    include_hidden_layers: bool,
    budget: &Budget,
) -> (String, bool) {
    if !budget.allows(options.background.as_ref().map_or(0, String::len)) {
        return (String::new(), false);
    }
    let font_family = options.font_family.as_deref().filter(|f| !f.is_empty());
    if !budget.allows(font_family.map_or(0, str::len)) {
        return (String::new(), false);
    }
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

    let plan = layout_plan(doc, options, include_hidden_layers);
    let mut fitted = fit_bbox(doc, options.model, options.max_work, include_hidden_layers);
    if let Some(plan) = &plan {
        for frame in &plan.frames {
            let (x0, y0, x1, y1) = frame.sheet_rect();
            let rect = BBox::from_points([Point3::xy(x0, y0), Point3::xy(x1, y1)]);
            if let Some(rect) = rect {
                fitted = Some(fitted.map_or(rect, |bb| bb.merged(&rect)));
            }
        }
    }
    let Some(bb) = fitted else {
        let origin = resolve_origin(options.origin, None);
        let mut out = budget.format(format_args!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{w}\" viewBox=\"0 0 100 100\"{}>\n",
            root_attributes(origin, font_family, false, budget),
            w = px.num(width)
        ));
        if let Some(b) = bg {
            out.push_str(&budget.format(format_args!(
                "<rect x=\"0\" y=\"0\" width=\"100\" height=\"100\" fill=\"{}\"/>\n",
                escape_bounded(b, budget)
            )));
        }
        out.push_str("</svg>\n");
        return (out.into_string(), false);
    };

    let origin = resolve_origin(options.origin, Some(&bb));
    let (ox, oy) = origin.unwrap_or((0.0, 0.0));
    let extent = bb.width().max(bb.height());
    let margin = if extent > 0.0 { extent * 0.02 } else { 1.0 };
    // Subtract the origin before adding the margin, so that a drawing moved by a round
    // offset and rendered relative to it gives the same numbers.
    let vx = (bb.min.x - ox) - margin;
    let vy = -(bb.max.y - oy) - margin;
    let vw = bb.width() + 2.0 * margin;
    let vh = bb.height() + 2.0 * margin;
    let height = (width * vh / vw).max(1.0);
    let view_extent = vw.max(vh);
    let px_per_unit = if vw > 1e-12 { width / vw } else { 1.0 };
    let fmt = Fmt::for_extent(view_extent);

    let sheet = SpaceView {
        origin,
        extent: view_extent,
        px_per_unit,
    };
    let Some(mut r) = Renderer::new(doc, options, include_hidden_layers, budget, stroke, &sheet)
    else {
        return (String::new(), false);
    };
    let cap = options.max_output_bytes;
    let (mut truncated, spent) = r.draw(options.model, options.max_work, cap);

    // Paper layout: model space rendered once, shown through each viewport.
    let mut model_content = budget.buffer();
    let mut viewports = budget.buffer();
    if let Some(plan) = &plan
        && !truncated
        && !budget.exceeded()
    {
        let remaining = options.max_work.saturating_sub(spent);
        let mut model_origin = (0.0, 0.0);
        if let Some(source) = plan.source
            && let Some(mbb) = fit_bbox(doc, source, remaining, include_hidden_layers)
        {
            let own = mbb.width().max(mbb.height()) * 1.04;
            // Precision and point/hatch sizes follow the smallest area a viewport shows.
            let shown = plan
                .frames
                .iter()
                .map(ViewportFrame::view_extent)
                .fold(own, f64::min);
            let first_scale = plan.frames.first().map_or(1.0, |f| f.h / f.vh);
            let space = SpaceView {
                origin: if origin.is_some() {
                    resolve_origin(SvgOrigin::Auto, Some(&mbb))
                } else {
                    None
                },
                extent: if shown.is_finite() && shown > 0.0 {
                    shown
                } else {
                    1.0
                },
                px_per_unit: px_per_unit * first_scale,
            };
            let Some(mut m) =
                Renderer::new(doc, options, include_hidden_layers, budget, stroke, &space)
            else {
                return (String::new(), false);
            };
            m.defs = std::mem::take(&mut r.defs);
            m.hatch_cache = std::mem::take(&mut r.hatch_cache);
            m.layers.bytes = r.layers.bytes;
            let (model_truncated, _) = m.draw(source, remaining, cap);
            truncated |= model_truncated;
            r.defs = std::mem::take(&mut m.defs);
            r.layers.bytes = m.layers.bytes;
            model_origin = space.origin.unwrap_or((0.0, 0.0));
            let origin_attr = match space.origin {
                Some((x, y)) => budget.format(format_args!(" data-origin=\"{x} {y}\"")),
                None => budget.buffer(),
            };
            model_content.push_str(&budget.format(format_args!(
                "<g id=\"cadkit-model\" data-model=\"{source}\"{origin_attr}>\n"
            )));
            write_layers(&mut model_content, &m.layers, stroke, budget);
            model_content.push_str("</g>\n");
        }
        let frame_color = hex(with_contrast((0, 0, 0), r.background));
        for (i, f) in plan.frames.iter().enumerate() {
            if budget.exceeded() {
                break;
            }
            let (x0, _, _, y1) = f.sheet_rect();
            let (x, y) = (fmt.num(x0 - ox), fmt.num(-(y1 - oy)));
            let (w, h) = (fmt.num(f.w), fmt.num(f.h));
            let mut item = budget.buffer();
            if !model_content.is_empty() {
                // Model view point (mx, my) lands on sheet view point (k mx + tx, k my + ty).
                let k = f.h / f.vh;
                let tx = (f.cx - ox) + (model_origin.0 - f.vcx) * k;
                let ty = (oy - f.cy) - (model_origin.1 - f.vcy) * k;
                item.push_str(&budget.format(format_args!(
                    "<clipPath id=\"cadkit-viewport-{n}\"><rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"/></clipPath>\n\
                     <g clip-path=\"url(#cadkit-viewport-{n})\"{refs} data-scale=\"{scale}\"><use href=\"#cadkit-model\" transform=\"matrix({ks} 0 0 {ks} {txs} {tys})\"/></g>\n",
                    n = i + 1,
                    refs = f.refs,
                    scale = fmt_sig(f.vh / f.h, 10),
                    ks = fmt_sig(k, 12),
                    txs = fmt_num(tx, fmt.dec + 3),
                    tys = fmt_num(ty, fmt.dec + 3),
                )));
            }
            item.push_str(&budget.format(format_args!(
                "<path{} d=\"M{x} {y}h{w}v{h}h-{w}Z\" stroke=\"{frame_color}\" vector-effect=\"non-scaling-stroke\"/>\n",
                f.refs
            )));
            if !r.layers.reserve(item.len()) {
                truncated = true;
                break;
            }
            viewports.push_str(&item);
        }
    }

    let mut out = budget.format(format_args!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\"{}>\n",
        px.num(width),
        px.num(height),
        fmt.num(vx),
        fmt.num(vy),
        fmt.num(vw),
        fmt.num(vh),
        root_attributes(origin, font_family, truncated, budget),
    ));
    if truncated {
        out.push_str("<!-- cadkit: drawing truncated, work or output size limit reached -->\n");
    }
    if let Some(b) = bg {
        out.push_str(&budget.format(format_args!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
            fmt.num(vx),
            fmt.num(vy),
            fmt.num(vw),
            fmt.num(vh),
            escape_bounded(b, budget)
        )));
    }
    if !r.defs.is_empty() || !model_content.is_empty() {
        out.push_str("<defs>\n");
        for def in &r.defs {
            out.push_str(def);
        }
        out.push_str(&model_content);
        out.push_str("</defs>\n");
    }
    if !viewports.is_empty() {
        // Viewports first: the sheet's own entities (title block, notes) draw over them.
        out.push_str(&budget.format(format_args!(
            "<g data-kind=\"viewports\" fill=\"none\" stroke-width=\"{}\">\n",
            fmt_num(stroke, 3)
        )));
        out.push_str(&viewports);
        out.push_str("</g>\n");
    }
    write_layers(&mut out, &r.layers, stroke, budget);
    out.push_str("</svg>\n");
    (out.into_string(), truncated)
}

/// Bounding box the view is fitted to: the vector geometry, or the image frames when there is
/// nothing else.
fn fit_bbox(doc: &Document, model: usize, max_work: u64, include_hidden: bool) -> Option<BBox> {
    let no_images = BBoxOptions {
        include_images: false,
        max_work,
    };
    let with_images = BBoxOptions {
        include_images: true,
        max_work,
    };
    if include_hidden {
        doc.bbox_all_layers_with(model, &no_images)
            .or_else(|| doc.bbox_all_layers_with(model, &with_images))
    } else {
        doc.bbox_with(model, &no_images)
            .or_else(|| doc.bbox_with(model, &with_images))
    }
}

/// Root element attributes after the `viewBox`; empty with default options.
fn root_attributes(
    origin: Option<(f64, f64)>,
    font_family: Option<&str>,
    truncated: bool,
    budget: &Budget,
) -> Buffer {
    let mut out = budget.buffer();
    if let Some((x, y)) = origin {
        // Shortest round-trip form: the page can recover the exact value.
        out.push_str(&budget.format(format_args!(" data-origin=\"{x} {y}\"")));
    }
    if let Some(family) = font_family {
        out.push_str(&budget.format(format_args!(
            " font-family=\"{}\"",
            escape_bounded(family, budget)
        )));
    }
    if truncated {
        out.push_str(" data-truncated=\"true\"");
    }
    out
}

/// Layer groups, then the image frame group, of one rendered space.
fn write_layers(out: &mut Buffer, layers: &Layers, stroke: f64, budget: &Budget) {
    for (name, body) in &layers.items {
        out.push_str(&layer_open(name, stroke, budget));
        out.push_str(body);
        out.push_str("</g>\n");
    }
    if !layers.images.is_empty() {
        out.push_str(&budget.format(format_args!(
            "<g data-kind=\"images\" fill=\"none\" stroke-width=\"{}\">\n",
            fmt_num(stroke, 3)
        )));
        out.push_str(&layers.images);
        out.push_str("</g>\n");
    }
}

/// A paper-space viewport that shows model space, in drawing units.
struct ViewportFrame {
    /// Centre and size on the sheet.
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
    /// Model-space centre and height shown.
    vcx: f64,
    vcy: f64,
    vh: f64,
    /// ` data-e` of the viewport entity (empty unless entity references are on).
    refs: String,
}

impl ViewportFrame {
    /// `(x0, y0, x1, y1)` on the sheet.
    fn sheet_rect(&self) -> (f64, f64, f64, f64) {
        let (hw, hh) = (self.w / 2.0, self.h / 2.0);
        (self.cx - hw, self.cy - hh, self.cx + hw, self.cy + hh)
    }

    /// Larger side of the model area shown, in model units.
    fn view_extent(&self) -> f64 {
        self.vh.max(self.w * self.vh / self.h)
    }

    /// The first viewport of a DWG/DXF layout stands for the sheet itself: it shows paper
    /// space at 1:1 around its own centre.
    fn is_sheet(&self) -> bool {
        let eps = 1e-6 * self.h.abs().max(1.0);
        (self.vh - self.h).abs() <= eps
            && (self.vcx - self.cx).abs() <= eps
            && (self.vcy - self.cy).abs() <= eps
    }
}

/// Viewports of a paper layout and the model space they show.
struct LayoutPlan {
    /// Index of the first model of kind `Model`, when there is one.
    source: Option<usize>,
    /// Viewports that show model space, in walk order.
    frames: Vec<ViewportFrame>,
}

/// The layout plan of the selected model when [`SvgOptions::layout_viewports`] applies and
/// it has at least one viewport onto model space.
fn layout_plan(doc: &Document, options: &SvgOptions, include_hidden: bool) -> Option<LayoutPlan> {
    if !options.layout_viewports
        || doc
            .models
            .get(options.model)
            .is_none_or(|m| m.kind == ModelKind::Model)
    {
        return None;
    }
    let walk = WalkOptions {
        max_depth: DEFAULT_WALK_DEPTH,
        expand_inserts: options.expand_blocks,
        max_work: options.max_work,
    };
    let mut frames = Vec::new();
    let collect = |e: &Entity, ctx: &WalkContext<'_>| {
        if let EntityKind::Viewport {
            center,
            width,
            height,
            view_center,
            view_height,
        } = &e.kind
        {
            let c = ctx.transform.apply_point(*center);
            let frame = ViewportFrame {
                cx: c.x,
                cy: c.y,
                w: *width,
                h: *height,
                vcx: view_center.x,
                vcy: view_center.y,
                vh: *view_height,
                refs: if options.entity_refs {
                    let path = ctx.path();
                    let path = path.get(..ctx.model_path_len()).unwrap_or(&path);
                    let joined: Vec<String> = path.iter().map(usize::to_string).collect();
                    format!(" data-e=\"{}\"", joined.join("."))
                } else {
                    String::new()
                },
            };
            let usable = [
                frame.cx, frame.cy, frame.w, frame.h, frame.vcx, frame.vcy, frame.vh,
            ]
            .iter()
            .all(|v| v.is_finite())
                && frame.w > 0.0
                && frame.h > 0.0
                && frame.vh > 0.0
                && frame.view_extent().is_finite();
            if usable {
                frames.push(frame);
            }
        }
        true
    };
    if include_hidden {
        doc.walk_all_layers_while(options.model, &walk, collect);
    } else {
        doc.walk_while(options.model, &walk, collect);
    }
    if frames.first().is_some_and(ViewportFrame::is_sheet) {
        frames.remove(0);
    }
    if frames.is_empty() {
        return None;
    }
    Some(LayoutPlan {
        source: doc.models.iter().position(|m| m.kind == ModelKind::Model),
        frames,
    })
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
    fn displayed_attributes_use_their_own_color_when_set() {
        let mut owner = line(0.0, 0.0, 10.0, 0.0);
        owner.color = Color::Rgb { r: 0, g: 0, b: 128 };
        let display = crate::model::AttributeDisplay::new(1.0, 1.0);
        owner.attributes.push(
            crate::model::Attribute::new("OWN", "a")
                .with_position(Point3::xy(1.0, 1.0))
                .with_display(display.clone())
                .with_color(Color::Rgb { r: 128, g: 0, b: 0 }),
        );
        owner.attributes.push(
            crate::model::Attribute::new("INHERITED", "b")
                .with_position(Point3::xy(2.0, 2.0))
                .with_display(display),
        );
        let s = svg(vec![owner]);
        let texts: Vec<&str> = s.lines().filter(|l| l.contains("<text")).collect();
        assert_eq!(texts.len(), 2, "{s}");
        assert!(
            texts
                .iter()
                .any(|l| l.contains("fill=\"#800000\">a</text>")),
            "{s}"
        );
        assert!(
            texts
                .iter()
                .any(|l| l.contains("fill=\"#000080\">b</text>")),
            "{s}"
        );
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

    #[test]
    fn lineweight_linetype_and_hatch_patterns_render() {
        let mut dashed = line(0.0, 0.0, 20.0, 0.0)
            .with_linetype("DASHED")
            .with_lineweight(Lineweight::Millimeters(0.5));
        dashed.layer = Some("WALLS".into());

        let hatch = Entity::new(EntityKind::Hatch {
            loops: vec![HatchLoop {
                edges: vec![
                    HatchEdge::Line {
                        start: Point3::xy(0.0, 2.0),
                        end: Point3::xy(10.0, 2.0),
                    },
                    HatchEdge::Line {
                        start: Point3::xy(10.0, 2.0),
                        end: Point3::xy(10.0, 8.0),
                    },
                    HatchEdge::Line {
                        start: Point3::xy(10.0, 8.0),
                        end: Point3::xy(0.0, 8.0),
                    },
                    HatchEdge::Line {
                        start: Point3::xy(0.0, 8.0),
                        end: Point3::xy(0.0, 2.0),
                    },
                ],
                external: true,
            }],
            solid: false,
            pattern: Some("ANSI37".into()),
            pattern_scale: 1.0,
            pattern_angle: 0.0,
            normal: Vec3::Z,
        });

        let mut d = doc(vec![dashed, hatch]);
        d.linetypes.push(Linetype {
            name: "DASHED".into(),
            description: None,
            pattern: vec![1.0, -0.5],
            props: Default::default(),
        });

        let opts = SvgOptions::new().dark().with_width_px(800.0);
        let s = render(&d, &opts);
        assert!(s.contains("stroke-width=\"2\""), "{s}");
        assert!(s.contains("stroke-dasharray=\""), "{s}");
        assert!(
            s.contains("<defs>") && s.contains("<pattern id=\"cadkit-hatch-1\""),
            "{s}"
        );
        assert!(s.contains("fill=\"url(#cadkit-hatch-1)\""), "{s}");
        // Cross-hatch ANSI37 emits two orthogonal lines inside the pattern tile.
        assert_eq!(s.matches("<line ").count(), 2, "{s}");
    }

    #[test]
    fn oblique_text_wrapped_mtext_leader_arrow_and_insert_attributes() {
        let oblique_text = Entity::new(EntityKind::Text {
            position: Point3::xy(0.0, 0.0),
            end_point: None,
            height: 2.0,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.25,
            value: "SLANT".into(),
            style: None,
            halign: HAlign::Left,
            valign: VAlign::Baseline,
            normal: Vec3::Z,
        });
        let wrapped_mtext = Entity::new(EntityKind::MText {
            position: Point3::xy(0.0, 10.0),
            height: 1.0,
            width: 6.0,
            rotation: 0.0,
            value: "alpha beta gamma delta".into(),
            plain: "alpha beta gamma delta".into(),
            style: None,
            attachment: Attachment::TopLeft,
            line_spacing: 1.0,
            normal: Vec3::Z,
        });
        let leader = Entity::new(EntityKind::Leader {
            vertices: vec![Point3::xy(0.0, 0.0), Point3::xy(10.0, 0.0)],
            arrowhead: true,
        });
        let mut ins = Entity::insert("TAGBLK", Point3::xy(5.0, 5.0));
        ins.attributes.push(crate::model::Attribute {
            tag: "ROOM".into(),
            value: Value::from("A-101"),
            position: Some(Point3::xy(6.0, 6.0)),
            invisible: false,
            display: Some(crate::model::AttributeDisplay {
                height: 1.5,
                width: 1.5,
                style: None,
                halign: HAlign::Left,
                valign: VAlign::Baseline,
                rotation: 0.0,
            }),
            ..Default::default()
        });

        let mut d = doc(vec![oblique_text, wrapped_mtext, leader, ins]);
        d.blocks.push(Block {
            name: "TAGBLK".into(),
            entities: vec![line(0.0, 0.0, 2.0, 0.0)],
            ..Block::default()
        });

        let s = render(&d, &SvgOptions::default());
        assert!(s.contains("skewX("), "{s}");
        assert!(s.matches("<tspan").count() >= 2, "{s}");
        assert!(s.contains(">A-101</text>"), "{s}");
        // Leader has 1 move_to for the shaft + 1 move_to for the arrowhead wings.
        assert!(s.contains("M0 0L10 0M"), "{s}");
        assert_eq!(background_rgb(Some("rgb(18, 22, 28)")), (18, 22, 28));
    }

    /// A document that uses every entity kind, with its geometry moved by `(dx, dy)`.
    /// Coordinates are binary fractions so that a shifted copy renders identically relative
    /// to its origin.
    fn every_kind_document(dx: f64, dy: f64) -> Document {
        let p = |x: f64, y: f64| Point3::xy(x + dx, y + dy);
        let v = |x: f64, y: f64, bulge: f64| Vertex {
            position: p(x, y),
            bulge,
            ..Vertex::default()
        };
        let text = |x: f64, y: f64, value: &str, halign: HAlign, end: Option<Point3>| {
            Entity::new(EntityKind::Text {
                position: p(x, y),
                end_point: end,
                height: 1.5,
                rotation: 0.25,
                width_factor: 1.0,
                oblique: if halign == HAlign::Left { 0.25 } else { 0.0 },
                value: value.into(),
                style: None,
                halign,
                valign: VAlign::Baseline,
                normal: Vec3::Z,
            })
        };
        let square = |x: f64, y: f64, s: f64| HatchLoop {
            edges: vec![
                HatchEdge::Line {
                    start: p(x, y),
                    end: p(x + s, y),
                },
                HatchEdge::Line {
                    start: p(x + s, y),
                    end: p(x + s, y + s),
                },
                HatchEdge::Line {
                    start: p(x + s, y + s),
                    end: p(x, y + s),
                },
                HatchEdge::Line {
                    start: p(x, y + s),
                    end: p(x, y),
                },
            ],
            external: true,
        };
        let hatch = |solid: bool, x: f64| {
            Entity::new(EntityKind::Hatch {
                loops: vec![
                    square(x, 20.0, 6.0),
                    HatchLoop {
                        edges: vec![HatchEdge::Arc {
                            center: p(x + 3.0, 23.0),
                            radius: 1.0,
                            start_angle: 0.0,
                            end_angle: TAU,
                            ccw: true,
                        }],
                        external: false,
                    },
                ],
                solid,
                pattern: if solid { None } else { Some("ANSI37".into()) },
                pattern_scale: 1.0,
                pattern_angle: 0.5,
                normal: Vec3::Z,
            })
        };
        let mut tagged = Entity::insert("TAG", p(30.0, 30.0));
        tagged.attributes.push(crate::model::Attribute {
            tag: "ROOM".into(),
            value: Value::from("R-1"),
            position: Some(p(31.0, 31.0)),
            display: Some(crate::model::AttributeDisplay {
                height: 1.0,
                width: 1.0,
                style: None,
                halign: HAlign::Left,
                valign: VAlign::Baseline,
                rotation: 0.0,
            }),
            ..Default::default()
        });
        let mut white = line(0.0 + dx, 40.0 + dy, 10.0 + dx, 40.0 + dy);
        white.color = Color::Rgb {
            r: 255,
            g: 255,
            b: 250,
        };
        let mut dashed = line(0.0 + dx, 42.0 + dy, 20.0 + dx, 42.0 + dy)
            .with_linetype("DASHED")
            .with_lineweight(Lineweight::Millimeters(0.5));
        dashed.layer = Some("WALLS".into());
        let entities = vec![
            Entity::new(EntityKind::Point {
                position: p(1.0, 1.0),
            }),
            line(dx, dy, 10.0 + dx, 5.0 + dy),
            Entity::new(EntityKind::Polyline {
                vertices: vec![v(0.0, 10.0, 0.5), v(4.0, 10.0, 0.0), v(4.0, 14.0, 0.0)],
                closed: true,
                normal: Vec3::Z,
            }),
            Entity::new(EntityKind::Circle {
                center: p(15.0, 5.0),
                radius: 2.5,
                normal: Vec3::Z,
            }),
            Entity::new(EntityKind::Arc {
                center: p(22.0, 5.0),
                radius: 2.0,
                start_angle: 0.25,
                end_angle: 2.5,
                normal: Vec3::Z,
            }),
            Entity::new(EntityKind::Ellipse {
                center: p(30.0, 5.0),
                major_axis: Vec3::new(3.0, 1.0, 0.0),
                ratio: 0.5,
                start_param: 0.0,
                end_param: 4.0,
                normal: Vec3::Z,
            }),
            Entity::new(EntityKind::Spline {
                degree: 3,
                knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                control_points: vec![p(36.0, 0.0), p(38.0, 4.0), p(40.0, -2.0), p(42.0, 2.0)],
                weights: vec![],
                fit_points: vec![],
                closed: false,
            }),
            text(0.0, 16.0, "Left <&>", HAlign::Left, None),
            text(10.0, 16.0, "Fit", HAlign::Fit, Some(p(16.0, 17.0))),
            text(20.0, 16.0, "Aligned", HAlign::Aligned, Some(p(28.0, 16.0))),
            text(30.0, 16.0, "Middle", HAlign::Middle, None),
            Entity::new(EntityKind::MText {
                position: p(36.0, 18.0),
                height: 1.0,
                width: 6.0,
                rotation: 0.0,
                value: "alpha beta gamma delta".into(),
                plain: "alpha beta gamma delta".into(),
                style: None,
                attachment: Attachment::TopLeft,
                line_spacing: 1.0,
                normal: Vec3::Z,
            }),
            tagged,
            Entity::insert("MISSING", p(34.0, 30.0)),
            hatch(true, 0.0),
            hatch(false, 8.0),
            Entity::new(EntityKind::Dimension {
                dimension_type: crate::model::DimensionType::Linear,
                points: vec![p(16.0, 20.0), p(24.0, 20.0)],
                text_position: None,
                measurement: Some(8.0),
                text: None,
                block: Some("DIM".into()),
                style: None,
            }),
            Entity::new(EntityKind::Dimension {
                dimension_type: crate::model::DimensionType::Linear,
                points: vec![p(16.0, 24.0), p(24.0, 24.0)],
                text_position: None,
                measurement: Some(8.0),
                text: None,
                block: None,
                style: None,
            }),
            Entity::new(EntityKind::Face {
                points: vec![p(26.0, 20.0), p(30.0, 20.0), p(30.0, 24.0), p(26.0, 24.0)],
                filled: true,
            }),
            Entity::new(EntityKind::Polygon {
                exterior: vec![p(32.0, 20.0), p(38.0, 20.0), p(38.0, 26.0), p(32.0, 26.0)],
                interiors: vec![vec![p(34.0, 22.0), p(36.0, 22.0), p(36.0, 24.0)]],
            }),
            Entity::new(EntityKind::Leader {
                vertices: vec![p(40.0, 20.0), p(44.0, 24.0), p(46.0, 24.0)],
                arrowhead: true,
            }),
            Entity::new(EntityKind::Image {
                path: Some("photo.png".into()),
                position: p(0.0, 30.0),
                u_vector: Vec3::new(8.0, 0.0, 0.0),
                v_vector: Vec3::new(0.0, 6.0, 0.0),
                size_px: Some([640, 480]),
            }),
            Entity::new(EntityKind::Mesh {
                vertices: vec![p(10.0, 30.0), p(14.0, 30.0), p(12.0, 34.0)],
                faces: vec![vec![0, 1, 2]],
            }),
            Entity::new(EntityKind::Viewport {
                center: p(20.0, 30.0),
                width: 4.0,
                height: 4.0,
                view_center: p(20.0, 30.0),
                view_height: 4.0,
            }),
            Entity::new(EntityKind::Group {
                group_kind: GroupKind::Cell,
                name: Some("cell".into()),
                origin: Some(p(22.0, 30.0)),
                children: vec![
                    line(22.0 + dx, 30.0 + dy, 26.0 + dx, 34.0 + dy),
                    Entity::new(EntityKind::Point {
                        position: p(24.0, 30.0),
                    }),
                ],
            }),
            Entity::new(EntityKind::Unknown {
                type_name: "dwg.X".into(),
            }),
            white,
            dashed,
        ];
        let mut d = doc(entities);
        d.blocks.push(Block {
            name: "TAG".into(),
            entities: vec![
                line(0.0, 0.0, 2.0, 0.0),
                Entity::new(EntityKind::Circle {
                    center: Point3::xy(1.0, 1.0),
                    radius: 0.5,
                    normal: Vec3::Z,
                }),
            ],
            ..Block::default()
        });
        d.blocks.push(Block {
            name: "DIM".into(),
            entities: vec![
                line(16.0 + dx, 21.0 + dy, 24.0 + dx, 21.0 + dy),
                text(19.0, 21.5, "8", HAlign::Center, None),
            ],
            ..Block::default()
        });
        d.layers.push(Layer {
            name: "WALLS".into(),
            visible: true,
            color: Color::Aci { index: 1 },
            ..Layer::default()
        });
        d.linetypes.push(Linetype {
            name: "DASHED".into(),
            description: None,
            pattern: vec![1.0, -0.5],
            props: Default::default(),
        });
        d
    }

    /// `render(every_kind_document(0, 0))` with default options. The new opt-in options must
    /// not change a byte of it.
    const PINNED_DEFAULT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1454.98" viewBox="-1.29853 -42.92742 48.22595 43.85484">
<rect x="-1.29853" y="-42.92742" width="48.22595" height="43.85484" fill="#ffffff"/>
<defs>
<pattern id="cadkit-hatch-1" patternUnits="userSpaceOnUse" width="3.175" height="3.175" patternTransform="rotate(-73.65)"><line x1="0" y1="0" x2="3.175" y2="0" stroke="#000000" stroke-width="0.75" vector-effect="non-scaling-stroke"/><line x1="0" y1="0" x2="0" y2="3.175" stroke="#000000" stroke-width="0.75" vector-effect="non-scaling-stroke"/></pattern>
</defs>
<g data-layer="0" fill="none" stroke-width="1" stroke-linecap="round" stroke-linejoin="round">
<path d="M0.8071 -1L1.1929 -1M1 -1.1929L1 -0.8071" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M0 0L10 -5" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M0 -10A2.5 2.5 0 0 0 4 -10L4 -14L0 -10Z" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M17.5 -5A2.5 2.5 0 0 0 12.5 -5A2.5 2.5 0 0 0 17.5 -5Z" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M23.93782 -5.49481A2 2 0 0 0 20.39771 -6.19694" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M33 -6A3.16228 1.58114 -18.435 1 0 28.41747 -3.21115" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M36 0L36.375 -0.6377L36.75 -1.07031L37.125 -1.32715L37.5 -1.4375L37.875 -1.43066L38.25 -1.33594L38.625 -1.18262L39 -1L39.375 -0.81738L39.75 -0.66406L40.125 -0.56934L40.5 -0.5625L40.875 -0.67285L41.25 -0.92969L41.625 -1.3623L42 -2" stroke="#000000" vector-effect="non-scaling-stroke"/>
<text x="0" y="-16" transform="rotate(-14.324 0 -16) translate(0 -16) skewX(-14.324) translate(0 16)" font-size="1.5" text-anchor="start" fill="#000000">Left &lt;&amp;&gt;</text>
<text x="10" y="-16" transform="rotate(-9.462 10 -16)" font-size="1.5" text-anchor="start" textLength="6.08276" lengthAdjust="spacingAndGlyphs" fill="#000000">Fit</text>
<text x="20" y="-16" font-size="1.90476" text-anchor="start" fill="#000000">Aligned</text>
<text x="30" y="-16" transform="rotate(-14.324 30 -16)" font-size="1.5" text-anchor="middle" dominant-baseline="central" fill="#000000">Middle</text>
<text x="36" y="-18" font-size="1" text-anchor="start" dominant-baseline="hanging" fill="#000000"><tspan x="36" dy="0">alpha beta</tspan><tspan x="36" dy="1.66667">gamma</tspan><tspan x="36" dy="1.66667">delta</tspan></text>
<path d="M30 -30L32 -30" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M31.5 -31A0.5 0.5 0 0 0 30.5 -31A0.5 0.5 0 0 0 31.5 -31Z" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M33.8071 -30L34.1929 -30M34 -30.1929L34 -29.8071" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M0 -20L6 -20L6 -26L0 -26L0 -20ZM4 -23A1 1 0 0 0 2 -23A1 1 0 0 0 4 -23Z" fill="#000000" fill-rule="evenodd" stroke="none"/>
<path d="M8 -20L14 -20L14 -26L8 -26L8 -20ZM12 -23A1 1 0 0 0 10 -23A1 1 0 0 0 12 -23Z" fill="url(#cadkit-hatch-1)" fill-rule="evenodd" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M16 -21L24 -21" stroke="#000000" vector-effect="non-scaling-stroke"/>
<text x="19" y="-21.5" transform="rotate(-14.324 19 -21.5)" font-size="1.5" text-anchor="middle" fill="#000000">8</text>
<path d="M16 -24L24 -24" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M26 -20L30 -20L30 -24L26 -24Z" fill="#000000" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M32 -20L38 -20L38 -26L32 -26ZM34 -22L36 -22L36 -24Z" fill="#000000" fill-rule="evenodd" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M40 -20L44 -24L46 -24M40.46036 -20.22166L40 -20L40.22166 -20.46036" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M10 -30L14 -30L12 -34Z" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M22 -30L26 -34" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M23.8071 -30L24.1929 -30M24 -30.1929L24 -29.8071" stroke="#000000" vector-effect="non-scaling-stroke"/>
<path d="M0 -40L10 -40" stroke="#000000" vector-effect="non-scaling-stroke"/>
<text x="31" y="-31" font-size="1" text-anchor="start" fill="#000000">R-1</text>
</g>
<g data-layer="WALLS" fill="none" stroke-width="1" stroke-linecap="round" stroke-linejoin="round">
<path d="M0 -42L20 -42" stroke="#ff0000" vector-effect="non-scaling-stroke" stroke-width="2" stroke-dasharray="33.18 16.59"/>
</g>
<g data-kind="images" fill="none" stroke-width="1">
<path d="M0 -30L8 -30L8 -36L0 -36Z" stroke="#000000" vector-effect="non-scaling-stroke" stroke-dasharray="4 3"/>
</g>
</svg>
"##;

    #[test]
    fn default_output_is_pinned() {
        let d = every_kind_document(0.0, 0.0);
        let s = render(&d, &SvgOptions::default());
        assert_eq!(s, PINNED_DEFAULT);
        // The opt-in options are inert where they do not apply: layout viewports on a model
        // space, hatch spacing without pattern hatches, text metrics with text off.
        assert_eq!(
            render(&d, &SvgOptions::default().with_layout_viewports(true)),
            s
        );
        let exact = SvgOptions::default().with_max_output_bytes(s.len());
        assert_eq!(render_bounded(&d, &exact, false).expect("exact budget"), s);
        let serde_default: SvgOptions = serde_json::from_str("{}").expect("defaults");
        assert_eq!(serde_default, SvgOptions::default());
    }

    fn without_origin(s: &str) -> String {
        let start = s.find(" data-origin=\"").expect("data-origin");
        let len = s[start + 14..].find('"').expect("closing quote") + 15;
        format!("{}{}", &s[..start], &s[start + len..])
    }

    #[test]
    fn origin_keeps_relative_geometry_of_a_georeferenced_drawing() {
        let local = render(&every_kind_document(0.0, 0.0), &SvgOptions::default());
        let geo = every_kind_document(500_000.0, 4_000_000.0);
        let absolute = render(&geo, &SvgOptions::default());
        assert!(
            absolute.contains("M500000 -4000000L500010 -4000005"),
            "{absolute}"
        );
        for origin in [
            SvgOrigin::Explicit {
                x: 500_000.0,
                y: 4_000_000.0,
            },
            SvgOrigin::Auto,
        ] {
            let s = render(&geo, &SvgOptions::default().with_origin(origin));
            assert!(s.contains(" data-origin=\"500000 4000000\""), "{s}");
            assert_eq!(without_origin(&s), local);
        }
        // Relative to another origin the numbers stay small and keep the drawing's precision.
        let s = render(
            &geo,
            &SvgOptions::default().with_origin(SvgOrigin::Explicit {
                x: 500_010.5,
                y: 4_000_000.0,
            }),
        );
        assert!(s.contains(" data-origin=\"500010.5 4000000\""), "{s}");
        assert!(s.contains("d=\"M-10.5 0L-0.5 -5\""), "{s}");
        assert!(
            s.contains("viewBox=\"-11.79853 -42.92742 48.22595 43.85484\""),
            "{s}"
        );
        assert!(s.contains("<rect x=\"-11.79853\" y=\"-42.92742\""), "{s}");
        assert!(s.contains("<text x=\"20.5\" y=\"-31\""), "{s}");
        assert!(s.contains("rotate(-14.324 -10.5 -16)"), "{s}");
        assert!(s.contains("<path d=\"M-10.5 -30L-2.5 -30"), "{s}");
    }

    #[test]
    fn automatic_origin_is_round_and_hostile_origins_fall_back() {
        assert_eq!(auto_origin_axis(500_022.5, 46.0), 500_000.0);
        assert_eq!(auto_origin_axis(3_999_979.0, 46.0), 4_000_000.0);
        assert_eq!(auto_origin_axis(0.3, 2.0), 0.0);
        assert_eq!(auto_origin_axis(-0.3, 2.0), 0.0);
        assert_eq!(auto_origin_axis(12.345_67, 0.05), 12.3);
        assert_eq!(auto_origin_axis(f64::NAN, 1.0), 0.0);
        assert_eq!(auto_origin_axis(1e308, 0.0), 1e308);
        assert_eq!(auto_origin_axis(5.0, f64::INFINITY), 5.0);
        let d = doc(vec![line(10.0, 10.0, 12.0, 11.0)]);
        let auto = render(&d, &SvgOptions::default().with_origin(SvgOrigin::Auto));
        assert!(auto.contains("data-origin=\"10 10\""), "{auto}");
        for (x, y) in [
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
            (f64::NEG_INFINITY, f64::NAN),
        ] {
            let s = render(
                &d,
                &SvgOptions::default().with_origin(SvgOrigin::Explicit { x, y }),
            );
            assert_eq!(s, auto);
        }
        let empty = render(
            &Document::default(),
            &SvgOptions::default().with_origin(SvgOrigin::Auto),
        );
        assert!(
            empty.contains("viewBox=\"0 0 100 100\" data-origin=\"0 0\""),
            "{empty}"
        );
        let json = serde_json::to_string(&SvgOrigin::Explicit { x: 1.5, y: -2.0 }).expect("json");
        assert_eq!(json, r#"{"explicit":{"x":1.5,"y":-2.0}}"#);
        let back: SvgOptions = serde_json::from_str(r#"{"origin":"auto"}"#).expect("options");
        assert_eq!(back.origin, SvgOrigin::Auto);
    }

    /// Every element the option tags, by element name: `(tagged, all)`.
    fn tagged(s: &str, element: &str) -> (usize, usize) {
        let open = format!("<{element} ");
        let refd = format!("<{element} data-e=\"");
        (s.matches(&refd).count(), s.matches(&open).count())
    }

    #[test]
    fn entity_references_tag_every_element() {
        let d = every_kind_document(0.0, 0.0);
        let s = render(&d, &SvgOptions::default().with_entity_refs(true));
        for element in ["path", "text"] {
            let (refd, all) = tagged(&s, element);
            assert_eq!(refd, all, "{element}: {s}");
            assert!(all > 0);
        }
        // Model entities by index; a group child by its group path; block geometry and the
        // insert's attribute text by the insert; dimension block geometry by the dimension.
        assert!(
            s.contains("<path data-e=\"0\" d=\"M0.8071 -1L1.1929"),
            "{s}"
        );
        assert!(s.contains("<path data-e=\"1\" d=\"M0 0L10 -5\""), "{s}");
        assert!(
            s.contains("<path data-e=\"24.0\" d=\"M22 -30L26 -34\""),
            "{s}"
        );
        assert!(s.contains("<path data-e=\"24.1\" d=\"M23.8071"), "{s}");
        assert!(
            s.contains("<path data-e=\"12\" d=\"M30 -30L32 -30\""),
            "{s}"
        );
        assert!(s.contains("<path data-e=\"12\" d=\"M31.5 -31A"), "{s}");
        assert!(s.contains("<text data-e=\"12\" x=\"31\" y=\"-31\""), "{s}");
        assert!(
            s.contains("<path data-e=\"16\" d=\"M16 -21L24 -21\""),
            "{s}"
        );
        assert!(s.contains("<text data-e=\"16\" x=\"19\""), "{s}");
        // The near-white line was drawn black; its own colour is kept for restyling.
        assert!(
            s.contains(
                "<path data-e=\"26\" data-c=\"#fffffa\" d=\"M0 -40L10 -40\" stroke=\"#000000\""
            ),
            "{s}"
        );
        assert_eq!(s.matches("data-c=").count(), 1, "{s}");
        assert!(
            s.contains("<path data-e=\"21\" data-image-path=\"photo.png\" data-image-px=\"640 480\" d=\"M0 -30L8 -30"),
            "{s}"
        );
        for path in ["0", "24.1", "12", "16", "26", "21"] {
            let steps: Vec<usize> = path.split('.').map(|p| p.parse().expect("index")).collect();
            assert!(d.entity_at(0, &steps).is_some(), "{path}");
        }
        // Without the option the document renders exactly as before.
        assert_eq!(render(&d, &SvgOptions::default()), PINNED_DEFAULT);
    }

    #[test]
    fn entity_references_of_nested_insert_attributes_use_the_outer_insert() {
        let mut inner = Entity::insert("LEAF", Point3::xy(1.0, 0.0));
        inner.attributes.push(crate::model::Attribute {
            tag: "T".into(),
            value: Value::from("leaf"),
            position: Some(Point3::xy(1.0, 1.0)),
            display: Some(crate::model::AttributeDisplay::new(1.0, 1.0)),
            ..Default::default()
        });
        let group = Entity::new(EntityKind::Group {
            group_kind: GroupKind::Other,
            name: None,
            origin: None,
            children: vec![
                line(0.0, 0.0, 1.0, 1.0),
                Entity::insert("NEST", Point3::xy(5.0, 5.0)),
            ],
        });
        let mut d = doc(vec![line(0.0, 0.0, 9.0, 9.0), group]);
        d.blocks.push(Block {
            name: "NEST".into(),
            entities: vec![inner],
            ..Block::default()
        });
        d.blocks.push(Block {
            name: "LEAF".into(),
            entities: vec![line(0.0, 0.0, 0.5, 0.0)],
            ..Block::default()
        });
        let s = render(&d, &SvgOptions::default().with_entity_refs(true));
        assert!(s.contains("<path data-e=\"1.1\" d=\"M6 -5L6.5 -5\""), "{s}");
        assert!(s.contains("<text data-e=\"1.1\" x=\"6\" y=\"-6\""), "{s}");
        assert!(d.entity_at(0, &[1, 1]).is_some());
    }

    #[test]
    fn points_as_pixel_dots() {
        let d = doc(vec![
            line(0.0, 0.0, 1000.0, 0.0),
            Entity::new(EntityKind::Point {
                position: Point3::xy(1.0, 1.0),
            })
            .with_linetype("DASHED"),
        ]);
        let s = render(&d, &SvgOptions::default().with_point_px(Some(4.0)));
        assert!(
            s.contains("<path d=\"M1 -1h0\" stroke=\"#000000\" vector-effect=\"non-scaling-stroke\" stroke-width=\"4\" stroke-linecap=\"round\"/>"),
            "{s}"
        );
        let crosses = render(&d, &SvgOptions::default());
        assert!(
            crosses.contains("M-3.16 -1L5.16 -1M1 -5.16L1 3.16"),
            "{crosses}"
        );
        for bad in [f64::NAN, 0.0, -2.0, f64::NEG_INFINITY] {
            assert_eq!(
                render(&d, &SvgOptions::default().with_point_px(Some(bad))),
                crosses
            );
        }
        let huge = render(&d, &SvgOptions::default().with_point_px(Some(1e9)));
        assert!(huge.contains("stroke-width=\"1000\""), "{huge}");
    }

    #[test]
    fn text_metrics_and_font_family() {
        let text = |halign: HAlign, end: Option<Point3>| {
            Entity::new(EntityKind::Text {
                position: Point3::xy(0.0, 0.0),
                end_point: end,
                height: 1.4,
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                value: "AB".into(),
                style: None,
                halign,
                valign: VAlign::Baseline,
                normal: Vec3::Z,
            })
        };
        let mut tagged = Entity::insert("B", Point3::xy(0.0, 5.0));
        tagged.attributes.push(crate::model::Attribute {
            tag: "T".into(),
            value: Value::from("x"),
            position: Some(Point3::xy(0.0, 5.0)),
            display: Some(crate::model::AttributeDisplay::new(0.7, 0.7)),
            ..Default::default()
        });
        let mut d = doc(vec![
            text(HAlign::Left, None),
            text(HAlign::Fit, Some(Point3::xy(4.0, 0.0))),
            text(HAlign::Aligned, Some(Point3::xy(4.0, 0.0))),
            tagged,
        ]);
        d.blocks.push(Block {
            name: "B".into(),
            entities: vec![line(0.0, 0.0, 1.0, 0.0)],
            ..Block::default()
        });
        let opts = SvgOptions::default()
            .with_cap_height_ratio(Some(0.7))
            .with_font_family(Some("\"Noto Sans\", sans-serif"));
        let s = render(&d, &opts);
        assert!(
            s.contains(" font-family=\"&quot;Noto Sans&quot;, sans-serif\">"),
            "{s}"
        );
        assert_eq!(s.matches("font-family").count(), 1, "{s}");
        assert!(
            s.contains("font-size=\"2\" text-anchor=\"start\" fill"),
            "{s}"
        );
        assert!(
            s.contains("font-size=\"2\" text-anchor=\"start\" textLength=\"4\""),
            "{s}"
        );
        // Aligned text is sized by its baseline: 4 / (0.6 * 2 chars).
        assert!(s.contains("font-size=\"3.333333\""), "{s}");
        assert!(
            s.contains("font-size=\"1\" text-anchor=\"start\" fill=\"#000000\">x<"),
            "{s}"
        );
        let plain = render(&d, &SvgOptions::default());
        assert!(plain.contains("font-size=\"1.4\""), "{plain}");
        assert!(!plain.contains("font-family"));
        for bad in [f64::NAN, 0.0, -0.7, 100.0, f64::INFINITY] {
            assert_eq!(
                render(&d, &SvgOptions::default().with_cap_height_ratio(Some(bad))),
                plain
            );
        }
        assert_eq!(
            render(&d, &SvgOptions::default().with_font_family(Some(""))),
            plain
        );
    }

    fn square_hatch(x: f64, size: f64, scale: f64) -> Entity {
        let p = Point3::xy;
        Entity::new(EntityKind::Hatch {
            loops: vec![HatchLoop {
                edges: vec![HatchEdge::Polyline {
                    vertices: vec![
                        Vertex::at(p(x, 0.0)),
                        Vertex::at(p(x + size, 0.0)),
                        Vertex::at(p(x + size, size)),
                        Vertex::at(p(x, size)),
                    ],
                    closed: true,
                }],
                external: true,
            }],
            solid: false,
            pattern: Some("ANSI31".into()),
            pattern_scale: scale,
            pattern_angle: 0.0,
            normal: Vec3::Z,
        })
    }

    fn pattern_widths(s: &str) -> Vec<String> {
        s.split("<pattern ")
            .skip(1)
            .filter_map(|p| p.split("width=\"").nth(1))
            .filter_map(|w| w.split('"').next())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn native_hatch_spacing_follows_the_pattern_scale_with_a_line_floor() {
        let d = doc(vec![
            line(0.0, 0.0, 10_000.0, 0.0),
            square_hatch(0.0, 10.0, 1.0),
            square_hatch(20.0, 10.0, 2.0),
            square_hatch(0.0, 10_000.0, 0.001),
            square_hatch(40.0, 10.0, f64::NAN),
        ]);
        // Clamped to 0.8 % of the 10 400-unit view: one line across a 10-unit square.
        assert_eq!(
            pattern_widths(&render(&d, &SvgOptions::default())),
            ["83.2"]
        );
        let s = render(&d, &SvgOptions::default().with_hatch_native_spacing(true));
        // Own spacing, 2x scale, the floor of 500 lines across a 10 000-unit hatch (20), and
        // the default scale for a non-finite one (shares the first tile).
        // Pattern spacing uses the drawing's number precision (two decimals here).
        assert_eq!(pattern_widths(&s), ["3.17", "6.35", "20"], "{s}");
        assert_eq!(s.matches("url(#cadkit-hatch-1)").count(), 2, "{s}");
        // Tiles stay pattern-local with an origin.
        let shifted = render(
            &d,
            &SvgOptions::default()
                .with_hatch_native_spacing(true)
                .with_origin(SvgOrigin::Explicit { x: 5000.0, y: 0.0 }),
        );
        assert!(
            shifted.contains("<line x1=\"0\" y1=\"0\" x2=\"3.17\" y2=\"0\""),
            "{shifted}"
        );
    }

    /// Model space with lines from (0, 0) to (100, 100) and a layout whose first viewport is
    /// the sheet (10 x 8 around (5, 4)) and whose second shows the 20-unit-high area around
    /// (50, 50) in a 4 x 4 window around (5, 4).
    fn layout_document() -> Document {
        let viewport = |center: Point3, size: f64, view_center: Point3, view_height: f64| {
            Entity::new(EntityKind::Viewport {
                center,
                width: size,
                height: size,
                view_center,
                view_height,
            })
        };
        let mut d = doc(vec![
            line(0.0, 0.0, 100.0, 100.0),
            line(0.0, 100.0, 100.0, 0.0),
        ]);
        let mut sheet = viewport(Point3::xy(5.0, 4.0), 8.0, Point3::xy(5.0, 4.0), 8.0);
        if let EntityKind::Viewport { width, .. } = &mut sheet.kind {
            *width = 10.0;
        }
        d.models.push(Model {
            name: "Layout1".into(),
            kind: ModelKind::Layout,
            entities: vec![
                sheet,
                viewport(Point3::xy(5.0, 4.0), 4.0, Point3::xy(50.0, 50.0), 20.0),
                line(0.0, 0.0, 1.0, 0.0),
            ],
            ..Model::default()
        });
        d
    }

    #[test]
    fn layout_viewports_show_model_space() {
        let d = layout_document();
        let base = SvgOptions::default().with_model(1);
        let off = render(&d, &base);
        assert!(!off.contains("cadkit-model"), "{off}");
        let s = render(&d, &base.clone().with_layout_viewports(true));
        assert!(
            s.contains("<defs>\n<g id=\"cadkit-model\" data-model=\"0\">\n<g data-layer=\"0\""),
            "{s}"
        );
        assert!(s.contains("<path d=\"M0 0L100 -100\""), "{s}");
        // The sheet viewport is skipped; the window maps model (50, 50) to sheet (5, 4).
        assert_eq!(s.matches("<clipPath").count(), 1, "{s}");
        assert!(
            s.contains("<clipPath id=\"cadkit-viewport-1\"><rect x=\"3\" y=\"-6\" width=\"4\" height=\"4\"/></clipPath>"),
            "{s}"
        );
        assert!(
            s.contains("<g clip-path=\"url(#cadkit-viewport-1)\" data-scale=\"5\"><use href=\"#cadkit-model\" transform=\"matrix(0.2 0 0 0.2 -5 6)\"/></g>"),
            "{s}"
        );
        assert!(
            s.contains("<path d=\"M3 -6h4v4h-4Z\" stroke=\"#000000\""),
            "{s}"
        );
        // Viewports draw under the sheet's own entities.
        let vp = s.find("data-kind=\"viewports\"").expect("viewports");
        let own = s.rfind("<g data-layer=\"0\"").expect("sheet layer");
        assert!(vp < own, "{s}");
        // Extents: the window and the sheet's own line.
        assert!(s.contains("viewBox=\"-0.14 -6.14 7.28 6.28\""), "{s}");

        // With an origin and references, both spaces are written relative to their own origin.
        let s = render(
            &d,
            &base
                .clone()
                .with_layout_viewports(true)
                .with_entity_refs(true)
                .with_origin(SvgOrigin::Auto),
        );
        assert!(s.contains("data-origin=\"0 0\""), "{s}");
        assert!(
            s.contains("<g id=\"cadkit-model\" data-model=\"0\" data-origin=\"100 100\">"),
            "{s}"
        );
        assert!(s.contains("<path data-e=\"0\" d=\"M-100 100L0 0\""), "{s}");
        // Model view (sx, sy) is drawing (100 + sx, 100 - sy): (50, 50) is view (-50, 50),
        // which lands on sheet view (5, -4).
        assert!(
            s.contains("transform=\"matrix(0.2 0 0 0.2 15 -14)\""),
            "{s}"
        );
        assert!(s.contains("<path data-e=\"1\" d=\"M3 -6h4v4h-4Z\""), "{s}");
        assert!(s.contains("data-e=\"1\" data-scale=\"5\""), "{s}");
    }

    #[test]
    fn layout_viewports_with_hostile_or_missing_parts_fall_back() {
        let mut d = layout_document();
        let opts = SvgOptions::default()
            .with_model(1)
            .with_layout_viewports(true);
        let plain = render(&d, &SvgOptions::default().with_model(1));
        if let Some(layout) = d.models.get_mut(1) {
            for e in &mut layout.entities {
                if let EntityKind::Viewport {
                    width,
                    view_height,
                    center,
                    ..
                } = &mut e.kind
                    && *width < 6.0
                {
                    *view_height = f64::NAN;
                    *width = -1.0;
                    center.x = f64::INFINITY;
                }
            }
        }
        assert_eq!(render(&d, &opts), plain);
        // A sheet-only layout and a document without model space.
        let mut d = layout_document();
        if let Some(layout) = d.models.get_mut(1) {
            layout.entities.remove(1);
        }
        assert_eq!(render(&d, &opts), plain);
        let mut d = layout_document();
        if let Some(model) = d.models.get_mut(0) {
            model.kind = ModelKind::Sheet;
        }
        let s = render(&d, &opts);
        assert!(!s.contains("<use") && s.contains("M3 -6h4v4h-4Z"), "{s}");
        // Viewport sizes of zero.
        let mut d = layout_document();
        if let Some(layout) = d.models.get_mut(1) {
            for e in &mut layout.entities {
                if let EntityKind::Viewport { height, .. } = &mut e.kind {
                    *height = 0.0;
                }
            }
        }
        assert_eq!(render(&d, &opts), plain);
    }

    #[test]
    fn layout_viewports_respect_work_and_output_budgets() {
        let mut d = layout_document();
        if let Some(model) = d.models.get_mut(0) {
            model.entities = (0..200)
                .map(|i| line(0.0, f64::from(i), 100.0, 0.0))
                .collect();
        }
        let opts = SvgOptions::default()
            .with_model(1)
            .with_layout_viewports(true)
            .with_entity_refs(true);
        let expected = render_all_layers(&d, &opts);
        assert!(expected.contains("<use"), "{expected}");
        let exact = opts.clone().with_max_output_bytes(expected.len());
        assert_eq!(render_bounded(&d, &exact, true).expect("exact"), expected);
        assert!(matches!(
            render_bounded(&d, &exact.with_max_output_bytes(expected.len() - 1), true),
            Err(crate::Error::LimitExceeded {
                limit: crate::LimitKind::Output,
                ..
            })
        ));
        // The sheet costs a few units; model space does not fit in the rest.
        let small = opts.clone().with_max_work(100);
        let s = render(&d, &small);
        assert!(s.contains("data-truncated=\"true\""), "{s}");
        assert!(matches!(
            render_bounded(&d, &small, false),
            Err(crate::Error::LimitExceeded {
                limit: crate::LimitKind::Work,
                ..
            })
        ));
        let s = render(&d, &opts.clone().with_max_output_bytes(2000));
        assert!(
            s.contains("data-truncated=\"true\"") && s.len() < 4000,
            "{s}"
        );
    }

    #[test]
    fn bounded_rendering_is_exact_with_every_option() {
        let d = every_kind_document(500_000.0, 4_000_000.0);
        let opts = SvgOptions::default()
            .with_origin(SvgOrigin::Auto)
            .with_entity_refs(true)
            .with_point_px(Some(3.0))
            .with_cap_height_ratio(Some(0.7))
            .with_font_family(Some("serif"))
            .with_hatch_native_spacing(true)
            .with_layout_viewports(true);
        for hidden in [false, true] {
            let expected = if hidden {
                render_all_layers(&d, &opts)
            } else {
                render(&d, &opts)
            };
            assert!(!expected.contains("data-truncated"));
            let exact = opts.clone().with_max_output_bytes(expected.len());
            assert_eq!(render_bounded(&d, &exact, hidden).expect("exact"), expected);
            for cap in [expected.len() - 1, expected.len() / 2, 64] {
                assert!(matches!(
                    render_bounded(&d, &exact.clone().with_max_output_bytes(cap), hidden),
                    Err(crate::Error::LimitExceeded {
                        limit: crate::LimitKind::Output,
                        ..
                    })
                ));
            }
            assert!(matches!(
                render_bounded(&d, &opts.clone().with_max_work(20), hidden),
                Err(crate::Error::LimitExceeded {
                    limit: crate::LimitKind::Work,
                    ..
                })
            ));
        }
        // A family name alone larger than the budget is rejected before rendering.
        let long = opts
            .with_font_family(Some("x".repeat(100)))
            .with_max_output_bytes(50);
        assert!(render_bounded(&d, &long, false).is_err());
    }
}
