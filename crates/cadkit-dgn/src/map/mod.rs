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

/// [`tree_v8_counted`] without the count.
#[cfg(test)]
pub(crate) fn tree_v8(items: &[Item<'_>], max_depth: u32) -> Vec<Node> {
    tree_v8_counted(items, max_depth).0
}

/// Complex chain (12) and complex shape (14) headers.
fn is_chain_or_shape(type_code: u16) -> bool {
    matches!(type_code, 12 | 14)
}

/// Builds the V8 hierarchy from the role bits and declared component counts, and counts
/// the complex chains / shapes closed before their declared count was reached because
/// another chain or shape header followed.
///
/// A component decrements the innermost open header; a non-component closes every open
/// header (writers sometimes declare more components than they store). Headers whose
/// count is unknown accept components until the next non-component. Nesting deeper than
/// `max_depth` is flattened.
///
/// A complex chain or shape owns plain components (lines, line strings, arcs, curves);
/// GDAL dgnlib's complex-header reader collects line geometry only and notes that a child
/// "should verify complex bit set, not another header". In every chain and shape of the
/// public and private corpus the declared count equals the run of plain components that
/// follows. So a chain or shape header that arrives while a chain or shape is still open
/// is its sibling: the open one declared more components than it stores. Nesting it would
/// turn the following chains of a cell into a staircase of chains (issue #27).
pub(crate) fn tree_v8_counted(items: &[Item<'_>], max_depth: u32) -> (Vec<Node>, usize) {
    struct Open {
        node: Node,
        remaining: Option<u32>,
        chain: bool,
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
    let mut closed_early = 0usize;
    for (i, item) in items.iter().enumerate() {
        while stack.last().is_some_and(|o| o.remaining == Some(0)) {
            close(&mut stack, &mut roots);
        }
        let h = &item.el.header;
        if h.complex_header && is_chain_or_shape(h.type_code) {
            while stack.last().is_some_and(|o| o.chain) {
                close(&mut stack, &mut roots);
                closed_early += 1;
            }
        }
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
                chain: is_chain_or_shape(h.type_code),
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
    (roots, closed_early)
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

/// Lineweight of a weight step: millimeters for 0..=31, `ByBlock` for `0xffff_fffe`;
/// `None` leaves the model default (other values are kept in `dgn.weight` only).
pub(crate) fn lineweight(w: u32) -> Option<Lineweight> {
    if w <= 31 {
        Some(Lineweight::Millimeters(weight_mm(w)))
    } else if w == 0xffff_fffe {
        Some(Lineweight::ByBlock)
    } else {
        None
    }
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
    /// V8 file: property bit `0x0080` hides a graphic element (see [`Mapper::base`]).
    pub(crate) v8: bool,
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
            v8: false,
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
        self.warnings.push({
            let mut warning = Warning::new(code, message);
            warning.offset = offset;
            warning.object = object;
            warning
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
            out.push(Warning::new(code, message));
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
        if index == 0xffff_fffe {
            return Color::ByBlock;
        }
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
                {
                    let mut style = TextStyle::new(name)
                        .with_width_factor(1.0)
                        .with_props(props);
                    style.font = self.fonts.get(f).cloned();
                    style
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
                Linetype::new(style_name(*s), style_pattern(*s)).with_props(props)
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
            if let Some(w) = lineweight(h.weight) {
                e.lineweight = w;
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
            // V8 property bit 0x0080 marks an element that is not drawn: hidden tags carry
            // it (TreeText), and issue #25 reports that other DGN readers do not draw the
            // graphics carrying it. GDAL dgnlib gives the V7 bit no meaning: V7 stays visible.
            if self.v8 && h.properties & 0x0080 != 0 {
                e.visible = false;
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
        if let Some(id) = h.id
            && let Some(attrs) = self.pending_tags.remove(&id)
        {
            e.attributes = attrs
                .into_iter()
                .map(|(_, a)| relative_to_owner(a, &e))
                .collect();
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
                if let Some((start, end)) = curve_end_tangents(points) {
                    for (key, t) in [
                        ("dgn.curve_start_tangent", start),
                        ("dgn.curve_end_tangent", end),
                    ] {
                        e.props.insert(
                            key.into(),
                            Value::List([t.x, t.y, t.z].map(Value::Float).to_vec()),
                        );
                    }
                }
            }
            ElementData::Ellipse {
                primary,
                secondary,
                rotation,
                center,
            } if is_degenerate_conic(*primary, *secondary) => {
                degenerate_props(e, xf, *primary, *secondary, rotation, *center, None);
            }
            ElementData::Arc {
                start,
                sweep,
                primary,
                secondary,
                rotation,
                center,
            } if is_degenerate_conic(*primary, *secondary) => {
                degenerate_props(
                    e,
                    xf,
                    *primary,
                    *secondary,
                    rotation,
                    *center,
                    Some((*start, *sweep)),
                );
            }
            ElementData::PointString { .. } => {
                e.props.insert("dgn.point_string".into(), Value::Bool(true));
            }
            ElementData::RasterFrame { matrix, .. } => {
                // Pixel-to-UOR matrix as stored; the range only checks the placement.
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
                // `0x118` / `0x120`: horizontal and vertical resolution in dots per inch.
                let dpi = crate::le::f64_at(item.raw, 0x118);
                if let Some(x) = dpi {
                    e.props.insert("dgn.raster_dpi".into(), Value::Float(x));
                }
                if let Some(y) = crate::le::f64_at(item.raw, 0x120).filter(|y| Some(*y) != dpi) {
                    e.props.insert("dgn.raster_dpi_y".into(), Value::Float(y));
                }
                if let Some((rotation, skew)) = frame_angles(matrix) {
                    e.props
                        .insert("dgn.raster_rotation".into(), Value::Float(rotation));
                    e.props.insert("dgn.raster_skew".into(), Value::Float(skew));
                }
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
        let (matrix_placement, pixels, axes) = match &item.el.data {
            ElementData::RasterFrame { matrix, corner } => (
                frame_placement(matrix, *corner),
                frame_pixels(matrix, *corner),
                Some(frame_axes(matrix)),
            ),
            _ => (None, None, None),
        };
        // The element range is MicroStation's own bounding box. It only checks the matrix
        // placement: one that does not fit inside it is replaced by a placement derived from
        // the range, and the pixel size is then not trusted either.
        let (placement, size_px) = match (matrix_placement, range) {
            (Some(p), Some(r)) if !within_range(&p, &r, RASTER_RANGE_TOLERANCE) => {
                self.note("dgn.raster_outside_range", || {
                    "raster transform places the image outside its element range; the range was used"
                        .into()
                });
                (Some(range_placement(&r, axes)), None)
            }
            (Some(p), _) => (Some(p), pixels),
            (None, Some(r)) => {
                self.note("dgn.raster_range_placement", || {
                    "raster frame placed from its range (no usable transform)".into()
                });
                (Some(range_placement(&r, axes)), None)
            }
            (None, None) => (None, None),
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
            size_px,
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
        let mut color = None;
        let mut weight = None;
        if h.has_display_header {
            layer = Some(self.level_name(h.level));
            // Resolved like an entity's (file color table, else the default palette);
            // `relative_to_owner` drops both when they equal the owner's.
            color = Some(self.color(h.color));
            weight = Some(lineweight(h.weight).unwrap_or_default());
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
            AttributeDisplay::new(xf.dist(t.size[1]), xf.dist(t.size[0]))
                .with_style(self.font_style_name(t.font))
                .with_halign(halign)
                .with_valign(valign)
                .with_rotation(tag_rotation(&t.display_rotation(h.is_3d)))
        });
        {
            let mut attribute = Attribute::new(tag, tag_value(&t.value))
                .with_invisible(invisible)
                .with_props(props);
            attribute.set = set_name;
            attribute.position = has_pos.then(|| xf.pt([ox + dx, oy + dy, oz + dz]));
            attribute.layer = layer;
            attribute.display = display;
            attribute.color = color;
            attribute.lineweight = weight;
            attribute
        }
    }
}

fn style_name(s: u32) -> String {
    STYLE_NAMES
        .get(s as usize)
        .map_or_else(|| format!("DGN_STYLE_{s}"), |n| (*n).to_owned())
}

/// Dash-gap pattern (positive = dash, zero = dot, negative = gap) for built-in DGN
/// line styles `1..=7`; style `0` (`DGN_SOLID`) and custom styles have an empty pattern.
fn style_pattern(s: u32) -> Vec<f64> {
    match s {
        1 => vec![0.0, -1.5],
        2 => vec![3.0, -1.5],
        3 => vec![6.0, -1.5],
        4 => vec![4.5, -1.5, 0.0, -1.5],
        5 => vec![1.5, -1.5],
        6 => vec![4.5, -1.5, 0.0, -1.5, 0.0, -1.5],
        7 => vec![6.0, -1.5, 1.5, -1.5],
        _ => Vec::new(),
    }
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

/// End tangents of a type 11 curve, derived as GDAL dgnlib's `DGNStrokeCurve` (MIT/X)
/// derives the slope at its first and last fit points: with `m[k]` the unit direction of
/// the chord `P[k] -> P[k+1]` (zero for a zero-length chord), each component of the tangent
/// at fit point `k` is the Akima weighted mean
/// `(m[k-1]·|m[k+1] - m[k]| + m[k]·|m[k-1] - m[k-2]|) / (|m[k+1] - m[k]| + |m[k-1] - m[k-2]|)`,
/// or `(m[k-1] + m[k]) / 2` when both weights are zero. The start tangent is taken at
/// `P[2]` (it uses the two leading tangent points), the end tangent at `P[n-3]`.
///
/// The result is GDAL's slope of the position along the chord length: dimensionless and
/// not normalised (each component mixes the same component of two unit chord directions,
/// so it lies in [-1, 1]; the length is not 1 in general and may be 0). GDAL measures
/// chords in XY only; 3D curves here use 3D chord lengths, which equals GDAL for 2D files.
/// `None` below six points, the fewest GDAL strokes.
pub(crate) fn curve_end_tangents(points: &[UorPoint]) -> Option<(Vec3, Vec3)> {
    let n = points.len();
    if n < 6 {
        return None;
    }
    let chord = |k: usize| -> Option<Vec3> {
        let [px, py, pz] = *points.get(k)?;
        let [qx, qy, qz] = *points.get(k.checked_add(1)?)?;
        Some(
            Vec3::new(qx - px, qy - py, qz - pz)
                .normalized()
                .unwrap_or(Vec3::new(0.0, 0.0, 0.0)),
        )
    };
    let akima = |a: f64, b: f64, c: f64, d: f64| {
        // a = m[k-2], b = m[k-1], c = m[k], d = m[k+1] (one component each).
        let (w1, w2) = ((d - c).abs(), (b - a).abs());
        if w1 == 0.0 && w2 == 0.0 {
            (c + b) / 2.0
        } else {
            (b * w1 + c * w2) / (w1 + w2)
        }
    };
    let tangent = |k: usize| -> Option<Vec3> {
        let (a, b, c, d) = (
            chord(k.checked_sub(2)?)?,
            chord(k.checked_sub(1)?)?,
            chord(k)?,
            chord(k.checked_add(1)?)?,
        );
        let t = Vec3::new(
            akima(a.x, b.x, c.x, d.x),
            akima(a.y, b.y, c.y, d.y),
            akima(a.z, b.z, c.z, d.z),
        );
        t.is_finite().then(|| tidy(t))
    };
    Some((tangent(2)?, tangent(n.checked_sub(3)?)?))
}

/// Type 11 curves: the first two and last two points only define the end tangents (kept in
/// `dgn.curve_start_tangent` / `dgn.curve_end_tangent`, see [`curve_end_tangents`]).
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

/// Pixel X and Y axes of a raster frame matrix (row-major 4x4): its first two columns,
/// one pixel long each, in UOR.
pub(crate) fn frame_axes(m: &[f64; 16]) -> (Vec3, Vec3) {
    let [m00, m01, _, _, m10, m11, _, _, m20, m21, ..] = *m;
    (Vec3::new(m00, m10, m20), Vec3::new(m01, m11, m21))
}

/// Raster frame placement from its pixel-to-UOR matrix (row-major, translation in the
/// last column) and its edge lengths (`0x108`/`0x110`, UOR): the image starts at the
/// translation (its lower-left corner) and its edges run along the unit matrix X / Y
/// columns for those lengths, `u = â·ext_x`, `v = b̂·ext_y`. The lengths are measured
/// along the image axes (width·|a| and height·|b|), not as an XY extent, so rotated and
/// sheared frames need no solving. `None` when an axis or a length is degenerate.
pub(crate) fn frame_placement(
    m: &[f64; 16],
    edges: Option<[f64; 2]>,
) -> Option<(UorPoint, Vec3, Vec3)> {
    let [_, _, _, m03, _, _, _, m13, _, _, _, m23, ..] = *m;
    let (a, b) = frame_axes(m);
    let [ex, ey] = edges?;
    if !(ex.is_finite() && ey.is_finite() && ex > 0.0 && ey > 0.0) {
        return None;
    }
    let (ua, ub) = (a.normalized()?, b.normalized()?);
    // Parallel axes span no rectangle.
    if ua.cross(ub).length() <= 1e-9 {
        return None;
    }
    if ![m03, m13, m23].iter().all(|v| v.is_finite()) {
        return None;
    }
    Some(([m03, m13, m23], ua.scaled(ex), ub.scaled(ey)))
}

/// Pixel size of a raster frame: its edge lengths over the pixel lengths `|a|` and `|b|`
/// of the matrix axes, rounded. `None` when that is not a positive `u32`.
pub(crate) fn frame_pixels(m: &[f64; 16], edges: Option<[f64; 2]>) -> Option<[u32; 2]> {
    let (a, b) = frame_axes(m);
    let [ex, ey] = edges?;
    let count = |edge: f64, axis: Vec3| {
        let n = (edge / axis.length()).round();
        (n.is_finite() && n >= 1.0 && n <= f64::from(u32::MAX)).then_some(n as u32)
    };
    Some([count(ex, a)?, count(ey, b)?])
}

/// Rotation of the image X axis and skew of its Y axis from the perpendicular, both in
/// the XY plane, in radians within (-π, π].
fn frame_angles(m: &[f64; 16]) -> Option<(f64, f64)> {
    let (a, b) = frame_axes(m);
    if a.normalized().is_none() || b.normalized().is_none() {
        return None;
    }
    let rotation = a.y.atan2(a.x);
    let skew = b.y.atan2(b.x) - rotation - FRAC_PI_2;
    let skew = skew.sin().atan2(skew.cos());
    (rotation.is_finite() && skew.is_finite()).then_some((rotation, skew))
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
/// owner's level, `color` / `lineweight` stay `None` when the tag's color index / weight
/// equal the owner's, and symbology/flag props equal to the owner's are dropped.
fn relative_to_owner(mut a: Attribute, owner: &Entity) -> Attribute {
    if a.layer.is_some() && a.layer == owner.layer {
        a.layer = None;
    }
    let same = |a: &Attribute, key: &str| {
        !a.props.contains_key(key) || a.props.get(key) == owner.props.get(key)
    };
    if same(&a, "dgn.color_index") {
        a.color = None;
    }
    if same(&a, "dgn.weight") {
        a.lineweight = None;
    }
    a.props
        .retain(|key, value| owner.props.get(key) != Some(value) || key == "dgn.font_number");
    a
}

/// Rotation of a displayed tag about +Z in radians, in (-π, π], from its stored orientation
/// (2D angle or 3D quaternion, same convention as text, see [`Rotation::matrix`]). An
/// all-zero quaternion means none.
pub(crate) fn tag_rotation(rotation: &Rotation) -> f64 {
    let [m00, _, _, m10, ..] = rotation.matrix();
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
    if is_degenerate_conic(a, b) {
        return degenerate_conic(center, a, b, ux, uy, start, sweep, full);
    }
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

/// An ellipse or arc whose smaller semi-axis is at most 1e-9 of the larger one (the relative
/// tolerance [`conic`] uses for equal axes) has no area: its points lie on a segment.
pub(crate) fn is_degenerate_conic(a: f64, b: f64) -> bool {
    let (a, b) = (a.abs(), b.abs());
    a.min(b) <= 1e-9 * a.max(b)
}

/// A flat ellipse or arc as the geometry it draws: the segment covered by the swept points
/// along the non-zero axis, or a point at the center when both axes are zero (issue #24).
///
/// The point at parameter `t` is `center + a·cos t·ux + b·sin t·uy`; with `b` flat only
/// `cos t` varies (`sin t` when `a` is the flat axis). Its extremes over the swept range
/// are the values at both ends and ±1 where the range passes an axis end. The segment starts
/// at the extreme nearer the arc's start point, so a flat arc that does not pass an axis end
/// runs from its start to its end point, as a complex chain component must.
#[allow(clippy::too_many_arguments)]
fn degenerate_conic(
    center: Point3,
    a: f64,
    b: f64,
    ux: Vec3,
    uy: Vec3,
    start: f64,
    sweep: f64,
    full: bool,
) -> EntityKind {
    if a.max(b) == 0.0 {
        return EntityKind::Point { position: center };
    }
    // (axis, length, f(t), parameter of f's maximum, parameter of f's minimum)
    let (axis, len, f, t_max, t_min): (Vec3, f64, fn(f64) -> f64, f64, f64) = if a >= b {
        (ux, a, f64::cos, 0.0, std::f64::consts::PI)
    } else {
        (uy, b, f64::sin, FRAC_PI_2, 3.0 * FRAC_PI_2)
    };
    let (lo, hi) = if full {
        (-1.0, 1.0)
    } else {
        let (s, e) = if sweep >= 0.0 {
            (start, start + sweep)
        } else {
            (start + sweep, start)
        };
        // The range [s, e] contains t + 2πk for some integer k.
        let passes = |t: f64| t + TAU * ((s - t) / TAU).ceil() <= e;
        let (fs, fe) = (f(s), f(e));
        let lo = if passes(t_min) { -1.0 } else { fs.min(fe) };
        let hi = if passes(t_max) { 1.0 } else { fs.max(fe) };
        (lo, hi)
    };
    let at = |v: f64| center.translated(tidy(axis.scaled(len * v)));
    let first = f(start);
    let (p, q) = if (first - lo).abs() <= (hi - first).abs() {
        (at(lo), at(hi))
    } else {
        (at(hi), at(lo))
    };
    if p == q {
        EntityKind::Point { position: p }
    } else {
        EntityKind::Line { start: p, end: q }
    }
}

/// The element data a flat ellipse or arc mapped to a `Line` / `Point` would otherwise
/// lose: axes and center in document units, angles in radians as stored, and the stored
/// orientation (2D: `dgn.rotation` angle; 3D: `dgn.quaternion` `(w, x, y, z)`).
fn degenerate_props(
    e: &mut Entity,
    xf: &Xf,
    primary: f64,
    secondary: f64,
    rotation: &Rotation,
    center: UorPoint,
    arc: Option<(f64, f64)>,
) {
    let p = &mut e.props;
    p.insert("dgn.degenerate_ellipse".into(), Value::Bool(true));
    p.insert("dgn.primary_axis".into(), Value::Float(xf.dist(primary)));
    p.insert(
        "dgn.secondary_axis".into(),
        Value::Float(xf.dist(secondary)),
    );
    let c = xf.pt(center);
    p.insert(
        "dgn.center".into(),
        Value::List([c.x, c.y, c.z].map(Value::Float).to_vec()),
    );
    if let Some((start, sweep)) = arc {
        p.insert("dgn.start_angle".into(), Value::Float(start));
        p.insert("dgn.sweep_angle".into(), Value::Float(sweep));
    }
    match rotation {
        Rotation::Angle(a) => {
            p.insert("dgn.rotation".into(), Value::Float(*a));
        }
        Rotation::Quaternion(q) => {
            p.insert(
                "dgn.quaternion".into(),
                Value::List(q.iter().map(|v| Value::Float(*v)).collect()),
            );
        }
        Rotation::Matrix(m) => {
            p.insert(
                "dgn.matrix".into(),
                Value::List(m.iter().map(|v| Value::Float(*v)).collect()),
            );
        }
        Rotation::None => {}
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

    fn line_ends(k: EntityKind) -> (Point3, Point3) {
        match k {
            EntityKind::Line { start, end } => (start, end),
            other => panic!("not a line: {other:?}"),
        }
    }

    fn near(p: Point3, x: f64, y: f64) -> bool {
        close(p.x, x) && close(p.y, y) && close(p.z, 0.0)
    }

    #[test]
    fn degenerate_conics_become_lines_or_points() {
        let c = Point3::new(1.0, 1.0, 0.0);
        let none = Rotation::None;
        // Full flat ellipse: the whole diameter, from the extreme at the start point.
        let (s, e) = line_ends(conic(c, 2.0, 0.0, &none, 0.0, TAU));
        assert!(near(s, 3.0, 1.0) && near(e, -1.0, 1.0), "{s:?} {e:?}");
        // Secondary axis of 6e-17 (issue #24) and exactly 1e-9 of the primary.
        for b in [6e-17, 2e-9] {
            let (s, e) = line_ends(conic(c, 2.0, b, &none, 0.0, TAU));
            assert!(near(s, 3.0, 1.0) && near(e, -1.0, 1.0));
        }
        // Partial sweep inside (0, π): from the start point to the end point.
        let (s, e) = line_ends(conic(c, 2.0, 0.0, &none, 0.5, 1.0));
        assert!(near(s, 1.0 + 2.0 * 0.5f64.cos(), 1.0) && near(e, 1.0 + 2.0 * 1.5f64.cos(), 1.0));
        // The same arc travelled clockwise keeps its travel direction.
        let (s, e) = line_ends(conic(c, 2.0, 0.0, &none, 1.5, -1.0));
        assert!(near(s, 1.0 + 2.0 * 1.5f64.cos(), 1.0) && near(e, 1.0 + 2.0 * 0.5f64.cos(), 1.0));
        // A sweep passing the axis end (t = 0) reaches it.
        let (s, e) = line_ends(conic(c, 2.0, 0.0, &none, -0.5, 1.0));
        assert!(
            near(s, 1.0 + 2.0 * 0.5f64.cos(), 1.0) && near(e, 3.0, 1.0),
            "{s:?} {e:?}"
        );
        // A sweep from 2π + 1 that passes 3π and 4π spans the diameter, starting at the
        // extreme nearer its start point; one that passes 3π only stops short of +X.
        let (s, e) = line_ends(conic(c, 2.0, 0.0, &none, TAU + 1.0, 6.0));
        assert!(near(s, 3.0, 1.0) && near(e, -1.0, 1.0), "{s:?} {e:?}");
        let (s, e) = line_ends(conic(c, 2.0, 0.0, &none, TAU + 1.0, 4.0));
        assert!(
            near(s, 1.0 + 2.0 * 1f64.cos(), 1.0) && near(e, -1.0, 1.0),
            "{s:?} {e:?}"
        );
        // Flat primary axis: the points move along the rotated local Y axis (sin t).
        let rot = Rotation::Angle(FRAC_PI_2);
        let (s, e) = line_ends(conic(c, 0.0, 3.0, &rot, 0.0, std::f64::consts::PI));
        assert!(near(s, 1.0, 1.0) && near(e, -2.0, 1.0), "{s:?} {e:?}");
        // Both axes zero, or a sweep too small to move: a point.
        assert_eq!(
            conic(c, 0.0, 0.0, &none, 0.0, TAU),
            EntityKind::Point { position: c }
        );
        assert!(matches!(
            conic(c, 2.0, 0.0, &none, 0.5, 0.0),
            EntityKind::Point { .. }
        ));
        // Small but non-degenerate ratios stay ellipses.
        assert!(matches!(
            conic(c, 1.0, 1e-8, &none, 0.0, TAU),
            EntityKind::Ellipse { ratio, .. } if close(ratio, 1e-8) && ratio > 0.0
        ));
    }

    #[test]
    fn degenerate_ellipses_keep_their_element_data() {
        let mut m = Mapper::new(false, 64);
        let xf = Xf {
            origin: [0.0; 3],
            scale: 0.5,
        };
        let header = ElementHeader {
            type_code: 16,
            has_display_header: true,
            ..ElementHeader::default()
        };
        let arc = ElementData::Arc {
            start: 0.25,
            sweep: -1.0,
            primary: 8.0,
            secondary: 0.0,
            rotation: Rotation::Angle(0.5),
            center: [2.0, 4.0, 0.0],
        };
        let items = vec![item(header, arc)];
        let e = m.entity(&items, &Node::leaf(0), &xf, 0).unwrap();
        assert!(matches!(e.kind, EntityKind::Line { .. }));
        assert_eq!(
            e.props.get("dgn.degenerate_ellipse"),
            Some(&Value::Bool(true))
        );
        assert_eq!(e.props.get("dgn.primary_axis"), Some(&Value::Float(4.0)));
        assert_eq!(e.props.get("dgn.secondary_axis"), Some(&Value::Float(0.0)));
        assert_eq!(e.props.get("dgn.start_angle"), Some(&Value::Float(0.25)));
        assert_eq!(e.props.get("dgn.sweep_angle"), Some(&Value::Float(-1.0)));
        assert_eq!(e.props.get("dgn.rotation"), Some(&Value::Float(0.5)));
        assert_eq!(
            e.props.get("dgn.center"),
            Some(&Value::List(vec![
                Value::Float(1.0),
                Value::Float(2.0),
                Value::Float(0.0)
            ]))
        );
        // A regular ellipse gets none of them.
        let header = ElementHeader {
            type_code: 15,
            has_display_header: true,
            ..ElementHeader::default()
        };
        let ellipse = ElementData::Ellipse {
            primary: 8.0,
            secondary: 2.0,
            rotation: Rotation::Quaternion([1.0, 0.0, 0.0, 0.0]),
            center: [0.0; 3],
        };
        let e = m
            .entity(&[item(header, ellipse)], &Node::leaf(0), &xf, 0)
            .unwrap();
        assert!(matches!(e.kind, EntityKind::Ellipse { .. }));
        assert!(!e.props.contains_key("dgn.degenerate_ellipse"));
    }

    #[test]
    fn v8_property_bit_0x0080_hides_graphics() {
        let header = |properties: u32| ElementHeader {
            type_code: 3,
            has_display_header: true,
            properties,
            ..ElementHeader::default()
        };
        let kind = || EntityKind::Unknown {
            type_name: "test".into(),
        };
        let mut v8 = Mapper::new(false, 64);
        v8.v8 = true;
        let hidden = v8.base(&item(header(0x0880), ElementData::Unknown), kind());
        assert!(!hidden.visible);
        // The raw word stays in props; the writer derives 0x0080 from `visible` only.
        assert_eq!(
            hidden.props.get("dgn.properties"),
            Some(&Value::Int(0x0880))
        );
        assert!(
            v8.base(&item(header(0x0800), ElementData::Unknown), kind())
                .visible
        );
        // V7: GDAL dgnlib documents no meaning for the bit.
        let mut v7 = Mapper::new(false, 64);
        assert!(
            v7.base(&item(header(0x0080), ElementData::Unknown), kind())
                .visible
        );
    }

    #[test]
    fn chain_headers_close_open_chains_instead_of_nesting() {
        // A cell of three chains that each declare three components but store one (the
        // pattern of issue #27): every chain is a direct child of the cell.
        let items = vec![
            item(h(2, true, false), ElementData::Cell(cell(3))),
            item(h(12, true, true), ElementData::Complex { children: 3 }),
            item(h(3, false, true), ElementData::Unknown),
            item(h(12, true, true), ElementData::Complex { children: 3 }),
            item(h(3, false, true), ElementData::Unknown),
            item(h(14, true, true), ElementData::Complex { children: 3 }),
            item(h(3, false, true), ElementData::Unknown),
            item(h(4, false, false), ElementData::Unknown),
        ];
        let (t, closed_early) = tree_v8_counted(&items, 64);
        assert_eq!(closed_early, 2);
        assert_eq!(t.len(), 2);
        let chains: Vec<_> = t[0]
            .children
            .iter()
            .map(|c| (c.idx, c.children.len()))
            .collect();
        assert_eq!(chains, vec![(1, 1), (3, 1), (5, 1)]);
        // Correct counts are unaffected, and a B-spline curve header still nests in a chain.
        let items = vec![
            item(h(12, true, false), ElementData::Complex { children: 2 }),
            item(h(3, false, true), ElementData::Unknown),
            item(
                h(27, true, true),
                ElementData::BSplineCurve {
                    children: 1,
                    order: 4,
                    flags: 0,
                    curve_type: 0,
                    closed: false,
                    num_poles: 4,
                    num_knots: 0,
                },
            ),
            item(h(21, false, true), ElementData::Unknown),
            item(h(12, true, false), ElementData::Complex { children: 1 }),
            item(h(3, false, true), ElementData::Unknown),
        ];
        let (t, closed_early) = tree_v8_counted(&items, 64);
        assert_eq!(closed_early, 0);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].children.len(), 2);
        assert_eq!(t[0].children[1].children.len(), 1);
        assert_eq!(t[1].children.len(), 1);
    }

    #[test]
    fn curve_end_tangents_follow_gdal_stroking() {
        // Collinear points: every chord has the same direction, so both weights are zero and
        // the tangent is that direction.
        let line: Vec<UorPoint> = (0..6)
            .map(|i| [3.0 * f64::from(i), 4.0 * f64::from(i), 0.0])
            .collect();
        let (s, e) = curve_end_tangents(&line).unwrap();
        assert!(close(s.x, 0.6) && close(s.y, 0.8) && close(e.x, 0.6) && close(e.y, 0.8));
        // Turning chords: the Akima weighted mean at P[2] of m0 = (1,1)/√2, m1 = (1,0),
        // m2 = (1,1)/√2, m3 = (1,2)/√5 and, at P[3], of m1, m2, m3 and m4 = (0,1).
        let pts = [
            [-1.0, -1.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
            [3.0, 3.0, 0.0],
            [3.0, 4.0, 0.0],
        ];
        let r2 = std::f64::consts::FRAC_1_SQRT_2;
        let r5 = 1.0 / 5f64.sqrt();
        let akima = |a: f64, b: f64, c: f64, d: f64| {
            let (w1, w2) = ((d - c).abs(), (b - a).abs());
            (b * w1 + c * w2) / (w1 + w2)
        };
        let (s, e) = curve_end_tangents(&pts).unwrap();
        assert!(close(s.x, akima(r2, 1.0, r2, r5)) && close(s.y, akima(r2, 0.0, r2, 2.0 * r5)));
        assert!(close(e.x, akima(1.0, r2, r5, 0.0)) && close(e.y, akima(0.0, r2, 2.0 * r5, 1.0)));
        assert!([s.x, s.y, e.x, e.y].iter().all(|v| v.abs() <= 1.0));
        // Too few points, and a zero-length chord (direction 0, as GDAL treats it).
        assert!(curve_end_tangents(&pts[..5]).is_none());
        let mut dup = pts.to_vec();
        dup[1] = dup[0];
        let (s, _) = curve_end_tangents(&dup).unwrap();
        assert!(s.x.is_finite() && s.y.is_finite());
    }

    fn tag(quaternion: [f64; 4]) -> TagData {
        TagData {
            set_id: None,
            set_number: None,
            tag_index: 1,
            value_type: 1,
            value: TagValue::Text("v".into()),
            target: Some(7),
            origin: [0.0; 3],
            offset: [1.0, 0.0, 0.0],
            size: [1.0, 1.0],
            font: 0,
            justification: 0,
            quaternion,
        }
    }

    #[test]
    fn tag_rotation_is_an_angle_in_2d_and_a_quaternion_in_3d() {
        let mut m = Mapper::new(false, 64);
        let xf = Xf {
            origin: [0.0; 3],
            scale: 1.0,
        };
        let rotation = |m: &mut Mapper, q: [f64; 4], is_3d: bool| {
            let h = ElementHeader {
                type_code: 37,
                has_display_header: true,
                is_3d,
                ..ElementHeader::default()
            };
            m.attribute(&tag(q), &h, &xf).display.unwrap().rotation
        };
        let pi = std::f64::consts::PI;
        // 2D: the first double is the angle (issue #29 survey: π, π/2, ...; zeros unrotated).
        assert!(close(rotation(&mut m, [pi, 0.0, 0.0, 0.0], false), pi));
        assert!(close(
            rotation(&mut m, [FRAC_PI_2, 0.0, 0.0, 0.0], false),
            FRAC_PI_2
        ));
        assert!(close(
            rotation(&mut m, [-1.1602, 0.0, 0.0, 0.0], false),
            -1.1602
        ));
        assert_eq!(rotation(&mut m, [0.0; 4], false), 0.0);
        // 3D: a quaternion; (-1, 0, 0, 0) is unrotated, (cos 45°, 0, 0, -sin 45°) is +90°.
        assert_eq!(rotation(&mut m, [-1.0, 0.0, 0.0, 0.0], true), 0.0);
        let h = std::f64::consts::FRAC_PI_4;
        assert!(close(
            rotation(&mut m, [h.cos(), 0.0, 0.0, -h.sin()], true),
            FRAC_PI_2
        ));
        assert_eq!(rotation(&mut m, [pi, 0.0, 0.0, 0.0], true), 0.0);
    }

    #[test]
    fn tag_symbology_resolves_like_entities_and_is_kept_when_it_differs() {
        let mut m = Mapper::new(false, 64);
        m.palette[5] = [200, 0, 200];
        let xf = Xf {
            origin: [0.0; 3],
            scale: 1.0,
        };
        let owner_header = ElementHeader {
            type_code: 3,
            has_display_header: true,
            color: 3,
            weight: 0,
            ..ElementHeader::default()
        };
        let owner = m.base(
            &item(owner_header.clone(), ElementData::Unknown),
            EntityKind::Unknown {
                type_name: "test".into(),
            },
        );
        let own = ElementHeader {
            type_code: 37,
            color: 5,
            weight: 2,
            ..owner_header.clone()
        };
        let a = relative_to_owner(m.attribute(&tag([0.0; 4]), &own, &xf), &owner);
        assert_eq!(
            a.color,
            Some(Color::Rgb {
                r: 200,
                g: 0,
                b: 200
            })
        );
        assert_eq!(a.lineweight, Some(Lineweight::Millimeters(weight_mm(2))));
        assert_eq!(a.props.get("dgn.color_index"), Some(&Value::Int(5)));
        assert_eq!(a.props.get("dgn.weight"), Some(&Value::Int(2)));
        // Same index and weight as the owner: nothing of its own.
        let same = ElementHeader {
            type_code: 37,
            ..owner_header
        };
        let a = relative_to_owner(m.attribute(&tag([0.0; 4]), &same, &xf), &owner);
        assert_eq!((a.color, a.lineweight), (None, None));
        assert!(!a.props.contains_key("dgn.color_index"));
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
        // 30 degree rotation, 2 UOR per pixel, translation (100, 50); a 300 x 200 pixel
        // image. `0x108`/`0x110` hold the edge lengths along the image axes (W·|a|, H·|b|).
        let (s, c) = 30f64.to_radians().sin_cos();
        let (a, b) = ([2.0 * c, 2.0 * s], [-2.0 * s, 2.0 * c]);
        let m = [
            a[0], b[0], 0.0, 100.0, a[1], b[1], 0.0, 50.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let edges = [600.0, 400.0];
        let (o, u, v) = frame_placement(&m, Some(edges)).unwrap();
        assert_eq!(o, [100.0, 50.0, 0.0]);
        assert!(close(u.x, 600.0 * c) && close(u.y, 600.0 * s));
        assert!(close(v.x, -400.0 * s) && close(v.y, 400.0 * c));
        assert_eq!(frame_pixels(&m, Some(edges)), Some([300, 200]));
        let (rotation, skew) = frame_angles(&m).unwrap();
        assert!(close(rotation, 30f64.to_radians()) && close(skew, 0.0));
        assert!(frame_placement(&m, None).is_none());
        assert!(frame_placement(&m, Some([0.0, 400.0])).is_none());
        let mut flat = m;
        flat[1] = flat[0];
        flat[5] = flat[4];
        assert!(frame_placement(&flat, Some(edges)).is_none(), "degenerate");
    }

    #[test]
    fn sheared_raster_frames_keep_their_axes() {
        // X axis 3 UOR per pixel along +X; Y axis 2 UOR per pixel tilted 10 degrees left of
        // +Y. The Y edge runs along the tilted axis for H·|b| UOR.
        let (s, c) = 10f64.to_radians().sin_cos();
        let (a, b) = ([3.0, 0.0], [-2.0 * s, 2.0 * c]);
        let m = [
            a[0], b[0], 0.0, -5.0, a[1], b[1], 0.0, 7.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let edges = [3.0 * 120.0, 2.0 * 80.0];
        let (o, u, v) = frame_placement(&m, Some(edges)).unwrap();
        assert_eq!(o, [-5.0, 7.0, 0.0]);
        assert!(close(u.x, 360.0) && close(u.y, 0.0));
        assert!(close(v.x, -160.0 * s) && close(v.y, 160.0 * c));
        assert_eq!(frame_pixels(&m, Some(edges)), Some([120, 80]));
        let (rotation, skew) = frame_angles(&m).unwrap();
        assert!(close(rotation, 0.0) && close(skew, 10f64.to_radians()));
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

    #[test]
    fn bycell_symbology_and_builtin_style_patterns() {
        let mut m = Mapper::new(false, 64);
        assert_eq!(m.color(0xffff_fffe), Color::ByBlock);
        assert_eq!(m.color(0xffff_ffff), Color::ByLayer);
        let mut hdr = ElementHeader {
            type_code: 3,
            has_display_header: true,
            color: 0xffff_fffe,
            weight: 0xffff_fffe,
            style: 2,
            ..ElementHeader::default()
        };
        let e = m.base(
            &item(hdr.clone(), ElementData::Unknown),
            EntityKind::Unknown {
                type_name: "test".into(),
            },
        );
        assert_eq!(e.color, Color::ByBlock);
        assert_eq!(e.lineweight, Lineweight::ByBlock);
        hdr.style = 0;
        let _ = m.base(
            &item(hdr, ElementData::Unknown),
            EntityKind::Unknown {
                type_name: "test".into(),
            },
        );
        let lts = m.linetypes();
        assert_eq!(lts.len(), 2);
        assert!(
            lts.iter()
                .any(|l| l.name == "DGN_SOLID" && l.pattern.is_empty())
        );
        assert!(
            lts.iter()
                .any(|l| l.name == "DGN_MEDIUM_DASH" && l.pattern == vec![3.0, -1.5])
        );
    }
}
