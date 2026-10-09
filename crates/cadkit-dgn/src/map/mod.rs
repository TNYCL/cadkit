//! Native elements -> `cadkit_core::Document`.

pub(crate) mod v7;
pub(crate) mod v8;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::f64::consts::{FRAC_PI_2, TAU};

use cadkit_core::geom_ops::{arbitrary_axes, clamped_uniform_knots};
use cadkit_core::{
    Attribute, AttributeDisplay, Color, Entity, EntityKind, GroupKind, HAlign, LengthUnit,
    Linetype, Lineweight, Point3, Props, Raw, TextStyle, VAlign, Value, Vec3, Vertex, Warning,
};

use crate::native::element::{
    CellData, Element, ElementData, Rotation, TagData, TagSetData, TagValue, TextData, UorPoint,
};
use crate::native::linkage::{self, LinkageData};
use crate::palette;

/// UOR -> document units: `((p - origin) * scale) * doc_scale`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Xf {
    pub(crate) origin: UorPoint,
    /// Master units per UOR.
    pub(crate) scale: f64,
}

impl Xf {
    pub(crate) fn pt(&self, p: UorPoint) -> Point3 {
        let [x, y, z] = p;
        let [ox, oy, oz] = self.origin;
        Point3::new(
            (x - ox) * self.scale,
            (y - oy) * self.scale,
            (z - oz) * self.scale,
        )
    }

    pub(crate) fn dist(&self, d: f64) -> f64 {
        d * self.scale
    }

    /// The same scale without the origin shift (block definitions are origin-relative).
    pub(crate) fn relative(&self) -> Xf {
        Xf {
            origin: [0.0; 3],
            scale: self.scale,
        }
    }
}

/// One element ready for mapping.
pub(crate) struct Item<'a> {
    pub(crate) el: Element,
    pub(crate) raw: &'a [u8],
    /// Offset for diagnostics (V8: in the inflated page, V7: in the file).
    pub(crate) offset: u64,
}

/// Complex element hierarchy over [`Item`] indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) idx: usize,
    pub(crate) children: Vec<Node>,
}

impl Node {
    fn leaf(idx: usize) -> Self {
        Self {
            idx,
            children: Vec::new(),
        }
    }
}

/// Builds the V8 hierarchy from the role bits and declared component counts.
///
/// A component decrements the innermost open header; a non-component closes every open
/// header (writers sometimes declare more components than they store). Headers whose
/// count is unknown accept components until the next non-component. Nesting deeper than
/// `max_depth` is flattened.
pub(crate) fn tree_v8(items: &[Item<'_>], max_depth: u32) -> Vec<Node> {
    struct Open {
        node: Node,
        remaining: Option<u32>,
    }
    fn close(stack: &mut Vec<Open>, roots: &mut Vec<Node>) {
        if let Some(open) = stack.pop() {
            match stack.last_mut() {
                Some(parent) => parent.node.children.push(open.node),
                None => roots.push(open.node),
            }
        }
    }
    let mut roots = Vec::new();
    let mut stack: Vec<Open> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        while stack.last().is_some_and(|o| o.remaining == Some(0)) {
            close(&mut stack, &mut roots);
        }
        let h = &item.el.header;
        if !(h.complex_component && !stack.is_empty()) {
            while !stack.is_empty() {
                close(&mut stack, &mut roots);
            }
        } else if let Some(top) = stack.last_mut() {
            top.remaining = top.remaining.map(|r| r.saturating_sub(1));
        }
        let count = item.el.data.child_count();
        let opens = h.complex_header && count != Some(0) && stack.len() < max_depth as usize;
        if opens {
            stack.push(Open {
                node: Node::leaf(i),
                remaining: count,
            });
        } else {
            match stack.last_mut() {
                Some(top) => top.node.children.push(Node::leaf(i)),
                None => roots.push(Node::leaf(i)),
            }
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut roots);
    }
    roots
}

/// Builds the V7 hierarchy from description lengths (`ends[i]` = end offset of a header).
pub(crate) fn tree_v7(items: &[Item<'_>], ends: &[Option<u64>], max_depth: u32) -> Vec<Node> {
    let mut roots: Vec<Node> = Vec::new();
    let mut stack: Vec<(Node, u64)> = Vec::new();
    let close = |stack: &mut Vec<(Node, u64)>, roots: &mut Vec<Node>| {
        if let Some((n, _)) = stack.pop() {
            match stack.last_mut() {
                Some((p, _)) => p.children.push(n),
                None => roots.push(n),
            }
        }
    };
    for (i, item) in items.iter().enumerate() {
        while stack.last().is_some_and(|(_, end)| *end <= item.offset) {
            close(&mut stack, &mut roots);
        }
        if !item.el.header.complex_component {
            while !stack.is_empty() {
                close(&mut stack, &mut roots);
            }
        }
        let end = ends.get(i).copied().flatten().filter(|&e| e > item.offset);
        match end {
            Some(e) if stack.len() < max_depth as usize => stack.push((Node::leaf(i), e)),
            _ => match stack.last_mut() {
                Some((p, _)) => p.children.push(Node::leaf(i)),
                None => roots.push(Node::leaf(i)),
            },
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut roots);
    }
    roots
}

/// Standard line style names (index = style number).
const STYLE_NAMES: [&str; 8] = [
    "DGN_SOLID",
    "DGN_DOTTED",
    "DGN_MEDIUM_DASH",
    "DGN_LONG_DASH",
    "DGN_DOT_DASH",
    "DGN_SHORT_DASH",
    "DGN_DASH_DOUBLE_DOT",
    "DGN_LONG_DASH_SHORT_DASH",
];

/// Weight step -> line width in millimeters: a weight-`w` line is drawn `1 + 2w` pixels
/// wide; cadkit assumes 0.1 mm per pixel (weight 0 = 0.1 mm, 1 = 0.3 mm, 31 = 6.3 mm).
pub(crate) fn weight_mm(w: u32) -> f64 {
    0.1 * f64::from(1 + 2 * w)
}

/// Shared mapping state.
pub(crate) struct Mapper {
    pub(crate) keep_raw: bool,
    pub(crate) max_depth: u32,
    pub(crate) palette: [[u8; 3]; 256],
    /// Level id -> layer name.
    pub(crate) levels: HashMap<u32, String>,
    pub(crate) used_levels: BTreeSet<u32>,
    pub(crate) used_styles: BTreeSet<u32>,
    /// Font number -> name.
    pub(crate) fonts: HashMap<u32, String>,
    pub(crate) used_fonts: BTreeSet<u32>,
    /// V8: tag set definition element id -> set; V7: set number -> set.
    pub(crate) tag_sets: HashMap<u64, TagSetData>,
    /// Target element id -> (tag item index, attribute) waiting to be attached.
    pub(crate) pending_tags: HashMap<u64, Vec<(usize, Attribute)>>,
    /// Item indices of tags that are attached to a target (not emitted as entities).
    pub(crate) attached: HashSet<usize>,
    /// XAttribute record kinds per element id.
    pub(crate) xattributes: HashMap<u64, Vec<u32>>,
    /// Raster frame id -> image path.
    pub(crate) raster_paths: HashMap<u64, (Option<String>, Option<String>)>,
    /// Aggregated warnings: code -> (count, first message).
    counts: BTreeMap<String, (u64, String)>,
    pub(crate) warnings: Vec<Warning>,
}

impl Mapper {
    pub(crate) fn new(keep_raw: bool, max_depth: u32) -> Self {
        Self {
            keep_raw,
            max_depth,
            palette: palette::DEFAULT_PALETTE,
            levels: HashMap::new(),
            used_levels: BTreeSet::new(),
            used_styles: BTreeSet::new(),
            fonts: HashMap::new(),
            used_fonts: BTreeSet::new(),
            tag_sets: HashMap::new(),
            pending_tags: HashMap::new(),
            attached: HashSet::new(),
            xattributes: HashMap::new(),
            raster_paths: HashMap::new(),
            counts: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    /// Records a warning that may repeat many times; reported once with a count.
    pub(crate) fn note(&mut self, code: &str, message: impl FnOnce() -> String) {
        let entry = self
            .counts
            .entry(code.to_owned())
            .or_insert_with(|| (0, message()));
        entry.0 += 1;
    }

    pub(crate) fn warn(
        &mut self,
        code: &str,
        message: String,
        offset: Option<u64>,
        object: Option<u64>,
    ) {
        self.warnings.push(Warning {
            code: code.into(),
            message,
            offset,
            object,
        });
    }

    /// Moves aggregated notes into the warning list.
    pub(crate) fn finish_warnings(&mut self) -> Vec<Warning> {
        let mut out = std::mem::take(&mut self.warnings);
        for (code, (n, msg)) in std::mem::take(&mut self.counts) {
            let message = if n > 1 {
                format!("{msg} ({n} occurrences)")
            } else {
                msg
            };
            out.push(Warning {
                code,
                message,
                offset: None,
                object: None,
            });
        }
        out
    }

    pub(crate) fn level_name(&mut self, id: u32) -> String {
        self.used_levels.insert(id);
        self.levels
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("Level {id}"))
    }

    pub(crate) fn color(&self, index: u32) -> Color {
        match palette::lookup(&self.palette, index) {
            Some([r, g, b]) => Color::Rgb { r, g, b },
            None => Color::ByLayer,
        }
    }

    /// Text styles for every font referenced by text.
    pub(crate) fn text_styles(&self) -> Vec<TextStyle> {
        self.used_fonts
            .iter()
            .map(|f| {
                let name = self.font_style_name(*f);
                let mut props = Props::new();
                props.insert("dgn.font_number".into(), Value::Int(i64::from(*f)));
                TextStyle {
                    font: self.fonts.get(f).cloned(),
                    name,
                    height: 0.0,
                    width_factor: 1.0,
                    oblique: 0.0,
                    props,
                }
            })
            .collect()
    }

    fn font_style_name(&self, f: u32) -> String {
        self.fonts
            .get(&f)
            .cloned()
            .unwrap_or_else(|| format!("DGN_FONT_{f}"))
    }

    /// Tag set definitions as `props["dgn.tag_sets"]` (the model has no table for them).
    ///
    /// Shape (positional lists, see FORMAT_NOTES "Tag sets in props"):
    /// `[[set name | null, key, [[tag number, name, prompt, value type, default, flags], ...]], ...]`
    /// where `key` is the V8 definition element id or the V7 set number and `flags` holds
    /// the 5 uninterpreted definition bytes. Sorted by key; `None` when the file has none.
    pub(crate) fn tag_sets_value(&self) -> Option<Value> {
        if self.tag_sets.is_empty() {
            return None;
        }
        let mut keys: Vec<&u64> = self.tag_sets.keys().collect();
        keys.sort();
        let sets = keys
            .into_iter()
            .filter_map(|k| {
                let set = self.tag_sets.get(k)?;
                let tags = set
                    .tags
                    .iter()
                    .map(|d| {
                        Value::List(vec![
                            Value::Int(i64::from(d.id)),
                            Value::Text(d.name.clone()),
                            Value::Text(d.prompt.clone()),
                            Value::Int(i64::from(d.value_type)),
                            tag_value(&d.default),
                            Value::Bytes(d.flags.to_vec()),
                        ])
                    })
                    .collect();
                Some(Value::List(vec![
                    set.name.clone().map_or(Value::Null, Value::Text),
                    Value::Int(i64::try_from(*k).unwrap_or(i64::MAX)),
                    Value::List(tags),
                ]))
            })
            .collect();
        Some(Value::List(sets))
    }

    /// Linetype definitions for every style referenced.
    pub(crate) fn linetypes(&self) -> Vec<Linetype> {
        self.used_styles
            .iter()
            .map(|s| {
                let mut props = Props::new();
                props.insert("dgn.style".into(), Value::Int(i64::from(*s)));
                Linetype {
                    name: style_name(*s),
                    description: None,
                    pattern: Vec::new(),
                    props,
                }
            })
            .collect()
    }

    /// Common symbology, identity and props of an element.
    fn base(&mut self, item: &Item<'_>, kind: EntityKind) -> Entity {
        let h = &item.el.header;
        let mut e = Entity::new(kind);
        e.id = h.id;
        let mut p = Props::new();
        p.insert("dgn.type".into(), Value::Int(i64::from(h.type_code)));
        if h.has_display_header {
            e.layer = Some(self.level_name(h.level));
            p.insert("dgn.level_id".into(), Value::Int(i64::from(h.level)));
            e.color = self.color(h.color);
            p.insert("dgn.color_index".into(), Value::Int(i64::from(h.color)));
            p.insert("dgn.weight".into(), Value::Int(i64::from(h.weight)));
            if h.weight <= 31 {
                e.lineweight = Lineweight::Millimeters(weight_mm(h.weight));
            }
            p.insert("dgn.style".into(), Value::Int(i64::from(h.style)));
            if h.style < 0x7fff_ff00 {
                self.used_styles.insert(h.style);
                e.linetype = Some(style_name(h.style));
            }
            if h.graphic_group != 0 {
                p.insert(
                    "dgn.graphic_group".into(),
                    Value::Int(i64::from(h.graphic_group)),
                );
            }
            p.insert("dgn.properties".into(), Value::Int(i64::from(h.properties)));
            // Undocumented type-word flags MicroStation sets on its graphics (`0x00C0`);
            // kept so a rewrite reproduces them.
            if h.type_flags & 0x00c0 != 0 {
                p.insert(
                    "dgn.type_flags".into(),
                    Value::Int(i64::from(h.type_flags & 0x00c0)),
                );
            }
            if h.properties & 0x8000 != 0 && matches!(h.type_code, 6 | 14) {
                p.insert("dgn.hole".into(), Value::Bool(true));
            }
        }
        // Element ids come from the file and may repeat: the XAttributes of an id are
        // attached to the first entity carrying it only, with the kind list capped.
        if let Some(kinds) = h.id.and_then(|id| self.xattributes.remove(&id)) {
            p.insert(
                "dgn.xattribute_count".into(),
                Value::Int(kinds.len() as i64),
            );
            p.insert(
                "dgn.xattribute_kinds".into(),
                Value::List(
                    kinds
                        .iter()
                        .take(MAX_XATTRIBUTE_KINDS)
                        .map(|k| Value::Int(i64::from(*k)))
                        .collect(),
                ),
            );
        }
        linkage_props(&item.el, &mut p);
        if let Some(problem) = &item.el.problem {
            p.insert("dgn.decode_problem".into(), Value::Text(problem.clone()));
            let t = h.type_code;
            self.note(&format!("dgn.decode_failed.type_{t}"), || {
                format!("type {t} element not decoded: {problem}")
            });
        }
        e.props = p;
        if self.keep_raw {
            e.raw = Some(Raw {
                type_code: u32::from(h.type_code),
                bytes: item.raw.to_vec(),
            });
        }
        if let Some(id) = h.id {
            if let Some(attrs) = self.pending_tags.remove(&id) {
                e.attributes = attrs
                    .into_iter()
                    .map(|(_, a)| relative_to_owner(a, &e))
                    .collect();
            }
        }
        e
    }

    /// Converts a node (and its children) to an entity.
    pub(crate) fn entity(
        &mut self,
        items: &[Item<'_>],
        node: &Node,
        xf: &Xf,
        depth: u32,
    ) -> Option<Entity> {
        let item = items.get(node.idx)?;
        let kind = match &item.el.data {
            ElementData::Line { start, end } => {
                let (s, e) = (xf.pt(*start), xf.pt(*end));
                if s == e {
                    EntityKind::Point { position: s }
                } else {
                    EntityKind::Line { start: s, end: e }
                }
            }
            ElementData::LineString { points } => polyline(points, xf, false),
            ElementData::Shape { points } => polyline(points, xf, true),
            ElementData::Curve { points } => curve(points, xf),
            ElementData::BSplinePoles { points } if node.children.is_empty() => {
                EntityKind::Spline {
                    degree: 3.min(points.len().saturating_sub(1)) as u32,
                    knots: clamped_uniform_knots(
                        points.len(),
                        3.min(points.len().saturating_sub(1)),
                    ),
                    control_points: points.iter().map(|p| xf.pt(*p)).collect(),
                    weights: Vec::new(),
                    fit_points: Vec::new(),
                    closed: false,
                }
            }
            ElementData::PointString { points, .. } => EntityKind::Group {
                group_kind: GroupKind::Other,
                name: Some("point string".into()),
                origin: None,
                children: points
                    .iter()
                    .map(|p| {
                        Entity::new(EntityKind::Point {
                            position: xf.pt(*p),
                        })
                    })
                    .collect(),
            },
            ElementData::Ellipse {
                primary,
                secondary,
                rotation,
                center,
            } => conic(
                xf.pt(*center),
                xf.dist(*primary),
                xf.dist(*secondary),
                rotation,
                0.0,
                TAU,
            ),
            ElementData::Arc {
                start,
                sweep,
                primary,
                secondary,
                rotation,
                center,
            } => conic(
                xf.pt(*center),
                xf.dist(*primary),
                xf.dist(*secondary),
                rotation,
                *start,
                *sweep,
            ),
            ElementData::Text(t) => self.text(t, xf),
            ElementData::BSplineCurve { .. } => self.bspline(items, node, xf),
            ElementData::TextNode(t) => EntityKind::Group {
                group_kind: GroupKind::TextNode,
                name: None,
                origin: Some(xf.pt(t.origin)),
                children: self.children(items, node, xf, depth),
            },
            ElementData::Complex { .. } => {
                let group_kind = match item.el.header.type_code {
                    12 => GroupKind::ComplexChain,
                    14 => GroupKind::ComplexShape,
                    _ => GroupKind::Other,
                };
                EntityKind::Group {
                    group_kind,
                    name: None,
                    origin: None,
                    children: self.children(items, node, xf, depth),
                }
            }
            ElementData::Cell(c) => EntityKind::Group {
                group_kind: GroupKind::Cell,
                name: c
                    .name
                    .clone()
                    .or_else(|| linkage::string(&item.el.linkages, 1)),
                origin: Some(xf.pt(c.origin)),
                children: self.children(items, node, xf, depth),
            },
            ElementData::SharedCellInstance(c) => self.insert(item, c, xf),
            ElementData::Tag(t) => {
                // Tags normally become attributes of their target; this one has none.
                let mut e = self.base(
                    item,
                    EntityKind::Unknown {
                        type_name: "dgn.type_37".into(),
                    },
                );
                let a = self.attribute(t, &item.el.header, xf);
                e.attributes.push(relative_to_owner(a, &e));
                return Some(e);
            }
            ElementData::RasterFrame { .. } => self.image(item, xf),
            _ if !node.children.is_empty() => EntityKind::Group {
                group_kind: GroupKind::Other,
                name: None,
                origin: None,
                children: self.children(items, node, xf, depth),
            },
            _ => {
                let t = item.el.header.type_code;
                if item.el.problem.is_none() {
                    self.note(&format!("dgn.unknown_element.type_{t}"), || {
                        format!("type {t} elements are kept as Unknown entities")
                    });
                }
                EntityKind::Unknown {
                    type_name: format!("dgn.type_{t}"),
                }
            }
        };
        let mut e = self.base(item, kind);
        self.extra_props(item, &mut e, xf);
        Some(e)
    }

    fn children(&mut self, items: &[Item<'_>], node: &Node, xf: &Xf, depth: u32) -> Vec<Entity> {
        if depth >= self.max_depth {
            self.note("dgn.depth_limit", || {
                "complex nesting deeper than the depth limit was cut".into()
            });
            return Vec::new();
        }
        let mut out = Vec::with_capacity(node.children.len());
        for c in &node.children {
            if self.attached.contains(&c.idx) {
                continue;
            }
            if let Some(e) = self.entity(items, c, xf, depth + 1) {
                out.push(e);
            }
        }
        out
    }

    fn extra_props(&mut self, item: &Item<'_>, e: &mut Entity, xf: &Xf) {
        match &item.el.data {
            ElementData::Text(t) => {
                // The stored lower-left origin and the length used to place the anchor.
                let o = xf.pt(t.origin);
                e.props.insert(
                    "dgn.origin".into(),
                    Value::List(vec![
                        Value::Float(o.x),
                        Value::Float(o.y),
                        Value::Float(o.z),
                    ]),
                );
                let (length, estimated) = text_length(t);
                e.props
                    .insert("dgn.text_length".into(), Value::Float(xf.dist(length)));
                if estimated {
                    e.props
                        .insert("dgn.text_length_estimated".into(), Value::Bool(true));
                }
                e.props.insert(
                    "dgn.justification".into(),
                    Value::Int(i64::from(t.justification)),
                );
                e.props
                    .insert("dgn.font_number".into(), Value::Int(i64::from(t.font)));
                e.props.insert(
                    "dgn.text_encoding".into(),
                    Value::Text(t.encoding.as_str().into()),
                );
            }
            ElementData::TextNode(t) => {
                e.props.insert(
                    "dgn.node_number".into(),
                    Value::Int(i64::from(t.node_number)),
                );
                e.props.insert(
                    "dgn.justification".into(),
                    Value::Int(i64::from(t.justification)),
                );
                e.props.insert(
                    "dgn.line_spacing".into(),
                    Value::Float(xf.dist(t.line_spacing)),
                );
                e.props
                    .insert("dgn.text_height".into(), Value::Float(xf.dist(t.height)));
            }
            ElementData::Curve { points } => {
                e.props
                    .insert("dgn.curve_points".into(), points_value(points, xf));
            }
            ElementData::PointString { .. } => {
                e.props.insert("dgn.point_string".into(), Value::Bool(true));
            }
            ElementData::RasterFrame { matrix, .. } => {
                // Pixel-to-UOR matrix as stored; the image rectangle comes from the range.
                if let Some([lo, hi]) = item.el.header.range {
                    let (a, b) = (xf.pt(lo), xf.pt(hi));
                    e.props.insert(
                        "dgn.range".into(),
                        Value::List([a.x, a.y, a.z, b.x, b.y, b.z].map(Value::Float).to_vec()),
                    );
                }
                e.props.insert(
                    "dgn.raster_matrix".into(),
                    Value::List(matrix.iter().map(|v| Value::Float(*v)).collect()),
                );
                let id = item.el.header.id.unwrap_or(0);
                if let Some((name, full)) = self.raster_paths.get(&id) {
                    if let Some(n) = name {
                        e.props
                            .insert("dgn.raster_file".into(), Value::Text(n.clone()));
                    }
                    if let Some(f) = full {
                        e.props
                            .insert("dgn.raster_path".into(), Value::Text(f.clone()));
                    }
                }
            }
            ElementData::Cell(c) | ElementData::SharedCellInstance(c) => {
                if let Some(d) = c
                    .description
                    .clone()
                    .or_else(|| linkage::string(&item.el.linkages, 2))
                {
                    e.props.insert("dgn.description".into(), Value::Text(d));
                }
                e.props.insert(
                    "dgn.matrix".into(),
                    Value::List(c.matrix.iter().map(|v| Value::Float(*v)).collect()),
                );
            }
            _ => {}
        }
    }

    fn text(&mut self, t: &TextData, xf: &Xf) -> EntityKind {
        self.used_fonts.insert(t.font);
        let m = t.rotation.matrix();
        let (normal, rotation) = plane_angle(&m);
        let height = xf.dist(t.height);
        let width_factor = if t.height.abs() > 0.0 {
            (t.width / t.height).abs()
        } else {
            1.0
        };
        // DGN stores the lower-left corner; the model wants the point implied by the
        // alignment, so move along the text axes by the justification fractions.
        let (halign, valign, fx, fy) = justification(t.justification);
        let (length, _) = text_length(t);
        let [m0, m1, _, m3, m4, _, m6, m7, _] = m;
        let ux = Vec3::new(m0, m3, m6)
            .normalized()
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        let uy = Vec3::new(m1, m4, m7)
            .normalized()
            .unwrap_or(Vec3::new(0.0, 1.0, 0.0));
        let shift = ux.scaled(fx * length).plus(uy.scaled(fy * t.height.abs()));
        let [ox, oy, oz] = t.origin;
        EntityKind::Text {
            position: xf.pt([ox + shift.x, oy + shift.y, oz + shift.z]),
            end_point: None,
            height,
            rotation,
            width_factor: if width_factor.is_finite() && width_factor > 0.0 {
                width_factor
            } else {
                1.0
            },
            oblique: 0.0,
            value: t.text.clone(),
            style: Some(self.font_style_name(t.font)),
            halign,
            valign,
            normal,
        }
    }

    fn bspline(&mut self, items: &[Item<'_>], node: &Node, xf: &Xf) -> EntityKind {
        let Some(ElementData::BSplineCurve {
            order,
            flags,
            closed,
            num_knots,
            ..
        }) = items.get(node.idx).map(|i| &i.el.data)
        else {
            return EntityKind::Unknown {
                type_name: "dgn.type_27".into(),
            };
        };
        let mut poles: Vec<Point3> = Vec::new();
        let mut knots_in: Vec<f64> = Vec::new();
        let mut weights: Vec<f64> = Vec::new();
        for c in &node.children {
            match items.get(c.idx).map(|i| &i.el.data) {
                Some(ElementData::BSplinePoles { points }) => {
                    poles.extend(points.iter().map(|p| xf.pt(*p)))
                }
                Some(ElementData::BSplineKnots { values }) => knots_in.extend_from_slice(values),
                Some(ElementData::BSplineWeights { values }) => weights.extend_from_slice(values),
                _ => {}
            }
        }
        let order = usize::from(*order).max(2);
        let degree = order - 1;
        let closed = *closed;
        if poles.len() < order {
            return EntityKind::Spline {
                degree: degree as u32,
                knots: Vec::new(),
                control_points: poles,
                weights: Vec::new(),
                fit_points: Vec::new(),
                closed,
            };
        }
        let rational = flags & 0x40 != 0 && weights.len() == poles.len();
        if !rational {
            weights.clear();
        }
        let n = poles.len();
        let explicit = *num_knots > 0 && knots_in.len() >= *num_knots as usize;
        let (control_points, knots, weights) = if closed && !explicit {
            // Periodic uniform curve: unwrap by repeating the first `degree` poles so that an
            // open NURBS with uniform knots traces the closed curve.
            let mut cp = poles.clone();
            cp.extend(poles.iter().take(degree).copied());
            let mut w = weights.clone();
            if !w.is_empty() {
                w.extend(weights.iter().take(degree).copied());
            }
            let k = (0..cp.len() + order).map(|i| i as f64).collect();
            (cp, k, w)
        } else if explicit {
            // Stored knots are the interior knots; the ends are clamped.
            let interior: Vec<f64> = knots_in.iter().take(*num_knots as usize).copied().collect();
            let mut k = vec![0.0; order];
            k.extend(interior);
            k.extend(std::iter::repeat_n(1.0, order));
            if k.len() == n + order {
                (poles, k, weights)
            } else {
                self.note("dgn.bspline_knots", || {
                    "B-spline knot count does not match its poles; uniform knots used".into()
                });
                (poles.clone(), clamped_uniform_knots(n, degree), weights)
            }
        } else {
            (poles.clone(), clamped_uniform_knots(n, degree), weights)
        };
        EntityKind::Spline {
            degree: degree as u32,
            knots,
            control_points,
            weights,
            fit_points: Vec::new(),
            closed,
        }
    }

    fn insert(&mut self, item: &Item<'_>, c: &CellData, xf: &Xf) -> EntityKind {
        let block = c
            .name
            .clone()
            .or_else(|| linkage::string(&item.el.linkages, 1))
            .unwrap_or_else(|| format!("DGN_SHARED_CELL_{}", item.el.header.id.unwrap_or(0)));
        let m = c.matrix;
        let col = |i: usize| {
            Vec3::new(
                m.get(i).copied().unwrap_or(0.0),
                m.get(3 + i).copied().unwrap_or(0.0),
                m.get(6 + i).copied().unwrap_or(0.0),
            )
        };
        let (cx, cy, cz) = (col(0), col(1), col(2));
        let normal = tidy(cx.cross(cy).normalized().unwrap_or(Vec3::Z));
        let (ax, ay, _) = arbitrary_axes(normal);
        let rotation = cy_angle(cx, ax, ay);
        let det = cx.cross(cy).dot(cz);
        let sz = if det < 0.0 { -cz.length() } else { cz.length() };
        EntityKind::Insert {
            block,
            position: xf.pt(c.origin),
            scale: Vec3::new(cx.length(), cy.length(), if sz == 0.0 { 1.0 } else { sz }),
            rotation,
            normal,
            columns: 1,
            rows: 1,
            column_spacing: 0.0,
            row_spacing: 0.0,
        }
    }

    fn image(&mut self, item: &Item<'_>, xf: &Xf) -> EntityKind {
        let id = item.el.header.id.unwrap_or(0);
        let range = item.el.header.range;
        let (matrix_placement, axes) = match &item.el.data {
            ElementData::RasterFrame { matrix, corner } => {
                let [m00, m01, _, _, m10, m11, _, _, m20, m21, ..] = *matrix;
                (
                    frame_placement(matrix, *corner),
                    Some((Vec3::new(m00, m10, m20), Vec3::new(m01, m11, m21))),
                )
            }
            _ => (None, None),
        };
        // The element range is MicroStation's own bounding box: a matrix placement that does
        // not fit inside it is rejected in favour of a placement derived from the range.
        let placement = match (matrix_placement, range) {
            (Some(p), Some(r)) if !within_range(&p, &r, RASTER_RANGE_TOLERANCE) => {
                self.note("dgn.raster_outside_range", || {
                    "raster transform places the image outside its element range; the range was used"
                        .into()
                });
                Some(range_placement(&r, axes))
            }
            (Some(p), _) => Some(p),
            (None, Some(r)) => {
                self.note("dgn.raster_range_placement", || {
                    "raster frame placed from its range (no usable transform)".into()
                });
                Some(range_placement(&r, axes))
            }
            (None, None) => None,
        };
        let Some((origin, u, v)) = placement else {
            return EntityKind::Unknown {
                type_name: "dgn.type_94".into(),
            };
        };
        let path = self
            .raster_paths
            .get(&id)
            .and_then(|(name, full)| full.clone().or_else(|| name.clone()));
        if path.is_none() {
            self.note("dgn.raster_unlinked", || {
                "raster frame without a resolvable attachment".into()
            });
        }
        EntityKind::Image {
            path,
            position: xf.pt(origin),
            u_vector: tidy(u.scaled(xf.scale)),
            v_vector: tidy(v.scaled(xf.scale)),
            size_px: None,
        }
    }

    /// A tag as an attribute (`position` = origin + offset). The tag element's own level,
    /// its text presentation and its symbology are kept so a writer can reproduce it.
    pub(crate) fn attribute(
        &mut self,
        t: &TagData,
        h: &crate::native::element::ElementHeader,
        xf: &Xf,
    ) -> Attribute {
        let set = t
            .set_id
            .or(t.set_number.map(u64::from))
            .and_then(|k| self.tag_sets.get(&k));
        let def = set.and_then(|s| s.tags.iter().find(|d| d.id == t.tag_index));
        let tag = def
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("tag_{}", t.tag_index));
        let set_name = set.and_then(|s| s.name.clone());
        let [ox, oy, oz] = t.origin;
        let [dx, dy, dz] = t.offset;
        let has_pos = t.origin != [0.0; 3] || t.offset != [0.0; 3];
        // Observed: hidden tags carry 0x0080 in the property word (FORMAT_NOTES).
        let invisible = h.properties & 0x0080 != 0;
        let mut props = Props::new();
        let mut layer = None;
        if h.has_display_header {
            layer = Some(self.level_name(h.level));
            props.insert("dgn.color_index".into(), Value::Int(i64::from(h.color)));
            props.insert("dgn.weight".into(), Value::Int(i64::from(h.weight)));
            props.insert("dgn.style".into(), Value::Int(i64::from(h.style)));
            if h.graphic_group != 0 {
                props.insert(
                    "dgn.graphic_group".into(),
                    Value::Int(i64::from(h.graphic_group)),
                );
            }
            // Only MicroStation's undecoded property bits add anything to `invisible`.
            if h.properties & 0x0600 != 0 {
                props.insert("dgn.properties".into(), Value::Int(i64::from(h.properties)));
            }
            if h.type_flags & 0x00c0 != 0 {
                props.insert(
                    "dgn.type_flags".into(),
                    Value::Int(i64::from(h.type_flags & 0x00c0)),
                );
            }
        }
        // V7 tags and hidden V8 tags without a text size have no usable presentation.
        let display = (t.size[1] > 0.0 && t.size[0] > 0.0).then(|| {
            self.used_fonts.insert(t.font);
            props.insert("dgn.font_number".into(), Value::Int(i64::from(t.font)));
            props.insert(
                "dgn.justification".into(),
                Value::Int(i64::from(t.justification)),
            );
            let (halign, valign, _, _) = justification(t.justification);
            AttributeDisplay {
                height: xf.dist(t.size[1]),
                width: xf.dist(t.size[0]),
                style: Some(self.font_style_name(t.font)),
                halign,
                valign,
                rotation: tag_rotation(t.quaternion),
            }
        });
        Attribute {
            tag,
            value: tag_value(&t.value),
            set: set_name,
            position: has_pos.then(|| xf.pt([ox + dx, oy + dy, oz + dz])),
            invisible,
            layer,
            display,
            props,
        }
    }
}

fn style_name(s: u32) -> String {
    STYLE_NAMES
        .get(s as usize)
        .map_or_else(|| format!("DGN_STYLE_{s}"), |n| (*n).to_owned())
}

pub(crate) fn tag_value(v: &TagValue) -> Value {
    match v {
        TagValue::Text(s) => Value::Text(s.clone()),
        TagValue::Int(i) => Value::Int(*i),
        TagValue::Float(f) => Value::Float(*f),
        TagValue::Binary(b) => Value::Bytes(b.clone()),
    }
}

fn points_value(points: &[UorPoint], xf: &Xf) -> Value {
    Value::List(
        points
            .iter()
            .map(|p| {
                let q = xf.pt(*p);
                Value::List(vec![
                    Value::Float(q.x),
                    Value::Float(q.y),
                    Value::Float(q.z),
                ])
            })
            .collect(),
    )
}

fn linkage_props(el: &Element, p: &mut Props) {
    if el.linkages.is_empty() {
        return;
    }
    p.insert(
        "dgn.linkage_ids".into(),
        Value::List(
            el.linkages
                .iter()
                .map(|l| Value::Int(i64::from(l.id)))
                .collect(),
        ),
    );
    for l in &el.linkages {
        match &l.data {
            LinkageData::Database { entity, mslink } => {
                p.insert("dgn.db_entity".into(), Value::Int(i64::from(*entity)));
                p.insert("dgn.mslink".into(), Value::Int(i64::from(*mslink)));
            }
            LinkageData::Fill { color } => {
                p.insert("dgn.fill_color".into(), Value::Int(i64::from(*color)));
            }
            LinkageData::AssocId(id) => {
                p.insert("dgn.assoc_id".into(), Value::Int(i64::from(*id)));
            }
            LinkageData::String {
                string_id, text, ..
            } if *string_id > 2 => {
                p.insert(format!("dgn.string_{string_id}"), Value::Text(text.clone()));
            }
            _ => {}
        }
    }
}

fn polyline(points: &[UorPoint], xf: &Xf, closed: bool) -> EntityKind {
    let mut v: Vec<Vertex> = points.iter().map(|p| Vertex::at(xf.pt(*p))).collect();
    if v.len() == 1 {
        return EntityKind::Point {
            position: v.first().map(|x| x.position).unwrap_or_default(),
        };
    }
    // Closed outlines repeat the first vertex; the model closes implicitly.
    if closed && v.len() > 2 && v.first().map(|x| x.position) == v.last().map(|x| x.position) {
        v.pop();
    }
    // DGN line strings and shapes have no bulges; +Z is the model's convention for them.
    EntityKind::Polyline {
        vertices: v,
        closed,
        normal: Vec3::Z,
    }
}

/// Type 11 curves: the first two and last two points only define the end tangents.
fn curve(points: &[UorPoint], xf: &Xf) -> EntityKind {
    let fit: Vec<Point3> = if points.len() > 4 {
        points
            .iter()
            .skip(2)
            .take(points.len() - 4)
            .map(|p| xf.pt(*p))
            .collect()
    } else {
        points.iter().map(|p| xf.pt(*p)).collect()
    };
    EntityKind::Spline {
        degree: 3,
        knots: Vec::new(),
        control_points: Vec::new(),
        weights: Vec::new(),
        fit_points: fit,
        closed: false,
    }
}

/// Raster frame placement from its pixel-to-UOR matrix (row-major, translation in the
/// last column): the image starts at the translation and its edges follow the matrix X / Y
/// columns, scaled so that the frame reaches its stored extent corner. Handles rotated and
/// sheared frames; `None` when the corner is missing or the system is degenerate.
pub(crate) fn frame_placement(
    m: &[f64; 16],
    corner: Option<[f64; 2]>,
) -> Option<(UorPoint, Vec3, Vec3)> {
    let [m00, m01, _, m03, m10, m11, _, m13, m20, m21, _, m23, ..] = *m;
    let a = Vec3::new(m00, m10, m20);
    let b = Vec3::new(m01, m11, m21);
    // `0x108`/`0x110` is the frame's extent from its origin (the same values as the
    // element range extent on the private sample), not an absolute corner.
    let [dx, dy] = corner?;
    let det = a.x * b.y - a.y * b.x;
    if !det.is_finite() || det.abs() <= 1e-12 * a.length() * b.length() {
        return None;
    }
    let w = (dx * b.y - dy * b.x) / det;
    let h = (a.x * dy - a.y * dx) / det;
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    Some(([m03, m13, m23], a.scaled(w), b.scaled(h)))
}

/// Most XAttribute kinds listed on one entity (the count is always complete).
const MAX_XATTRIBUTE_KINDS: usize = 64;

/// Allowed overshoot of a raster placement beyond its element range, in UOR.
const RASTER_RANGE_TOLERANCE: f64 = 4.0;

/// All four corners of the image rectangle lie inside the range (expanded by `tol`).
fn within_range((o, u, v): &(UorPoint, Vec3, Vec3), [lo, hi]: &[UorPoint; 2], tol: f64) -> bool {
    let [ox, oy, oz] = *o;
    [Vec3::new(0.0, 0.0, 0.0), *u, *v, u.plus(*v)]
        .iter()
        .all(|d| {
            let p = [ox + d.x, oy + d.y, oz + d.z];
            p.iter()
                .zip(lo.iter().zip(hi.iter()))
                .all(|(c, (l, h))| *c >= l.min(*h) - tol && *c <= l.max(*h) + tol)
        })
}

/// The rectangle whose bounding box is the range: edges along the matrix axes when that
/// system is solvable (rotated frames), else the axis-aligned range itself.
fn range_placement([lo, hi]: &[UorPoint; 2], axes: Option<(Vec3, Vec3)>) -> (UorPoint, Vec3, Vec3) {
    let [lx, ly, lz] = *lo;
    let [hx, hy, _] = *hi;
    let (rw, rh) = ((hx - lx).abs(), (hy - ly).abs());
    let aligned = (
        [lx.min(hx), ly.min(hy), lz],
        Vec3::new(rw, 0.0, 0.0),
        Vec3::new(0.0, rh, 0.0),
    );
    let Some((a, b)) = axes.and_then(|(a, b)| Some((a.normalized()?, b.normalized()?))) else {
        return aligned;
    };
    let det = a.x.abs() * b.y.abs() - a.y.abs() * b.x.abs();
    if !det.is_finite() || det.abs() < 1e-9 {
        return aligned;
    }
    let w = (rw * b.y.abs() - rh * b.x.abs()) / det;
    let h = (a.x.abs() * rh - a.y.abs() * rw) / det;
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return aligned;
    }
    let (u, v) = (a.scaled(w), b.scaled(h));
    let x0 = lx.min(hx) - u.x.min(0.0) - v.x.min(0.0);
    let y0 = ly.min(hy) - u.y.min(0.0) - v.y.min(0.0);
    ([x0, y0, lz], u, v)
}

/// DGN justification code -> alignment and the fractions of text length / height between
/// the lower-left origin and the justification point. Codes (GDAL `DGNJ_*`): 0-2 left
/// top/center/bottom, 3-5 left margin, 6-8 center, 9-11 right margin, 12-14 right; the
/// bottom of the DGN text box is the baseline.
pub(crate) fn justification(code: u16) -> (HAlign, VAlign, f64, f64) {
    if code > 14 {
        return (HAlign::Left, VAlign::Baseline, 0.0, 0.0);
    }
    let (h, fx) = match code {
        0..=5 => (HAlign::Left, 0.0),
        6..=8 => (HAlign::Center, 0.5),
        _ => (HAlign::Right, 1.0),
    };
    let (v, fy) = match code % 3 {
        0 => (VAlign::Top, 1.0),
        1 => (VAlign::Middle, 0.5),
        _ => (VAlign::Baseline, 0.0),
    };
    (h, v, fx, fy)
}

/// Keeps only what a tag element does not share with its owner: `layer` stays `None` on the
/// owner's level, and symbology/flag props equal to the owner's are dropped.
fn relative_to_owner(mut a: Attribute, owner: &Entity) -> Attribute {
    if a.layer.is_some() && a.layer == owner.layer {
        a.layer = None;
    }
    a.props
        .retain(|key, value| owner.props.get(key) != Some(value) || key == "dgn.font_number");
    a
}

/// Rotation of a displayed tag about +Z in radians, from its stored quaternion (same
/// convention as text, see [`Rotation::matrix`]). An all-zero quaternion means none.
pub(crate) fn tag_rotation(q: [f64; 4]) -> f64 {
    if q.iter().all(|v| *v == 0.0) {
        return 0.0;
    }
    let [m00, _, _, m10, ..] = Rotation::Quaternion(q).matrix();
    let a = m10.atan2(m00);
    if a.abs() < 1e-12 { 0.0 } else { a }
}

/// Text length in UOR and whether it is estimated (`characters x character width`, the
/// estimate GDAL dgnlib's writer uses for text bounds).
fn text_length(t: &TextData) -> (f64, bool) {
    match t.measured_length {
        Some(l) => (l, false),
        None => (t.text.chars().count() as f64 * t.width.abs(), true),
    }
}

/// Drops negative zeros so serialized vectors stay tidy.
fn tidy(v: Vec3) -> Vec3 {
    Vec3::new(v.x + 0.0, v.y + 0.0, v.z + 0.0)
}

/// Angle of `dir` in the plane spanned by `ax` / `ay`.
fn cy_angle(dir: Vec3, ax: Vec3, ay: Vec3) -> f64 {
    let a = dir.dot(ay).atan2(dir.dot(ax));
    if a.is_finite() { a } else { 0.0 }
}

/// Normal and in-plane rotation (about the normal, from the arbitrary-axis X) of a
/// local-to-world matrix.
fn plane_angle(m: &[f64; 9]) -> (Vec3, f64) {
    let [a, b, _, d, e, _, g, h, _] = *m;
    let ux = Vec3::new(a, d, g);
    let uy = Vec3::new(b, e, h);
    let normal = tidy(ux.cross(uy).normalized().unwrap_or(Vec3::Z));
    let (ax, ay, _) = arbitrary_axes(normal);
    (normal, cy_angle(ux, ax, ay))
}

/// Ellipse or arc with semi-axes `a` (local X) and `b` (local Y), from `start` sweeping
/// `sweep` radians (negative = clockwise) in the local frame given by `rotation`.
pub(crate) fn conic(
    center: Point3,
    a: f64,
    b: f64,
    rotation: &Rotation,
    start: f64,
    sweep: f64,
) -> EntityKind {
    let m = rotation.matrix();
    let [m0, m1, _, m3, m4, _, m6, m7, _] = m;
    let ux = Vec3::new(m0, m3, m6)
        .normalized()
        .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    let uy0 = Vec3::new(m1, m4, m7);
    let normal = tidy(ux.cross(uy0).normalized().unwrap_or(Vec3::Z));
    // Re-orthogonalize the in-plane Y axis.
    let uy = normal.cross(ux);
    let full = sweep.abs() >= TAU - 1e-12;
    // CCW about the normal: a clockwise arc is the same point set travelled backwards.
    let (s, e) = if sweep >= 0.0 {
        (start, start + sweep)
    } else {
        (start + sweep, start)
    };
    let (a, b) = (a.abs(), b.abs());
    if (a - b).abs() <= 1e-9 * a.max(b) {
        if full {
            return EntityKind::Circle {
                center,
                radius: a,
                normal,
            };
        }
        let (ax, ay, _) = arbitrary_axes(normal);
        let dir = |t: f64| ux.scaled(t.cos()).plus(uy.scaled(t.sin()));
        return EntityKind::Arc {
            center,
            radius: a,
            start_angle: cy_angle(dir(s), ax, ay).rem_euclid(TAU),
            end_angle: cy_angle(dir(e), ax, ay).rem_euclid(TAU),
            normal,
        };
    }
    let (major, ratio, shift) = if a >= b {
        (tidy(ux.scaled(a)), b / a, 0.0)
    } else {
        (tidy(uy.scaled(b)), a / b, FRAC_PI_2)
    };
    let (s, e) = if full {
        (0.0, TAU)
    } else {
        (
            (s - shift).rem_euclid(TAU),
            (s - shift).rem_euclid(TAU) + (e - s),
        )
    };
    EntityKind::Ellipse {
        center,
        major_axis: major,
        ratio,
        start_param: s,
        end_param: e,
        normal,
    }
}

/// Picks the named unit for a size in meters (1e-9 relative tolerance).
pub(crate) fn length_unit(meters: f64) -> LengthUnit {
    const UNITS: [LengthUnit; 21] = [
        LengthUnit::Meter,
        LengthUnit::Millimeter,
        LengthUnit::Centimeter,
        LengthUnit::Decimeter,
        LengthUnit::Kilometer,
        LengthUnit::Inch,
        LengthUnit::Foot,
        LengthUnit::UsSurveyFoot,
        LengthUnit::Yard,
        LengthUnit::Mile,
        LengthUnit::Micrometer,
        LengthUnit::Nanometer,
        LengthUnit::Angstrom,
        LengthUnit::Microinch,
        LengthUnit::Mil,
        LengthUnit::Decameter,
        LengthUnit::Hectometer,
        LengthUnit::Gigameter,
        LengthUnit::AstronomicalUnit,
        LengthUnit::LightYear,
        LengthUnit::Parsec,
    ];
    UNITS
        .into_iter()
        .find(|u| u.meters().is_some_and(|m| (m - meters).abs() <= 1e-9 * m))
        .unwrap_or(LengthUnit::Custom)
}

/// Unit and size in meters from a unit label (`"m"`, `"mm"`, `"ft"` ...).
pub(crate) fn unit_from_label(label: &str) -> Option<(LengthUnit, f64)> {
    let u = match label.trim().to_ascii_lowercase().as_str() {
        "m" | "meter" | "meters" | "metre" | "metres" => LengthUnit::Meter,
        "mm" | "millimeter" | "millimeters" => LengthUnit::Millimeter,
        "cm" | "centimeter" | "centimeters" => LengthUnit::Centimeter,
        "dm" => LengthUnit::Decimeter,
        "km" => LengthUnit::Kilometer,
        "in" | "inch" | "inches" | "\"" => LengthUnit::Inch,
        "ft" | "foot" | "feet" | "'" => LengthUnit::Foot,
        "yd" | "yard" | "yards" => LengthUnit::Yard,
        "mi" | "mile" | "miles" => LengthUnit::Mile,
        "um" | "µm" => LengthUnit::Micrometer,
        _ => return None,
    };
    u.meters().map(|m| (u, m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::element::ElementHeader;

    fn item(header: ElementHeader, data: ElementData) -> Item<'static> {
        Item {
            el: Element {
                header,
                data,
                linkages: Vec::new(),
                problem: None,
            },
            raw: &[],
            offset: 0,
        }
    }

    fn h(t: u16, hdr: bool, comp: bool) -> ElementHeader {
        ElementHeader {
            type_code: t,
            complex_header: hdr,
            complex_component: comp,
            ..ElementHeader::default()
        }
    }

    #[test]
    fn v8_tree_uses_counts_and_closes_on_standalone() {
        let items = vec![
            item(h(12, true, false), ElementData::Complex { children: 2 }),
            item(h(3, false, true), ElementData::Unknown),
            item(h(3, false, true), ElementData::Unknown),
            item(h(3, false, true), ElementData::Unknown), // extra component: becomes a root
            item(h(14, true, false), ElementData::Complex { children: 5 }),
            item(h(3, false, true), ElementData::Unknown),
            item(h(4, false, false), ElementData::Unknown), // closes the short header
        ];
        let t = tree_v8(&items, 64);
        assert_eq!(t.len(), 4);
        assert_eq!(t[0].children.len(), 2);
        assert_eq!(t[1].idx, 3);
        assert_eq!(t[2].children.len(), 1);
        assert_eq!(t[3].idx, 6);
        // Depth limit flattens.
        let t = tree_v8(&items, 0);
        assert_eq!(t.len(), items.len());
    }

    #[test]
    fn nested_v8_headers() {
        let items = vec![
            item(h(2, true, false), ElementData::Cell(cell(2))),
            item(h(12, true, true), ElementData::Complex { children: 1 }),
            item(h(3, false, true), ElementData::Unknown),
            item(h(3, false, true), ElementData::Unknown),
        ];
        let t = tree_v8(&items, 64);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].children.len(), 2);
        assert_eq!(t[0].children[0].children.len(), 1);
    }

    fn cell(children: u32) -> CellData {
        CellData {
            name: None,
            description: None,
            children,
            range: None,
            matrix: Rotation::None.matrix(),
            origin: [0.0; 3],
        }
    }

    #[test]
    fn v7_tree_uses_extents() {
        let mut items = vec![
            item(h(12, true, false), ElementData::Complex { children: 2 }),
            item(h(3, false, true), ElementData::Unknown),
            item(h(3, false, true), ElementData::Unknown),
            item(h(3, false, false), ElementData::Unknown),
        ];
        for (i, it) in items.iter_mut().enumerate() {
            it.offset = 100 * i as u64;
        }
        let ends = vec![Some(300), None, None, None];
        let t = tree_v7(&items, &ends, 64);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].children.len(), 2);
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn conics_map_to_circle_arc_and_ellipse() {
        let c = Point3::new(0.0, 1.0, 0.0);
        assert!(
            matches!(conic(c, 1.0, 1.0, &Rotation::Angle(0.3), 0.0, TAU), EntityKind::Circle { radius, .. } if radius == 1.0)
        );
        let EntityKind::Arc {
            start_angle,
            end_angle,
            ..
        } = conic(c, 1.0, 1.0, &Rotation::None, 0.5, -0.25)
        else {
            panic!()
        };
        assert!(close(start_angle, 0.25) && close(end_angle, 0.5));
        // Secondary axis larger than the primary: major along local Y, params shifted.
        let EntityKind::Ellipse {
            major_axis,
            ratio,
            start_param,
            end_param,
            ..
        } = conic(
            c,
            1.0,
            2.0,
            &Rotation::None,
            10f64.to_radians(),
            std::f64::consts::PI,
        )
        else {
            panic!()
        };
        assert!(close(major_axis.y, 2.0) && close(ratio, 0.5));
        let p = cadkit_core::geom_ops::ellipse_point(c, major_axis, ratio, Vec3::Z, start_param);
        assert!(
            close(p.x, 10f64.to_radians().cos())
                && close(p.y, 1.0 + 2.0 * 10f64.to_radians().sin())
        );
        assert!(close(end_param - start_param, std::f64::consts::PI));
    }

    #[test]
    fn justification_codes() {
        assert_eq!(justification(0), (HAlign::Left, VAlign::Top, 0.0, 1.0));
        assert_eq!(justification(2), (HAlign::Left, VAlign::Baseline, 0.0, 0.0));
        assert_eq!(justification(7), (HAlign::Center, VAlign::Middle, 0.5, 0.5));
        assert_eq!(justification(10), (HAlign::Right, VAlign::Middle, 1.0, 0.5));
        assert_eq!(
            justification(14),
            (HAlign::Right, VAlign::Baseline, 1.0, 0.0)
        );
        assert_eq!(
            justification(99),
            (HAlign::Left, VAlign::Baseline, 0.0, 0.0)
        );
    }

    #[test]
    fn raster_placement_follows_the_matrix() {
        // 30 degree rotation, 2 UOR per pixel, translation (100, 50); a 300 x 200 pixel image.
        // The frame stores its extent from the translation (rotated frames unverified).
        let (s, c) = 30f64.to_radians().sin_cos();
        let (a, b) = ([2.0 * c, 2.0 * s], [-2.0 * s, 2.0 * c]);
        let m = [
            a[0], b[0], 0.0, 100.0, a[1], b[1], 0.0, 50.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let corner = [300.0 * a[0] + 200.0 * b[0], 300.0 * a[1] + 200.0 * b[1]];
        let (o, u, v) = frame_placement(&m, Some(corner)).unwrap();
        assert_eq!(o, [100.0, 50.0, 0.0]);
        assert!(close(u.x, 600.0 * c) && close(u.y, 600.0 * s));
        assert!(close(v.x, -400.0 * s) && close(v.y, 400.0 * c));
        assert!(frame_placement(&m, None).is_none());
        let mut flat = m;
        flat[1] = flat[0];
        flat[5] = flat[4];
        assert!(frame_placement(&flat, Some(corner)).is_none(), "degenerate");
    }

    #[test]
    fn raster_placement_is_checked_against_the_range() {
        let rect = (
            [0.0, 0.0, 0.0],
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 5.0, 0.0),
        );
        assert!(within_range(
            &rect,
            &[[0.0, 0.0, 0.0], [10.0, 5.0, 0.0]],
            4.0
        ));
        assert!(within_range(
            &rect,
            &[[2.0, 2.0, 0.0], [12.0, 7.0, 0.0]],
            4.0
        ));
        assert!(!within_range(
            &rect,
            &[[0.0, 0.0, 0.0], [5.0, 5.0, 0.0]],
            4.0
        ));
        // Axis-aligned axes: the range itself.
        let r = [[1.0, 2.0, 0.0], [11.0, 7.0, 0.0]];
        let (o, u, v) = range_placement(
            &r,
            Some((Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0))),
        );
        assert_eq!(
            (o, u, v),
            (
                [1.0, 2.0, 0.0],
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(0.0, 5.0, 0.0)
            )
        );
        // A 30 degree rotated 4 x 2 rectangle is recovered from its bounding box.
        let (s, c) = 30f64.to_radians().sin_cos();
        let (a, b) = (Vec3::new(c, s, 0.0), Vec3::new(-s, c, 0.0));
        let corners = [
            Vec3::new(0.0, 0.0, 0.0),
            a.scaled(4.0),
            b.scaled(2.0),
            a.scaled(4.0).plus(b.scaled(2.0)),
        ];
        let lo = [
            corners.iter().map(|p| p.x).fold(f64::MAX, f64::min),
            corners.iter().map(|p| p.y).fold(f64::MAX, f64::min),
            0.0,
        ];
        let hi = [
            corners.iter().map(|p| p.x).fold(f64::MIN, f64::max),
            corners.iter().map(|p| p.y).fold(f64::MIN, f64::max),
            0.0,
        ];
        let (o, u, v) = range_placement(&[lo, hi], Some((a, b)));
        assert!(close(u.length(), 4.0) && close(v.length(), 2.0));
        assert!(close(o[0], 0.0) && close(o[1], 0.0));
        assert!(within_range(&(o, u, v), &[lo, hi], 1e-9));
        // 45 degrees with equal scale is degenerate: axis-aligned fallback.
        let d = std::f64::consts::FRAC_1_SQRT_2;
        let (_, u, _) = range_placement(
            &[lo, hi],
            Some((Vec3::new(d, d, 0.0), Vec3::new(-d, d, 0.0))),
        );
        assert_eq!(u.y, 0.0);
    }

    #[test]
    fn units_resolve() {
        assert_eq!(length_unit(1.0), LengthUnit::Meter);
        assert_eq!(length_unit(0.3048), LengthUnit::Foot);
        assert_eq!(length_unit(0.123), LengthUnit::Custom);
        assert_eq!(
            unit_from_label("mm").map(|u| u.0),
            Some(LengthUnit::Millimeter)
        );
        assert_eq!(unit_from_label("mu"), None);
        assert!(close(weight_mm(0), 0.1));
    }
}
