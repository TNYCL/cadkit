//! Native objects → [`cadkit_core::Document`].
//!
//! - Tables come from the control objects named in the header (falling back to all
//!   records of that type).
//! - Block definitions come from BLOCK_CONTROL; *Model_Space becomes the model and
//!   every *Paper_Space* block a layout [`Model`] named after its LAYOUT object.
//! - Entity membership: R2004+ block headers list their entities; R13–R2000 group
//!   entities by owner handle / entity mode, in first..last chain order when the
//!   chain covers exactly those entities (handle order otherwise).
//! - All OCS coordinates are converted to WCS with the arbitrary-axis algorithm;
//!   planar entities keep their normal (see the model docs).

mod entity;

use std::collections::{BTreeMap, HashMap, HashSet};

use cadkit_core::geom_ops::arbitrary_axes;
use cadkit_core::{
    Block, Color, Document, Entity, EntityKind, Error, Format, HatchEdge, Layer, LengthUnit,
    Limits, Linetype, Lineweight, Model, ModelKind, Point3, Props, Raw, ReadOptions, Result,
    SourceInfo, TextStyle, Units, Value, Vec3, Warning,
};

use crate::native::{
    BlockHeader, CmColor, DwgFile, DwgObject, EedItem, EntityCommon, ObjectData, lineweight_mm,
};

/// Names of table records by handle, for reference resolution.
struct Names {
    layers: HashMap<u64, String>,
    linetypes: HashMap<u64, String>,
    styles: HashMap<u64, String>,
    blocks: HashMap<u64, String>,
    appids: HashMap<u64, String>,
    dimstyles: HashMap<u64, String>,
    mlinestyles: HashMap<u64, String>,
    imagedefs: HashMap<u64, String>,
}

/// Membership key of an entity: its owner handle, or the space its entity mode names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OwnerKey {
    Owner(u64),
    Space(u8),
}

struct Mapper<'f> {
    file: &'f DwgFile,
    names: Names,
    /// Pre-R2004 block membership: owner key → entity handles in handle order.
    members: HashMap<OwnerKey, Vec<u64>>,
    /// ATTRIB / VERTEX records already attached to a parent (each belongs to one).
    claimed_children: HashSet<u64>,
    /// Entities already placed in a block or layout: each is mapped at most once, so
    /// repeated references cannot multiply the output.
    placed: HashSet<u64>,
    /// Document-wide totals checked against the limits.
    mapped_entities: u64,
    mapped_items: u64,
    limits: Limits,
    /// The first limit exceeded while mapping; reading then fails with it.
    limit_error: Option<Error>,
    keep_raw: bool,
    warnings: Vec<Warning>,
    unsupported: BTreeMap<String, usize>,
}

/// Vertex-like items of a mapped entity (vertices, points, knots, weights, faces,
/// hatch edges, attributes), counted against `Limits::max_total_vertices` document-wide.
fn item_count(e: &Entity) -> u64 {
    let edge_items = |edge: &HatchEdge| match edge {
        HatchEdge::Spline {
            knots,
            control_points,
            weights,
            ..
        } => knots.len() + control_points.len() + weights.len(),
        HatchEdge::Polyline { vertices, .. } => vertices.len(),
        _ => 1,
    };
    let n = match &e.kind {
        EntityKind::Polyline { vertices, .. } => vertices.len(),
        EntityKind::Spline {
            knots,
            control_points,
            weights,
            fit_points,
            ..
        } => knots.len() + control_points.len() + weights.len() + fit_points.len(),
        EntityKind::Hatch { loops, .. } => loops
            .iter()
            .map(|l| 1 + l.edges.iter().map(edge_items).sum::<usize>())
            .sum(),
        EntityKind::Dimension { points, .. } | EntityKind::Face { points, .. } => points.len(),
        EntityKind::Leader { vertices, .. } => vertices.len(),
        EntityKind::Mesh { vertices, faces } => {
            vertices.len() + faces.iter().map(Vec::len).sum::<usize>()
        }
        _ => 1,
    };
    (n + e.attributes.len()) as u64
}

/// True when `obj` is owned by the block record `block`: by its owner handle, or by
/// entity mode 1/2 (paper/model space) for the space `space_mode` names.
fn belongs_to(obj: &DwgObject, block: u64, space_mode: Option<u8>) -> bool {
    match obj.common.entity.as_ref().map(|e| e.entity_mode) {
        None | Some(0) => obj.common.owner == Some(block),
        mode => mode == space_mode,
    }
}

fn p3(a: [f64; 3]) -> Point3 {
    Point3::new(a[0], a[1], a[2])
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn is_unit_z(n: [f64; 3]) -> bool {
    n[0] == 0.0 && n[1] == 0.0 && n[2] == 1.0
}

fn list3(a: [f64; 3]) -> Value {
    Value::List(a.iter().map(|&x| Value::Float(x)).collect())
}

/// OCS point → WCS.
fn ocs_to_wcs(normal: [f64; 3], p: [f64; 3]) -> Point3 {
    if is_unit_z(normal) {
        return p3(p);
    }
    let (ax, ay, az) = arbitrary_axes(v3(normal));
    Point3::new(
        ax.x * p[0] + ay.x * p[1] + az.x * p[2],
        ax.y * p[0] + ay.y * p[1] + az.y * p[2],
        ax.z * p[0] + ay.z * p[1] + az.z * p[2],
    )
}

/// Converts a stored color (CMC/ENC) to the model.
pub(crate) fn to_color(c: &CmColor) -> Color {
    if let Some(v) = c.value {
        match v >> 24 {
            0xC0 => return Color::ByLayer,
            0xC1 => return Color::ByBlock,
            0xC3 => return aci(i32::try_from(v & 0xFF).unwrap_or(0)),
            0xC8 => return Color::ByLayer,
            0xC2 => {
                return Color::Rgb {
                    r: (v >> 16) as u8,
                    g: (v >> 8) as u8,
                    b: v as u8,
                };
            }
            _ if c.index == 0 && v != 0 => {
                return Color::Rgb {
                    r: (v >> 16) as u8,
                    g: (v >> 8) as u8,
                    b: v as u8,
                };
            }
            _ => {}
        }
    }
    aci(i32::from(c.index))
}

fn aci(index: i32) -> Color {
    match index.unsigned_abs() {
        0 => Color::ByBlock,
        256 => Color::ByLayer,
        i @ 1..=255 => Color::Aci { index: i as u8 },
        _ => Color::ByLayer,
    }
}

fn lineweight(raw: Option<u8>) -> Lineweight {
    match raw {
        None | Some(28 | 29) => Lineweight::ByLayer,
        Some(30) => Lineweight::ByBlock,
        Some(31) => Lineweight::Default,
        Some(r) => lineweight_mm(r).map_or(Lineweight::Default, Lineweight::Millimeters),
    }
}

fn eed_value(item: &EedItem) -> Value {
    match item {
        EedItem::String(s) => Value::Text(s.clone()),
        EedItem::Control(close) => Value::Text(if *close { "}" } else { "{" }.into()),
        EedItem::Layer(h) | EedItem::Handle(h) => Value::Int(*h as i64),
        EedItem::Binary(b) => Value::Bytes(b.clone()),
        EedItem::Point(_, p) => list3(*p),
        EedItem::Real(_, x) => Value::Float(*x),
        EedItem::Short(x) => Value::Int(i64::from(*x)),
        EedItem::Long(x) => Value::Int(i64::from(*x)),
    }
}

impl<'f> Mapper<'f> {
    fn obj(&self, handle: Option<u64>) -> Option<&'f DwgObject> {
        self.file.objects.get(&handle?)
    }

    fn warn(&mut self, code: &str, message: String, object: Option<u64>) {
        self.warnings.push(Warning {
            code: code.into(),
            message,
            offset: None,
            object,
        });
    }

    /// Entry handles of a table control object, or every record of `type_code`.
    fn table_entries(&self, control_var: &str, type_code: u16) -> Vec<u64> {
        if let Some(ObjectData::Control(c)) = self
            .obj(self.file.header.handle(control_var))
            .map(|o| &o.data)
        {
            let mut list: Vec<u64> = c
                .entries
                .iter()
                .chain(c.extra.iter())
                .copied()
                .filter(|&h| h != 0)
                .collect();
            let mut seen = HashSet::new();
            list.retain(|h| seen.insert(*h));
            return list;
        }
        self.file
            .objects
            .values()
            .filter(|o| o.type_code == type_code)
            .map(|o| o.handle)
            .collect()
    }

    /// Name of a block record. Anonymous records store only `*U`/`*D`/`*T`; their BLOCK
    /// entity carries the full name (`*U18`), whose number is the record's index in
    /// the BLOCK_CONTROL entry list. Pre-R2010 files save dynamic blocks as anonymous
    /// records whose BLOCK entity keeps the dynamic block's own name; DXF exports name
    /// those `*U<index>` too, and so does this reader (the stored name goes to props).
    fn block_name(file: &DwgFile, handle: u64, bh: &BlockHeader, index: Option<usize>) -> String {
        let entity_name = match bh
            .block_entity
            .and_then(|h| file.objects.get(&h))
            .map(|o| &o.data)
        {
            Some(ObjectData::Block { name }) if !name.is_empty() => Some(name.as_str()),
            _ => None,
        };
        let header_name = bh.entry.name.as_str();
        match (entity_name, index) {
            (Some(n), Some(i))
                if bh.anonymous && header_name.starts_with('*') && !n.starts_with('*') =>
            {
                format!("{header_name}{i}")
            }
            (Some(n), _) => n.to_owned(),
            (None, Some(i))
                if bh.anonymous && header_name.len() <= 2 && header_name.starts_with('*') =>
            {
                format!("{header_name}{i}")
            }
            (None, _) if !header_name.is_empty() => header_name.to_owned(),
            _ => format!("*BLOCK_{handle:X}"),
        }
    }

    fn collect_names(file: &DwgFile) -> Names {
        // Raw BLOCK_CONTROL entry positions (null slots included) for anonymous names.
        let block_index: HashMap<u64, usize> = file
            .header
            .handle("BLOCK_CONTROL_OBJECT")
            .and_then(|h| file.objects.get(&h))
            .and_then(|o| match &o.data {
                ObjectData::Control(c) => {
                    Some(c.entries.iter().enumerate().map(|(i, &h)| (h, i)).collect())
                }
                _ => None,
            })
            .unwrap_or_default();
        let mut n = Names {
            layers: HashMap::new(),
            linetypes: HashMap::new(),
            styles: HashMap::new(),
            blocks: HashMap::new(),
            appids: HashMap::new(),
            dimstyles: HashMap::new(),
            mlinestyles: HashMap::new(),
            imagedefs: HashMap::new(),
        };
        for (h, o) in &file.objects {
            match &o.data {
                ObjectData::Layer(l) => {
                    n.layers.insert(*h, l.entry.name.clone());
                }
                ObjectData::Linetype(l) => {
                    n.linetypes.insert(*h, l.entry.name.clone());
                }
                ObjectData::TextStyle(s) => {
                    n.styles.insert(*h, s.entry.name.clone());
                }
                ObjectData::BlockHeader(b) => {
                    n.blocks.insert(
                        *h,
                        Self::block_name(file, *h, b, block_index.get(h).copied()),
                    );
                }
                ObjectData::TableRecord(t) if o.type_code == 0x43 => {
                    n.appids.insert(*h, t.name.clone());
                }
                ObjectData::TableRecord(t) if o.type_code == 0x45 => {
                    n.dimstyles.insert(*h, t.name.clone());
                }
                ObjectData::ImageDef(d) => {
                    n.imagedefs.insert(*h, d.file_name.clone());
                }
                _ => {}
            }
        }
        n
    }

    fn common_props(&self, obj: &DwgObject, e: &EntityCommon, props: &mut Props) {
        if e.linetype_scale != 1.0 {
            props.insert("dwg.linetype_scale".into(), Value::Float(e.linetype_scale));
        }
        if let Some(t) = e.color.transparency {
            props.insert("dwg.transparency".into(), Value::Int(i64::from(t)));
        }
        if let Some(h) = e.color_book {
            props.insert("dwg.color_book".into(), Value::Int(h as i64));
        }
        if let Some(name) = &e.color.name {
            props.insert("dwg.color_name".into(), Value::Text(name.clone()));
        }
        if let Some(h) = e.plotstyle {
            props.insert("dwg.plotstyle".into(), Value::Int(h as i64));
        }
        if let Some(h) = e.material {
            props.insert("dwg.material".into(), Value::Int(h as i64));
        }
        for block in &obj.common.eed {
            let app = self
                .names
                .appids
                .get(&block.app)
                .cloned()
                .unwrap_or_else(|| format!("{:X}", block.app));
            let value = if block.items.is_empty() {
                Value::Bytes(block.data.clone())
            } else {
                Value::List(block.items.iter().map(eed_value).collect())
            };
            props.insert(format!("dwg.eed.{app}"), value);
        }
    }

    fn base_entity(&self, obj: &DwgObject, kind: EntityKind) -> Entity {
        let mut entity = Entity::new(kind);
        entity.id = Some(obj.handle);
        if let Some(e) = &obj.common.entity {
            entity.layer = e.layer.and_then(|h| self.names.layers.get(&h)).cloned();
            entity.color = to_color(&e.color);
            // Color-book colors live in the referenced DBCOLOR object.
            if let Some(ObjectData::DbColor(book)) = e
                .color_book
                .and_then(|h| self.file.objects.get(&h))
                .map(|o| &o.data)
            {
                entity.color = to_color(book);
                if let Some(name) = &book.name {
                    entity
                        .props
                        .insert("dwg.color_name".into(), Value::Text(name.clone()));
                }
                if let Some(book_name) = &book.book {
                    entity
                        .props
                        .insert("dwg.color_book_name".into(), Value::Text(book_name.clone()));
                }
            }
            entity.linetype = match e.linetype_flags {
                1 => Some("ByBlock".to_owned()),
                2 => Some("Continuous".to_owned()),
                3 => e
                    .linetype
                    .and_then(|h| self.names.linetypes.get(&h))
                    .cloned()
                    .filter(|n| !n.eq_ignore_ascii_case("ByLayer")),
                _ => None,
            };
            entity.lineweight = lineweight(e.lineweight);
            entity.visible = !e.invisible;
            self.common_props(obj, e, &mut entity.props);
        }
        if self.keep_raw {
            entity.raw = Some(Raw {
                type_code: u32::from(obj.type_code),
                bytes: obj.raw.clone(),
            });
        }
        entity
    }

    /// Child records of an INSERT (ATTRIB) or POLYLINE (VERTEX_*): the owned list
    /// (R2004+) or the first..last chain (R13–R2000). The chain stops at the first
    /// object whose type is not in `types`, and a child is never claimed twice, so
    /// corrupt links cannot cause quadratic work.
    fn child_handles(&mut self, list: &[u64], is_range: bool, types: &[u16]) -> Vec<u64> {
        if !is_range {
            return list
                .iter()
                .copied()
                .filter(|h| self.claimed_children.insert(*h))
                .collect();
        }
        let (Some(&first), Some(&last)) = (list.first(), list.get(1)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut cur = first;
        while cur != 0 {
            let Some(o) = self.file.objects.get(&cur) else {
                break;
            };
            if !types.contains(&o.type_code) || !self.claimed_children.insert(cur) {
                break;
            }
            out.push(cur);
            if cur == last {
                break;
            }
            let Some(e) = &o.common.entity else { break };
            cur = match (e.no_links, e.next) {
                (false, Some(n)) => n,
                _ => cur.saturating_add(1),
            };
        }
        out
    }

    /// Groups pre-R2004 entities by owner (entity mode 0) or space (modes 1 and 2).
    /// Sub-entities (ATTRIB, VERTEX) and block delimiters are excluded.
    fn build_members(file: &DwgFile) -> HashMap<OwnerKey, Vec<u64>> {
        let mut members: HashMap<OwnerKey, Vec<u64>> = HashMap::new();
        if file.version.r2004_plus() {
            return members;
        }
        for o in file.objects.values() {
            let Some(e) = &o.common.entity else { continue };
            let structural = matches!(
                o.data,
                ObjectData::Block { .. } | ObjectData::EndBlock | ObjectData::Seqend
            ) || matches!(o.type_code, 0x02 | 0x0A..=0x0E);
            if structural {
                continue;
            }
            let key = match e.entity_mode {
                0 => match o.common.owner {
                    Some(owner) => OwnerKey::Owner(owner),
                    None => continue,
                },
                m => OwnerKey::Space(m),
            };
            members.entry(key).or_default().push(o.handle);
        }
        members
    }

    /// Entities owned by a block header, in drawing order. R2004+ headers list them;
    /// before, membership comes from owner handles / entity mode and the order from
    /// the first..last entity chain when that chain covers exactly the members.
    fn block_entities(&self, handle: u64, bh: &BlockHeader, space_mode: Option<u8>) -> Vec<u64> {
        if self.file.version.r2004_plus() {
            return bh.entities.clone();
        }
        let mut members: Vec<u64> = self
            .members
            .get(&OwnerKey::Owner(handle))
            .cloned()
            .unwrap_or_default();
        if let Some(m) = space_mode {
            members.extend(
                self.members
                    .get(&OwnerKey::Space(m))
                    .into_iter()
                    .flatten()
                    .copied(),
            );
            members.sort_unstable();
            members.dedup();
        }
        let member_set: HashSet<u64> = members.iter().copied().collect();
        if let (Some(first), Some(last)) = (bh.first_entity, bh.last_entity) {
            let mut chain = Vec::new();
            let mut seen = HashSet::new();
            let mut cur = first;
            let mut ok = false;
            // Sub-entities sit between members in the chain; allow a generous slack.
            let max_steps = members.len().saturating_mul(4).saturating_add(64);
            for _ in 0..max_steps {
                if !seen.insert(cur) {
                    break;
                }
                let Some(o) = self.file.objects.get(&cur) else {
                    break;
                };
                if member_set.contains(&cur) {
                    chain.push(cur);
                }
                if cur == last {
                    ok = true;
                    break;
                }
                let Some(e) = &o.common.entity else { break };
                cur = match (e.no_links, e.next) {
                    (false, Some(n)) => n,
                    _ => cur.saturating_add(1),
                };
            }
            if ok && chain.len() == members.len() {
                return chain;
            }
        }
        members
    }

    /// Maps the entities a block record lists. Each entity is placed once, in the
    /// block that owns it: references from other blocks and repeats are ignored with a
    /// warning, and the document-wide totals are checked against the limits.
    fn entities(
        &mut self,
        block: u64,
        name: &str,
        space_mode: Option<u8>,
        handles: &[u64],
    ) -> Vec<Entity> {
        let mut out = Vec::new();
        let (mut repeated, mut foreign) = (0usize, 0usize);
        for &h in handles {
            if self.limit_error.is_some() {
                break;
            }
            let Some(obj) = self.file.objects.get(&h) else {
                self.warn(
                    "dwg.missing_object",
                    format!("entity {h:X} is not in the object map"),
                    Some(h),
                );
                continue;
            };
            if !obj.is_entity {
                continue;
            }
            if !belongs_to(obj, block, space_mode) {
                foreign += 1;
                continue;
            }
            if !self.placed.insert(h) {
                repeated += 1;
                continue;
            }
            if let Some(e) = self.map_entity(obj) {
                self.mapped_entities += 1;
                self.mapped_items = self.mapped_items.saturating_add(item_count(&e));
                if self.mapped_entities > self.limits.max_objects {
                    self.limit_error = Some(Error::LimitExceeded(format!(
                        "more than {} entities",
                        self.limits.max_objects
                    )));
                } else if self.mapped_items > self.limits.max_total_vertices {
                    self.limit_error = Some(Error::LimitExceeded(format!(
                        "more than {} vertices in the document",
                        self.limits.max_total_vertices
                    )));
                }
                out.push(e);
            }
        }
        if repeated > 0 {
            self.warn(
                "dwg.duplicate_entity",
                format!("block {name}: {repeated} repeated entity references ignored"),
                Some(block),
            );
        }
        if foreign > 0 {
            self.warn(
                "dwg.owner_mismatch",
                format!("block {name}: {foreign} listed entities are owned elsewhere; ignored"),
                Some(block),
            );
        }
        out
    }
}

fn header_props(file: &DwgFile, props: &mut Props) {
    for (name, value) in &file.header.vars {
        props.insert(format!("dwg.header.{name}"), value.clone());
    }
    props.insert(
        "dwg.maintenance_version".into(),
        Value::Int(i64::from(file.maintenance_version)),
    );
    props.insert("dwg.codepage".into(), Value::Int(i64::from(file.codepage)));
    if let Some(name) = crate::codepage::name(file.codepage) {
        props.insert("dwg.codepage_name".into(), Value::Text(name.into()));
    }
    if let Some(m) = file.measurement {
        props.insert("dwg.measurement".into(), Value::Int(i64::from(m)));
    }
    if !file.section_names.is_empty() {
        props.insert(
            "dwg.sections".into(),
            Value::List(
                file.section_names
                    .iter()
                    .map(|s| Value::Text(s.clone()))
                    .collect(),
            ),
        );
    }
    props.insert(
        "dwg.classes".into(),
        Value::List(
            file.classes
                .iter()
                .map(|c| Value::Text(format!("{}={}", c.number, c.dxf_name)))
                .collect(),
        ),
    );
}

/// Builds the neutral document. Fails only when a document-wide limit is exceeded
/// (more than `max_objects` entities or `max_total_vertices` vertex-like items).
pub fn to_document(file: &DwgFile, options: &ReadOptions) -> Result<Document> {
    let mut m = Mapper {
        file,
        names: Mapper::collect_names(file),
        members: Mapper::build_members(file),
        claimed_children: HashSet::new(),
        placed: HashSet::new(),
        mapped_entities: 0,
        mapped_items: 0,
        limits: options.limits,
        limit_error: None,
        keep_raw: options.keep_raw,
        warnings: Vec::new(),
        unsupported: BTreeMap::new(),
    };
    let mut doc = Document {
        source: SourceInfo {
            format: Format::Dwg,
            version: file.version.magic().to_owned(),
            application: None,
            codepage: Some(file.encoding.to_owned()),
        },
        ..Document::default()
    };
    let unit = file
        .header
        .int("INSUNITS")
        .map_or(LengthUnit::Unitless, |u| {
            LengthUnit::from_insunits(u as i32)
        });
    doc.units = Units {
        unit,
        meters_per_unit: unit.meters(),
    };
    header_props(file, &mut doc.props);

    // Layers.
    for h in m.table_entries("LAYER_CONTROL_OBJECT", 0x33) {
        let Some(ObjectData::Layer(l)) = m.obj(Some(h)).map(|o| &o.data) else {
            continue;
        };
        let mut props = Props::new();
        if let Some(name) = &l.color.name {
            props.insert("dwg.color_name".into(), Value::Text(name.clone()));
        }
        if let Some(book) = &l.color.book {
            props.insert("dwg.color_book".into(), Value::Text(book.clone()));
        }
        if l.frozen_new_viewports {
            props.insert("dwg.frozen_new_viewports".into(), Value::Bool(true));
        }
        if l.entry.xref_dependent {
            props.insert("dwg.xref_dependent".into(), Value::Bool(true));
        }
        // Layer descriptions are stored as the second string of the
        // "AcAecLayerStandard" EED of the layer.
        let layer_obj = m.obj(Some(h));
        let description = layer_obj
            .into_iter()
            .flat_map(|o| o.common.eed.iter())
            .find(|b| {
                m.names
                    .appids
                    .get(&b.app)
                    .is_some_and(|n| n.eq_ignore_ascii_case("AcAecLayerStandard"))
            })
            .and_then(|b| match b.items.get(1) {
                Some(EedItem::String(d)) if !d.is_empty() => Some(d.clone()),
                _ => None,
            });
        let color = match to_color(&l.color) {
            Color::ByLayer | Color::ByBlock => Color::Aci { index: 7 },
            c => c,
        };
        doc.layers.push(Layer {
            name: l.entry.name.clone(),
            id: Some(h),
            color,
            linetype: l.linetype.and_then(|t| m.names.linetypes.get(&t)).cloned(),
            lineweight: lineweight(l.lineweight),
            visible: l.on && l.color.index >= 0,
            frozen: l.frozen,
            locked: l.locked,
            plottable: l.plot,
            description,
            props,
        });
    }

    // Linetypes.
    for h in m.table_entries("LINETYPE_CONTROL_OBJECT", 0x39) {
        let Some(ObjectData::Linetype(l)) = m.obj(Some(h)).map(|o| &o.data) else {
            continue;
        };
        let mut props = Props::new();
        props.insert("dwg.pattern_length".into(), Value::Float(l.pattern_length));
        if l.dashes.iter().any(|d| d.shape_flags != 0) {
            props.insert("dwg.complex".into(), Value::Bool(true));
        }
        doc.linetypes.push(Linetype {
            name: l.entry.name.clone(),
            description: Some(l.description.clone()).filter(|d| !d.is_empty()),
            pattern: l.dashes.iter().map(|d| d.length).collect(),
            props,
        });
    }

    // Text styles.
    for h in m.table_entries("STYLE_CONTROL_OBJECT", 0x35) {
        let Some(ObjectData::TextStyle(s)) = m.obj(Some(h)).map(|o| &o.data) else {
            continue;
        };
        let mut props = Props::new();
        if !s.big_font.is_empty() {
            props.insert("dwg.big_font".into(), Value::Text(s.big_font.clone()));
        }
        if s.is_shape {
            props.insert("dwg.shape_file".into(), Value::Bool(true));
        }
        if s.vertical {
            props.insert("dwg.vertical".into(), Value::Bool(true));
        }
        if s.generation != 0 {
            props.insert("dwg.generation".into(), Value::Int(i64::from(s.generation)));
        }
        doc.text_styles.push(TextStyle {
            name: s.entry.name.clone(),
            font: Some(s.font.clone()).filter(|f| !f.is_empty()),
            height: s.fixed_height,
            width_factor: s.width_factor,
            oblique: s.oblique,
            props,
        });
    }

    // Blocks and spaces.
    let model_space = file.header.handle("BLOCK_RECORD_MODEL_SPACE");
    let paper_space = file.header.handle("BLOCK_RECORD_PAPER_SPACE");
    let mut layouts: Vec<(i32, Model)> = Vec::new();
    for h in m.table_entries("BLOCK_CONTROL_OBJECT", 0x31) {
        let Some(obj) = m.obj(Some(h)) else { continue };
        let ObjectData::BlockHeader(bh) = &obj.data else {
            continue;
        };
        let name = m.names.blocks.get(&h).cloned().unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        let is_model = model_space == Some(h) || lower == "*model_space";
        let is_paper = !is_model && (paper_space == Some(h) || lower.starts_with("*paper_space"));
        let space_mode = if is_model {
            Some(2)
        } else if paper_space == Some(h) || (paper_space.is_none() && lower == "*paper_space") {
            Some(1)
        } else {
            None
        };
        let handles = m.block_entities(h, bh, space_mode);
        let entities = m.entities(h, &name, space_mode, &handles);
        if is_model || is_paper {
            let layout = bh
                .layout
                .and_then(|l| file.objects.get(&l))
                .and_then(|o| match &o.data {
                    ObjectData::Layout(l) => Some(l),
                    _ => None,
                });
            let mut props = Props::new();
            props.insert("dwg.block".into(), Value::Text(name.clone()));
            let model = Model {
                name: layout.map_or_else(
                    || {
                        if is_model {
                            "Model".to_owned()
                        } else {
                            name.clone()
                        }
                    },
                    |l| l.name.clone(),
                ),
                id: Some(h),
                kind: if is_model {
                    ModelKind::Model
                } else {
                    ModelKind::Layout
                },
                is_3d: false,
                entities,
                props,
            };
            let order = if is_model {
                i32::MIN
            } else {
                layout.map_or(i32::MAX, |l| l.tab_order)
            };
            layouts.push((order, model));
        } else {
            let mut props = Props::new();
            if bh.anonymous {
                props.insert("dwg.anonymous".into(), Value::Bool(true));
            }
            if let Some(ObjectData::Block { name: stored }) = bh
                .block_entity
                .and_then(|b| file.objects.get(&b))
                .map(|o| &o.data)
            {
                if *stored != name {
                    props.insert("dwg.block_entity_name".into(), Value::Text(stored.clone()));
                }
            }
            if bh.has_attdefs {
                props.insert("dwg.has_attdefs".into(), Value::Bool(true));
            }
            if bh.entry.xref_dependent {
                props.insert("dwg.xref_dependent".into(), Value::Bool(true));
            }
            doc.blocks.push(Block {
                name,
                id: Some(h),
                base_point: p3(bh.base_point),
                entities,
                is_xref: bh.is_xref || bh.is_overlaid,
                xref_path: Some(bh.xref_path.clone()).filter(|p| !p.is_empty()),
                description: Some(bh.description.clone()).filter(|d| !d.is_empty()),
                props,
            });
        }
    }
    if let Some(e) = m.limit_error.take() {
        return Err(e);
    }
    layouts.sort_by_key(|(order, _)| *order);
    doc.models = layouts.into_iter().map(|(_, model)| model).collect();
    if doc.models.is_empty() {
        m.warn(
            "dwg.no_model_space",
            "no model space block found".into(),
            None,
        );
    }

    for (type_name, count) in std::mem::take(&mut m.unsupported) {
        m.warn(
            "dwg.unsupported_entity",
            format!("{count} {type_name} entities mapped to Unknown"),
            None,
        );
    }
    doc.warnings = file.warnings.clone();
    doc.warnings.append(&mut m.warnings);
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::TableEntry;

    #[test]
    fn ocs_to_wcs_for_mirrored_normal() {
        // Arbitrary-axis algorithm for N = -Z gives Ax = (-1,0,0), Ay = (0,1,0).
        assert_eq!(
            ocs_to_wcs([0.0, 0.0, -1.0], [2.0, 3.0, 4.0]),
            Point3::new(-2.0, 3.0, -4.0)
        );
        assert_eq!(
            ocs_to_wcs([0.0, 0.0, 1.0], [2.0, 3.0, 4.0]),
            Point3::new(2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn colors() {
        let c = |index: i16, value: Option<u32>| CmColor {
            index,
            value,
            ..CmColor::default()
        };
        assert_eq!(to_color(&c(256, None)), Color::ByLayer);
        assert_eq!(to_color(&c(0, None)), Color::ByBlock);
        assert_eq!(to_color(&c(-5, None)), Color::Aci { index: 5 });
        assert_eq!(to_color(&c(0, Some(0xC0_000000))), Color::ByLayer);
        assert_eq!(
            to_color(&c(0, Some(0xC3_000011))),
            Color::Aci { index: 0x11 }
        );
        assert_eq!(
            to_color(&c(0, Some(0xC2_34C918))),
            Color::Rgb {
                r: 0x34,
                g: 0xC9,
                b: 0x18
            }
        );
    }

    #[test]
    fn lineweights() {
        assert_eq!(lineweight(None), Lineweight::ByLayer);
        assert_eq!(lineweight(Some(29)), Lineweight::ByLayer);
        assert_eq!(lineweight(Some(30)), Lineweight::ByBlock);
        assert_eq!(lineweight(Some(31)), Lineweight::Default);
        assert_eq!(lineweight(Some(9)), Lineweight::Millimeters(0.35));
    }

    use crate::native::{ClassTable, CommonData, Control, Dimension, DwgClass, Line};

    const CONTROL: u64 = 0x1;
    const MODEL: u64 = 0x1F;
    const OTHER: u64 = 0x20;

    fn object(
        handle: u64,
        type_code: u16,
        data: ObjectData,
        owner: Option<u64>,
        entity_mode: Option<u8>,
    ) -> DwgObject {
        DwgObject {
            handle,
            type_code,
            type_name: String::new(),
            is_entity: entity_mode.is_some(),
            offset: 0,
            size: 0,
            crc_ok: None,
            common: CommonData {
                owner,
                entity: entity_mode.map(|entity_mode| EntityCommon {
                    entity_mode,
                    ..EntityCommon::default()
                }),
                ..CommonData::default()
            },
            data,
            raw: Vec::new(),
            handle_stream_bit: 0,
            string_stream: None,
            error: None,
            warnings: Vec::new(),
            data_bits_left: None,
            handle_bits_left: None,
        }
    }

    /// An R2004 file whose model space lists `entities` and holds `extra` objects.
    fn file_with_model_space(entities: Vec<u64>, extra: Vec<DwgObject>) -> DwgFile {
        let mut header = crate::native::HeaderVars::default();
        header
            .handles
            .insert("BLOCK_CONTROL_OBJECT".into(), CONTROL);
        header
            .handles
            .insert("BLOCK_RECORD_MODEL_SPACE".into(), MODEL);
        let bh = BlockHeader {
            entry: TableEntry {
                name: "*Model_Space".into(),
                ..TableEntry::default()
            },
            entities,
            ..BlockHeader::default()
        };
        let mut objects = BTreeMap::new();
        for o in [
            object(
                CONTROL,
                0x30,
                ObjectData::Control(Control {
                    entries: vec![MODEL],
                    extra: Vec::new(),
                }),
                None,
                None,
            ),
            object(
                MODEL,
                0x31,
                ObjectData::BlockHeader(bh),
                Some(CONTROL),
                None,
            ),
        ]
        .into_iter()
        .chain(extra)
        {
            objects.insert(o.handle, o);
        }
        DwgFile {
            version: crate::DwgVersion::R2004,
            maintenance_version: 0,
            codepage: 30,
            encoding: "windows-1252",
            header,
            measurement: None,
            classes: Vec::new(),
            class_index: Default::default(),
            classes_crc_ok: None,
            handle_map: Default::default(),
            objects,
            section_names: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn line(handle: u64, owner: Option<u64>, mode: u8) -> DwgObject {
        object(
            handle,
            0x13,
            ObjectData::Line(Line {
                end: [1.0, 0.0, 0.0],
                extrusion: [0.0, 0.0, 1.0],
                ..Line::default()
            }),
            owner,
            Some(mode),
        )
    }

    #[test]
    fn each_entity_is_placed_once_in_its_owner() {
        // The model space lists line 0x50 a thousand times, a line owned by another
        // block, and a line in model space by entity mode.
        let mut list = vec![0x50; 1000];
        list.extend([0x51, 0x52]);
        let file = file_with_model_space(
            list,
            vec![
                line(0x50, Some(MODEL), 0),
                line(0x51, Some(OTHER), 0),
                line(0x52, None, 2),
            ],
        );
        let doc = to_document(&file, &ReadOptions::default()).unwrap();
        let ids: Vec<Option<u64>> = doc.models[0].entities.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![Some(0x50), Some(0x52)]);
        let codes: Vec<&str> = doc.warnings.iter().map(|w| w.code.as_str()).collect();
        assert!(
            codes.contains(&"dwg.duplicate_entity") && codes.contains(&"dwg.owner_mismatch"),
            "{codes:?}"
        );
    }

    #[test]
    fn document_wide_limits_apply_to_mapped_entities() {
        let file = file_with_model_space(
            vec![0x50, 0x52],
            vec![line(0x50, Some(MODEL), 0), line(0x52, None, 2)],
        );
        let mut options = ReadOptions::default();
        options.limits.max_objects = 1;
        assert!(matches!(
            to_document(&file, &options),
            Err(Error::LimitExceeded(_))
        ));
        let mut options = ReadOptions::default();
        options.limits.max_total_vertices = 1;
        assert!(matches!(
            to_document(&file, &options),
            Err(Error::LimitExceeded(_))
        ));
        options.limits.max_total_vertices = 2;
        assert!(to_document(&file, &options).is_ok());
    }

    #[test]
    fn angular_dimension_arc_point_is_converted_from_ocs() {
        // Normal -Z: OCS (x, y, z) is WCS (-x, y, -z); point 16 is 2D plus elevation.
        let d = Dimension {
            type_code: 0x18,
            extrusion: [0.0, 0.0, -1.0],
            elevation: 4.0,
            pt16: Some([2.0, 3.0]),
            ..Dimension::default()
        };
        let dim = object(0x60, 0x18, ObjectData::Dimension(d), Some(MODEL), Some(0));
        let file = file_with_model_space(vec![0x60], vec![dim]);
        let doc = to_document(&file, &ReadOptions::default()).unwrap();
        let EntityKind::Dimension { points, .. } = &doc.models[0].entities[0].kind else {
            panic!("not a dimension")
        };
        let roles = &doc.models[0].entities[0].props["dwg.dim_roles"];
        let Value::List(roles) = roles else {
            panic!("roles")
        };
        let at = roles
            .iter()
            .position(|r| *r == Value::Text("16".into()))
            .unwrap();
        assert_eq!(points[at], Point3::new(-2.0, 3.0, -4.0));
    }

    #[test]
    fn class_lookup_uses_the_index() {
        let classes = vec![
            DwgClass {
                number: 500,
                dxf_name: "A".into(),
                ..DwgClass::default()
            },
            DwgClass {
                number: 501,
                dxf_name: "B".into(),
                ..DwgClass::default()
            },
        ];
        let mut file = file_with_model_space(Vec::new(), Vec::new());
        file.class_index = ClassTable::new(&classes).into_index();
        file.classes = classes;
        assert_eq!(file.class(501).map(|c| c.dxf_name.as_str()), Some("B"));
        assert!(file.class(600).is_none());
        // Without an index (e.g. classes edited afterwards) the lookup still works.
        file.class_index.clear();
        assert_eq!(file.class(500).map(|c| c.dxf_name.as_str()), Some("A"));
    }

    #[test]
    fn anonymous_block_names() {
        let file = DwgFile {
            version: crate::DwgVersion::R2000,
            maintenance_version: 0,
            codepage: 30,
            encoding: "windows-1252",
            header: Default::default(),
            measurement: None,
            classes: Vec::new(),
            class_index: Default::default(),
            classes_crc_ok: None,
            handle_map: Default::default(),
            objects: Default::default(),
            section_names: Vec::new(),
            warnings: Vec::new(),
        };
        let bh = |name: &str| BlockHeader {
            entry: TableEntry {
                name: name.into(),
                ..TableEntry::default()
            },
            anonymous: name.starts_with('*'),
            ..BlockHeader::default()
        };
        // Without a BLOCK entity the short anonymous name gets the table index.
        assert_eq!(Mapper::block_name(&file, 0x10, &bh("*U"), Some(17)), "*U17");
        assert_eq!(
            Mapper::block_name(&file, 0x10, &bh("MyBlock"), Some(3)),
            "MyBlock"
        );
        assert_eq!(Mapper::block_name(&file, 0x10, &bh(""), None), "*BLOCK_10");
    }
}
