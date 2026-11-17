//! Reading parts of a document without converting all of it: models, single entities by
//! path, pages of entities, blocks, statistics, bounding boxes and viewports. Values keep the
//! document schema's shape (docs/schema/document-v1.schema.json, snake_case keys); the wrappers
//! around them (pages, summaries, statistics) use camelCase like `info()`.

use std::collections::BTreeMap;

use cadkit::ops::{BBoxOptions, WalkOptions, entity_bbox};
use cadkit::{BBox, Entity, EntityKind};
use serde::Serialize;
use serde_json::json;
use wasm_bindgen::prelude::*;

use crate::args;
use crate::document::CadDocument;
use crate::error::{bad_argument, limit_exceeded};
use crate::options::{Call, Opts};
use crate::value::{CappedWriter, json_to_js, parse, to_js};

/// The JSON `type` tag of every entity kind (`EntityKind::type_name` spells `m_text` as
/// `mtext`).
pub(crate) const KIND_TAGS: [&str; 20] = [
    "point",
    "line",
    "polyline",
    "circle",
    "arc",
    "ellipse",
    "spline",
    "text",
    "m_text",
    "insert",
    "hatch",
    "dimension",
    "face",
    "polygon",
    "leader",
    "image",
    "viewport",
    "mesh",
    "group",
    "unknown",
];

/// JSON `type` tag of `kind`.
pub(crate) fn kind_tag(kind: &EntityKind) -> &'static str {
    match kind.type_name() {
        "mtext" => "m_text",
        other => other,
    }
}

/// Kind names given by a caller, normalized to JSON tags (`"mtext"` is accepted for
/// `"m_text"`); an unknown name is an error, so a typo does not silently match nothing.
pub(crate) fn kind_filter(names: Vec<String>, option: &str) -> Result<Vec<&'static str>, JsValue> {
    names
        .iter()
        .map(|n| {
            let n = if n == "mtext" { "m_text" } else { n.as_str() };
            KIND_TAGS.iter().copied().find(|t| *t == n).ok_or_else(|| {
                bad_argument(&format!(
                    "option {option}: unknown entity kind {n:?} (kinds: {})",
                    KIND_TAGS.join(", ")
                ))
            })
        })
        .collect()
}

/// Layer name of an entity as stored (`None` is the default layer `"0"`).
pub(crate) fn own_layer(e: &Entity) -> &str {
    e.layer.as_deref().unwrap_or("0")
}

#[derive(Serialize)]
pub(crate) struct BoxJson {
    min: [f64; 3],
    max: [f64; 3],
}

impl From<&BBox> for BoxJson {
    fn from(b: &BBox) -> Self {
        Self {
            min: [b.min.x, b.min.y, b.min.z],
            max: [b.max.x, b.max.y, b.max.z],
        }
    }
}

/// Default and highest number of entities in one page of `entities()`.
const PAGE_DEFAULT: u64 = 1_000;
const PAGE_MAX: u64 = 100_000;
/// Default and highest JSON size of one page.
const PAGE_BYTES_DEFAULT: u64 = 16 << 20;
const PAGE_BYTES_MAX: u64 = 256 << 20;
const FILTER_ITEMS: usize = 20_000;

#[wasm_bindgen]
impl CadDocument {
    /// The models (model space, layouts, DGN models) without their entities:
    /// `{index, name, id, kind, is3d, entities, props}` where `entities` is the number of
    /// top-level entities.
    #[wasm_bindgen(unchecked_return_type = "ModelSummary[]")]
    pub fn models(&self) -> Result<JsValue, JsValue> {
        let v: Vec<serde_json::Value> = self
            .doc
            .models
            .iter()
            .enumerate()
            .map(|(i, m)| {
                json!({
                    "index": i,
                    "name": m.name,
                    "id": m.id,
                    "kind": m.kind,
                    "is3d": m.is_3d,
                    "entities": m.entities.len(),
                    "props": m.props,
                })
            })
            .collect();
        json_to_js(&serde_json::Value::Array(v))
    }

    /// Number of top-level entities of `model` (group children and block contents not counted).
    #[wasm_bindgen(js_name = entityCount)]
    pub fn entity_count(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] model: JsValue,
    ) -> Result<usize, JsValue> {
        let m = self.model_index(&model)?;
        Ok(self.doc.models.get(m).map_or(0, |m| m.entities.len()))
    }

    /// The entity at `path` in `model` (an index of the model's entity list, or an array of
    /// indices that steps into group children and into the block of an insert, as `pick()`
    /// returns it), in the document schema's `Entity` shape; `null` when the path does not
    /// exist. An insert is returned as the insert, not its block's content.
    #[wasm_bindgen(unchecked_return_type = "Entity | null")]
    pub fn entity(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] model: JsValue,
        #[wasm_bindgen(unchecked_param_type = "EntityPath")] path: JsValue,
    ) -> Result<JsValue, JsValue> {
        let m = self.model_index(&model)?;
        let path = args::path("path", &path)?;
        match self.doc.entity_at(m, &path) {
            Some(e) => to_js(e),
            None => Ok(JsValue::NULL),
        }
    }

    /// One page of the top-level entities of `model`. Options (`EntityPageOptions`):
    /// `offset` (default 0) and `limit` (default 1,000, at most 100,000) count entities that
    /// pass the filters `layers` (own layer name; the default layer is `"0"`) and `kinds`
    /// (JSON `type` tags); `maxOutputBytes` (default 16 MiB, at most 256 MiB) bounds the JSON
    /// size of the page. Returns `{total, offset, next, entities: [{index, entity}]}`: `total`
    /// counts the matching entities, `index` is the entity's index in the model, `next` is the
    /// offset of the next page or `null`. A single entity larger than `maxOutputBytes` throws
    /// `limit_exceeded`.
    #[wasm_bindgen(unchecked_return_type = "EntityPage")]
    pub fn entities(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] model: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "EntityPageOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("entities");
        let m = self.model_index(&model)?;
        let mut o = Opts::parse("options", options.as_ref())?;
        let offset = o
            .uint("offset", 0, crate::args::MAX_SAFE_INTEGER as u64)?
            .unwrap_or(0);
        let limit = o.uint("limit", 1, PAGE_MAX)?.unwrap_or(PAGE_DEFAULT);
        let max_bytes = o
            .uint("maxOutputBytes", 64, PAGE_BYTES_MAX)?
            .unwrap_or(PAGE_BYTES_DEFAULT);
        let layers = o.strings("layers", FILTER_ITEMS, 2048, 1 << 20)?;
        let kinds = match o.strings("kinds", 64, 32, 4096)? {
            Some(k) => Some(kind_filter(k, "kinds")?),
            None => None,
        };
        o.finish(&mut call)?;
        let layers: Option<std::collections::HashSet<&str>> = layers
            .as_ref()
            .map(|l| l.iter().map(String::as_str).collect());
        let list = self.doc.models.get(m).map_or(&[][..], |m| &m.entities[..]);
        let matches = |e: &Entity| {
            layers.as_ref().is_none_or(|l| l.contains(own_layer(e)))
                && kinds
                    .as_ref()
                    .is_none_or(|k| k.contains(&kind_tag(&e.kind)))
        };
        let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
        let mut out = CappedWriter::new(max_bytes);
        let mut total = 0u64;
        let mut taken = 0u64;
        let mut next: Option<u64> = None;
        out.raw("[");
        for (index, e) in list.iter().enumerate() {
            if !matches(e) {
                continue;
            }
            let position = total;
            total += 1;
            if position < offset || next.is_some() {
                continue;
            }
            if taken == limit {
                next = Some(position);
                continue;
            }
            let item = PageItem { index, entity: e };
            let mark = out.bytes.len();
            let fits = (taken == 0 || out.raw(",")) && out.push(&item);
            if !fits {
                out.bytes.truncate(mark);
                if taken == 0 {
                    return Err(limit_exceeded(
                        "maxOutputBytes",
                        Some(max_bytes as u64),
                        None,
                        &format!("entity {index} does not fit in maxOutputBytes"),
                    ));
                }
                next = Some(position);
                continue;
            }
            taken += 1;
        }
        // Room for "]" is not reserved, so close the array past the budget: one byte.
        out.limit = out.limit.saturating_add(1);
        out.raw("]");
        let text = format!(
            "{{\"total\":{total},\"offset\":{offset},\"next\":{},\"entities\":{}}}",
            next.map_or("null".to_owned(), |n| n.to_string()),
            out.into_string()
        );
        parse(&text)
    }

    /// Block definitions without their entities:
    /// `{name, id, entities, basePoint, description, isXref, xrefPath}` where `entities` is
    /// the number of entities in the block.
    #[wasm_bindgen(unchecked_return_type = "BlockSummary[]")]
    pub fn blocks(&self) -> Result<JsValue, JsValue> {
        let v: Vec<serde_json::Value> = self
            .doc
            .blocks
            .iter()
            .map(|b| {
                json!({
                    "name": b.name,
                    "id": b.id,
                    "entities": b.entities.len(),
                    "basePoint": b.base_point,
                    "description": b.description,
                    "isXref": b.is_xref,
                    "xrefPath": b.xref_path,
                })
            })
            .collect();
        json_to_js(&serde_json::Value::Array(v))
    }

    /// The block definition named `name` in the document schema's `Block` shape, or `null`.
    #[wasm_bindgen(unchecked_return_type = "Block | null")]
    pub fn block(
        &self,
        #[wasm_bindgen(unchecked_param_type = "string")] name: JsValue,
    ) -> Result<JsValue, JsValue> {
        let name = args::string("name", &name, args::MAX_NAME_UNITS)?;
        match self.doc.blocks.iter().find(|b| b.name == name) {
            Some(b) => to_js(b),
            None => Ok(JsValue::NULL),
        }
    }

    /// Counts of `model` (or of every model and block definition when `model` is omitted):
    /// `entities` (top-level), `nested` (group children at any depth), `blockEntities`,
    /// `kinds` (per JSON `type` tag, nested ones included), `unknownTypes` (source type names
    /// of `unknown` entities) and `layers` (entities per own layer name).
    #[wasm_bindgen(unchecked_return_type = "DocumentStats")]
    pub fn stats(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "number | null")] model: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut s = Stats::default();
        match model.filter(|m| !m.is_undefined() && !m.is_null()) {
            Some(m) => {
                let m = self.model_index(&m)?;
                if let Some(model) = self.doc.models.get(m) {
                    s.add_list(&model.entities, Counter::Model);
                }
            }
            None => {
                for model in &self.doc.models {
                    s.add_list(&model.entities, Counter::Model);
                }
                for block in &self.doc.blocks {
                    s.add_list(&block.entities, Counter::Block);
                }
            }
        }
        to_js(&s)
    }

    /// Model-space bounding box `{min: [x, y, z], max: [x, y, z]}` of `model` (visible layers,
    /// as `toSvg` frames it), or of everything drawn for the entity at `path` (every layer;
    /// for an insert all its array cells); `null` when there is no geometry.
    #[wasm_bindgen(unchecked_return_type = "BoundingBox | null")]
    pub fn bbox(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] model: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "EntityPath | null")] path: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let m = self.model_index(&model)?;
        let path = match path.filter(|p| !p.is_undefined() && !p.is_null()) {
            Some(p) => Some(args::path("path", &p)?),
            None => None,
        };
        let bb = match path {
            None => self.doc.bbox_with(m, &BBoxOptions::default()),
            Some(path) => self.path_bbox(m, &path),
        };
        match bb {
            Some(b) => to_js(&BoxJson::from(&b)),
            None => Ok(JsValue::NULL),
        }
    }

    /// The viewport entities of `model` (usually a paper-space layout), group children
    /// included, as `[{path, entity}]`.
    #[wasm_bindgen(unchecked_return_type = "ViewportItem[]")]
    pub fn viewports(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] model: JsValue,
    ) -> Result<JsValue, JsValue> {
        let m = self.model_index(&model)?;
        let list = self.doc.models.get(m).map_or(&[][..], |m| &m.entities[..]);
        let mut found: Vec<PathItem<'_>> = Vec::new();
        let mut stack: Vec<(&[Entity], Vec<usize>)> = vec![(list, Vec::new())];
        while let Some((entities, prefix)) = stack.pop() {
            for (i, e) in entities.iter().enumerate() {
                let mut path = prefix.clone();
                path.push(i);
                match &e.kind {
                    EntityKind::Viewport { .. } => found.push(PathItem { path, entity: e }),
                    EntityKind::Group { children, .. } => stack.push((children, path)),
                    _ => {}
                }
            }
        }
        found.sort_by(|a, b| a.path.cmp(&b.path));
        to_js(&found)
    }
}

impl CadDocument {
    fn path_bbox(&self, model: usize, path: &[usize]) -> Option<BBox> {
        self.doc.entity_at(model, path)?;
        let mut bb: Option<BBox> = None;
        let walk = WalkOptions::default();
        self.doc.walk_all_layers_while(model, &walk, |e, ctx| {
            // A walk path has depth + 1 steps; only entities at least as deep can match.
            if ctx.depth as usize + 1 >= path.len()
                && ctx.path().starts_with(path)
                && let Some(b) = entity_bbox(e, &ctx.transform)
            {
                match &mut bb {
                    Some(acc) => acc.union(&b),
                    None => bb = Some(b),
                }
            }
            true
        });
        bb
    }
}

#[derive(Serialize)]
struct PageItem<'a> {
    index: usize,
    entity: &'a Entity,
}

#[derive(Serialize)]
struct PathItem<'a> {
    path: Vec<usize>,
    entity: &'a Entity,
}

#[derive(Clone, Copy)]
enum Counter {
    Model,
    Block,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    entities: u64,
    nested: u64,
    block_entities: u64,
    kinds: BTreeMap<&'static str, u64>,
    unknown_types: BTreeMap<String, u64>,
    layers: BTreeMap<String, u64>,
}

impl Stats {
    fn add_list(&mut self, list: &[Entity], counter: Counter) {
        // An explicit stack: group nesting comes from the file and must not grow the call stack.
        let mut stack: Vec<(&[Entity], bool)> = vec![(list, true)];
        while let Some((entities, top)) = stack.pop() {
            for e in entities {
                match (counter, top) {
                    (Counter::Block, _) => self.block_entities += 1,
                    (Counter::Model, true) => self.entities += 1,
                    (Counter::Model, false) => self.nested += 1,
                }
                *self.kinds.entry(kind_tag(&e.kind)).or_default() += 1;
                *self.layers.entry(own_layer(e).to_owned()).or_default() += 1;
                match &e.kind {
                    EntityKind::Unknown { type_name } => {
                        *self.unknown_types.entry(type_name.clone()).or_default() += 1;
                    }
                    EntityKind::Group { children, .. } => stack.push((children, false)),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadkit::{Document, GroupKind, Point3};

    #[test]
    fn kind_tags_match_type_names() {
        assert_eq!(
            kind_tag(&EntityKind::Point {
                position: Point3::ZERO
            }),
            "point"
        );
        assert!(KIND_TAGS.contains(&"m_text"));
        assert_eq!(
            kind_filter(vec!["mtext".into()], "kinds").ok(),
            Some(vec!["m_text"])
        );
    }

    #[test]
    fn stats_count_nested_entities_without_recursion() {
        let mut doc = Document::new();
        let mut inner = Entity::line(Point3::ZERO, Point3::xy(1.0, 0.0));
        for _ in 0..1_000 {
            inner = Entity::new(EntityKind::Group {
                group_kind: GroupKind::Other,
                name: None,
                origin: None,
                children: vec![inner],
            });
        }
        doc.model_space_mut().entities.push(inner);
        let mut s = Stats::default();
        s.add_list(&doc.models[0].entities, Counter::Model);
        assert_eq!(s.entities, 1);
        assert_eq!(s.nested, 1_000);
        assert_eq!(s.kinds["line"], 1);
        assert_eq!(s.kinds["group"], 1_000);
    }
}
