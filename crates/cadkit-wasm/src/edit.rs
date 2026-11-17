//! Operations that change a document in place: unit conversion, insert explosion, affine
//! transforms, layer and kind filters and purging of unused definitions.

use std::collections::{HashMap, HashSet};

use cadkit::ops::DEFAULT_WALK_DEPTH;
use cadkit::{Block, Entity, EntityKind, LengthUnit, Transform, Vec3};
use serde_json::json;
use wasm_bindgen::prelude::*;

use crate::access::{kind_filter, kind_tag};
use crate::args;
use crate::document::CadDocument;
use crate::error::{bad_argument, from_cadkit, limit_exceeded};
use crate::options::{Call, Opt, Opts, numbers_of};
use crate::value::json_to_js;

/// Default and highest number of entities `explodeInserts` may add. At about 800 bytes per
/// entity the default is about 800 MB of wasm memory.
const EXPLODE_DEFAULT: u64 = 1_000_000;
const EXPLODE_CEILING: u64 = 3_000_000;
/// Deepest group nesting that `retainKinds` filters; deeper groups are kept as they are.
const RETAIN_DEPTH: usize = 256;

#[wasm_bindgen]
impl CadDocument {
    /// Converts coordinates, block definitions, linetype dash lengths and fixed text heights in
    /// place to `unit` (`"mm"`, `"meter"`, `"ft"`, ...), returning the scale factor applied.
    /// Changes this document.
    #[wasm_bindgen(js_name = convertUnits)]
    pub fn convert_units(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] unit: JsValue,
    ) -> Result<f64, JsValue> {
        let unit = args::string("unit", &unit, 64)?;
        let target = LengthUnit::from_name(&unit)
            .ok_or_else(|| bad_argument(&format!("unknown length unit {unit:?}")))?;
        self.doc.convert_units(target).map_err(|e| from_cadkit(&e))
    }

    /// Replaces block references (inserts, and dimensions with a block) in every model by the
    /// block's entities in model space. Changes this document. Options (`ExplodeOptions`):
    /// `maxDepth` (nesting expanded, default 32, at most 64) and `maxEntities` (entities the
    /// expansion may add, default 1,000,000, at most 3,000,000; an entity with many vertices
    /// counts as one per 16 of them, so that the budget bounds memory).
    ///
    /// What the expansion would add is counted first: when it exceeds `maxEntities` the call
    /// throws `limit_exceeded` (`limit: "maxEntities"`, `needed`: the count) and the document
    /// is unchanged. Returns `{truncated, work, entities}`: `entities` counts the top-level
    /// entities afterwards; `truncated` is `true` when the traversal's own work budget still cut
    /// the expansion short (the inserts not reached stay inserts).
    #[wasm_bindgen(js_name = explodeInserts, unchecked_return_type = "ExplodeResult")]
    pub fn explode_inserts(
        &mut self,
        #[wasm_bindgen(unchecked_optional_param_type = "ExplodeOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("explodeInserts");
        let mut o = Opts::parse("options", options.as_ref())?;
        let max_depth = o
            .uint_capped("maxDepth", 64)?
            .map_or(DEFAULT_WALK_DEPTH, |d| u32::try_from(d).unwrap_or(64));
        let max_entities = o
            .uint_capped("maxEntities", EXPLODE_CEILING)?
            .unwrap_or(EXPLODE_DEFAULT);
        o.finish(&mut call)?;
        let present: u64 = self
            .doc
            .models
            .iter()
            .map(|m| m.entities.len() as u64)
            .sum();
        let est = estimate(&self.doc, max_depth);
        // Checked before anything changes: a walk cut by its budget drops the cells of the
        // insert it was expanding, so the budget must not be what stops a large expansion.
        if est.weight > max_entities {
            return Err(limit_exceeded(
                "maxEntities",
                Some(max_entities),
                Some(est.weight),
                &format!(
                    "exploding the inserts would add about {} entities (weighted by size), more than maxEntities {max_entities}",
                    est.weight
                ),
            ));
        }
        // Every visited entity and array cell costs one work unit. The estimate can be low for
        // unusual nesting (blocks met at several depths), hence the margin.
        let budget = present
            .saturating_add(est.work.saturating_mul(2))
            .saturating_add(1024);
        let stats = self.doc.explode_inserts_with(max_depth, budget);
        let entities: usize = self.doc.models.iter().map(|m| m.entities.len()).sum();
        json_to_js(&json!({
            "truncated": stats.truncated,
            "work": stats.work,
            "entities": entities,
        }))
    }

    /// Applies an affine transform in place to the entities of the design models (block
    /// definitions stay in block coordinates; inserts absorb the transform). Changes this
    /// document. `spec` (`TransformSpec`) is a row-major matrix (12 numbers or 3 rows of 4;
    /// 16 numbers or 4 rows of 4 with a last row of `0, 0, 0, 1`), `{matrix}`, or parts applied
    /// in the order scale, rotation, translation: `{scale: s | [sx, sy, sz], rotateZ: radians,
    /// translate: [x, y] | [x, y, z], origin: [x, y] | [x, y, z]}` (`origin` is the fixed point
    /// of scale and rotation). A singular or non-finite matrix is a `bad_argument` error.
    pub fn transform(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "TransformSpec | string")] spec: JsValue,
    ) -> Result<(), JsValue> {
        let mut call = Call::new("transform");
        let tf = transform_spec(&mut call, &spec)?;
        self.doc.transform(&tf);
        Ok(())
    }

    /// Keeps only the layers named in `names` and the model entities on them (own layer; the
    /// default layer is `"0"`). Changes this document. Returns `{layers, entities}`: how many
    /// layer definitions and model entities (group children included) were removed.
    #[wasm_bindgen(js_name = retainLayers, unchecked_return_type = "RetainResult")]
    pub fn retain_layers(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string[]")] names: JsValue,
    ) -> Result<JsValue, JsValue> {
        let names = string_list("names", &names)?;
        let keep: HashSet<&str> = names.iter().map(String::as_str).collect();
        let layers_before = self.doc.layers.len();
        let entities_before = self.entity_total();
        self.doc.retain_layers(|name| keep.contains(name));
        json_to_js(&json!({
            "layers": layers_before - self.doc.layers.len().min(layers_before),
            "entities": entities_before.saturating_sub(self.entity_total()),
        }))
    }

    /// Keeps only model entities whose JSON `type` tag is in `kinds` (`"line"`, `"m_text"`,
    /// ...). A group whose kind is not listed keeps the children that pass and is removed when
    /// none does. Block definitions are not changed. Changes this document. Returns
    /// `{layers: 0, entities}` with the number of entities removed.
    #[wasm_bindgen(js_name = retainKinds, unchecked_return_type = "RetainResult")]
    pub fn retain_kinds(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "EntityKindName[]")] kinds: JsValue,
    ) -> Result<JsValue, JsValue> {
        let kinds = kind_filter(string_list("kinds", &kinds)?, "kinds")?;
        let before = self.entity_total();
        for model in &mut self.doc.models {
            retain_kinds_in(&mut model.entities, &kinds, 0);
        }
        json_to_js(&json!({
            "layers": 0,
            "entities": before.saturating_sub(self.entity_total()),
        }))
    }

    /// Removes block, layer (except `"0"`), linetype and text style definitions that nothing
    /// refers to. Changes this document. Returns the counts removed
    /// `{blocks, layers, linetypes, textStyles}`.
    #[wasm_bindgen(js_name = purgeUnused, unchecked_return_type = "PurgeResult")]
    pub fn purge_unused(&mut self) -> Result<JsValue, JsValue> {
        let s = self.doc.purge_unused();
        json_to_js(&json!({
            "blocks": s.blocks,
            "layers": s.layers,
            "linetypes": s.linetypes,
            "textStyles": s.text_styles,
        }))
    }
}

impl CadDocument {
    /// Entities of every model, group children included.
    fn entity_total(&self) -> u64 {
        let mut total = 0u64;
        let mut stack: Vec<&[Entity]> = self.doc.models.iter().map(|m| &m.entities[..]).collect();
        while let Some(list) = stack.pop() {
            total += list.len() as u64;
            for e in list {
                if let EntityKind::Group { children, .. } = &e.kind {
                    stack.push(children);
                }
            }
        }
        total
    }
}

fn string_list(name: &str, value: &JsValue) -> Result<Vec<String>, JsValue> {
    if !js_sys::Array::is_array(value) {
        return Err(bad_argument(&format!("{name} must be an array of strings")));
    }
    let array: &js_sys::Array = wasm_bindgen::JsCast::unchecked_ref(value);
    if array.length() > 100_000 {
        return Err(bad_argument(&format!("{name} has more than 100000 items")));
    }
    array.iter().map(|v| args::string(name, &v, 2048)).collect()
}

fn retain_kinds_in(list: &mut Vec<Entity>, kinds: &[&str], depth: usize) {
    list.retain_mut(|e| {
        if kinds.contains(&kind_tag(&e.kind)) {
            return true;
        }
        match &mut e.kind {
            EntityKind::Group { children, .. } if depth < RETAIN_DEPTH => {
                retain_kinds_in(children, kinds, depth + 1);
                !children.is_empty()
            }
            EntityKind::Group { .. } => true,
            _ => false,
        }
    });
}

/// What exploding would do: entities added, their weight (one unit plus one per 16
/// vertex-like items, as a vertex costs about 1/16 of an entity's memory) and the work units
/// the expansion spends (entities visited and array cells).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Estimate {
    added: u64,
    weight: u64,
    work: u64,
}

impl Estimate {
    fn plus(self, o: Estimate) -> Estimate {
        Estimate {
            added: self.added.saturating_add(o.added),
            weight: self.weight.saturating_add(o.weight),
            work: self.work.saturating_add(o.work),
        }
    }

    fn times(self, n: u64) -> Estimate {
        Estimate {
            added: self.added.saturating_mul(n),
            weight: self.weight.saturating_mul(n),
            work: self.work.saturating_mul(n),
        }
    }
}

fn estimate(doc: &cadkit::Document, max_depth: u32) -> Estimate {
    let mut est = Estimator {
        blocks: doc.blocks.iter().map(|b| (b.name.as_str(), b)).collect(),
        memo: HashMap::new(),
        stack: Vec::new(),
        max_depth,
    };
    let mut total = Estimate::default();
    for model in &doc.models {
        for e in &model.entities {
            if let Some(add) = est.insert(e, 0) {
                total = total.plus(add);
            }
        }
    }
    total
}

struct Estimator<'a> {
    blocks: HashMap<&'a str, &'a Block>,
    memo: HashMap<&'a str, Estimate>,
    stack: Vec<&'a str>,
    max_depth: u32,
}

impl<'a> Estimator<'a> {
    /// What replaces `e` when it is an expandable insert (`None` when it stays as it is).
    fn insert(&mut self, e: &'a Entity, depth: u32) -> Option<Estimate> {
        let (name, cells) = match &e.kind {
            EntityKind::Insert {
                block,
                columns,
                rows,
                ..
            } => (
                block.as_str(),
                u64::from((*columns).max(1)) * u64::from((*rows).max(1)),
            ),
            EntityKind::Dimension {
                block: Some(block), ..
            } => (block.as_str(), 1),
            _ => return None,
        };
        let def = self.blocks.get(name).copied()?;
        if depth >= self.max_depth || self.stack.contains(&def.name.as_str()) {
            return None;
        }
        let per_cell = match self.memo.get(def.name.as_str()) {
            Some(v) => *v,
            None => {
                self.stack.push(def.name.as_str());
                let mut sum = Estimate::default();
                for child in &def.entities {
                    let add = self.insert(child, depth + 1).unwrap_or(Estimate {
                        added: 1,
                        weight: weight(&child.kind),
                        work: 0,
                    });
                    // Visiting the child costs one unit whether it is expanded or copied.
                    sum = sum.plus(add).plus(Estimate {
                        work: 1,
                        ..Estimate::default()
                    });
                }
                self.stack.pop();
                self.memo.insert(def.name.as_str(), sum);
                sum
            }
        };
        let attrs = e.attributes.len() as u64;
        Some(
            per_cell
                .plus(Estimate {
                    work: 1,
                    ..Estimate::default()
                })
                .times(cells)
                .plus(Estimate {
                    added: attrs,
                    weight: attrs,
                    work: 0,
                }),
        )
    }
}

fn weight(kind: &EntityKind) -> u64 {
    let n = |len: usize| len as u64;
    let items = match kind {
        EntityKind::Polyline { vertices, .. } => n(vertices.len()),
        EntityKind::Leader { vertices, .. } => n(vertices.len()),
        EntityKind::Spline {
            control_points,
            fit_points,
            knots,
            ..
        } => n(control_points.len() + fit_points.len() + knots.len()),
        EntityKind::Hatch { loops, .. } => loops
            .iter()
            .flat_map(|l| l.edges.iter())
            .map(|e| match e {
                cadkit::HatchEdge::Polyline { vertices, .. } => 1 + n(vertices.len()),
                cadkit::HatchEdge::Spline { control_points, .. } => 1 + n(control_points.len()),
                _ => 1,
            })
            .fold(0u64, u64::saturating_add),
        EntityKind::Polygon {
            exterior,
            interiors,
        } => interiors
            .iter()
            .fold(n(exterior.len()), |s, r| s.saturating_add(n(r.len()))),
        EntityKind::Mesh { vertices, faces } => faces
            .iter()
            .fold(n(vertices.len()), |s, f| s.saturating_add(n(f.len()))),
        EntityKind::Face { points, .. } | EntityKind::Dimension { points, .. } => n(points.len()),
        EntityKind::Text { value, .. } => n(value.len() / 8),
        EntityKind::MText { value, plain, .. } => n((value.len() + plain.len()) / 8),
        _ => 0,
    };
    1 + items / 16
}

/// Parses a `TransformSpec` (see `CadDocument::transform`).
fn transform_spec(call: &mut Call, spec: &JsValue) -> Result<Transform, JsValue> {
    // A bare matrix (array or JSON text of an array) is the same as `{ matrix }`.
    let wrapped;
    let spec = if js_sys::Array::is_array(spec) {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"matrix".into(), spec)?;
        wrapped = JsValue::from(obj);
        &wrapped
    } else if let Some(text) = spec.as_string().filter(|t| t.trim_start().starts_with('[')) {
        wrapped = JsValue::from_str(&format!("{{\"matrix\":{text}}}"));
        &wrapped
    } else {
        spec
    };
    if spec.is_undefined() || spec.is_null() {
        return Err(bad_argument(
            "transform needs a matrix or {scale, rotateZ, translate}",
        ));
    }
    let mut o = Opts::parse("spec", Some(spec))?;
    let matrix = o.take("matrix")?;
    let scale = o.take("scale")?;
    let rotate = o.number("rotateZ", -1e9, 1e9)?;
    let translate = o.numbers("translate", 3)?;
    let origin = o.numbers("origin", 3)?;
    o.finish(call)?;
    let parts = scale.is_some() || rotate.is_some() || translate.is_some() || origin.is_some();
    let tf = match matrix {
        Some(_) if parts => {
            return Err(bad_argument(
                "give either matrix or scale/rotateZ/translate/origin",
            ));
        }
        Some(m) => matrix_of(&m)?,
        None => {
            let point = |v: Option<Vec<f64>>, name: &str| -> Result<Vec3, JsValue> {
                match v.as_deref() {
                    None => Ok(Vec3::new(0.0, 0.0, 0.0)),
                    Some([x, y]) => Ok(Vec3::new(*x, *y, 0.0)),
                    Some([x, y, z]) => Ok(Vec3::new(*x, *y, *z)),
                    Some(_) => Err(bad_argument(&format!("{name} must have 2 or 3 numbers"))),
                }
            };
            let (sx, sy, sz) = match scale {
                None | Some(Opt::Null) => (1.0, 1.0, 1.0),
                Some(Opt::Num(s)) if s.is_finite() => (s, s, s),
                Some(other) => match numbers_of(&other, 3).as_deref() {
                    Some([x, y]) => (*x, *y, 1.0),
                    Some([x, y, z]) => (*x, *y, *z),
                    _ => return Err(bad_argument("scale must be a number or 2 or 3 numbers")),
                },
            };
            let origin = point(origin, "origin")?;
            let back = Vec3::new(-origin.x, -origin.y, -origin.z);
            Transform::translation(point(translate, "translate")?)
                .compose(&Transform::translation(origin))
                .compose(&Transform::rotation_z(rotate.unwrap_or(0.0)))
                .compose(&Transform::scaling(sx, sy, sz))
                .compose(&Transform::translation(back))
        }
    };
    let m = tf.to_matrix();
    let det = tf.determinant();
    if !m.iter().flatten().all(|v| v.is_finite()) || !det.is_finite() || det == 0.0 {
        return Err(bad_argument("the transform must be finite and invertible"));
    }
    Ok(tf)
}

fn matrix_of(m: &Opt) -> Result<Transform, JsValue> {
    let wrong =
        || bad_argument("matrix must be 12 or 16 numbers, or 3 or 4 rows of 4 numbers (row-major)");
    let flat: Vec<f64> = match numbers_of(m, 16) {
        Some(v) => v,
        None => {
            let Opt::List(rows) = m else {
                return Err(wrong());
            };
            let mut out = Vec::with_capacity(16);
            for row in rows.iter().take(5) {
                let r = numbers_of(row, 4)
                    .filter(|r| r.len() == 4)
                    .ok_or_else(wrong)?;
                out.extend(r);
            }
            if rows.len() > 4 {
                return Err(wrong());
            }
            out
        }
    };
    let row = |i: usize| -> Result<[f64; 4], JsValue> {
        flat.get(i * 4..i * 4 + 4)
            .and_then(|r| <[f64; 4]>::try_from(r).ok())
            .ok_or_else(wrong)
    };
    match flat.len() {
        12 => {}
        16 if row(3)? == [0.0, 0.0, 0.0, 1.0] => {}
        16 => {
            return Err(bad_argument(
                "the last row of a 4x4 matrix must be 0, 0, 0, 1",
            ));
        }
        _ => return Err(wrong()),
    }
    Ok(Transform::from_matrix([row(0)?, row(1)?, row(2)?]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use cadkit::{Document, Point3};

    #[test]
    fn estimate_multiplies_array_cells_and_saturates() {
        let mut doc = Document::new();
        let mut b = Block::new("B");
        b.entities
            .push(Entity::line(Point3::ZERO, Point3::xy(1.0, 0.0)));
        doc.blocks.push(b);
        let mut insert = Entity::insert("B", Point3::ZERO);
        if let EntityKind::Insert { columns, rows, .. } = &mut insert.kind {
            *columns = 32_767;
            *rows = 32_767;
        }
        doc.model_space_mut().entities.push(insert);
        let est = estimate(&doc, 32);
        assert_eq!(est.added, 32_767 * 32_767);
        assert!(est.weight >= est.added);
        assert_eq!(est.work, 32_767 * 32_767 * 2);
    }

    #[test]
    fn matrices_parse_in_both_layouts() {
        let flat = Opt::List((0..12).map(|i| Opt::Num(f64::from(i))).collect());
        let tf = matrix_of(&flat).unwrap();
        assert_eq!(tf.to_matrix()[1], [4.0, 5.0, 6.0, 7.0]);
        let rows = Opt::List(vec![
            Opt::List(vec![
                Opt::Num(1.0),
                Opt::Num(0.0),
                Opt::Num(0.0),
                Opt::Num(5.0),
            ]),
            Opt::List(vec![
                Opt::Num(0.0),
                Opt::Num(1.0),
                Opt::Num(0.0),
                Opt::Num(6.0),
            ]),
            Opt::List(vec![
                Opt::Num(0.0),
                Opt::Num(0.0),
                Opt::Num(1.0),
                Opt::Num(0.0),
            ]),
            Opt::List(vec![
                Opt::Num(0.0),
                Opt::Num(0.0),
                Opt::Num(0.0),
                Opt::Num(1.0),
            ]),
        ]);
        assert_eq!(matrix_of(&rows).unwrap().t, Vec3::new(5.0, 6.0, 0.0));
    }

    #[test]
    fn retain_kinds_keeps_groups_with_passing_children() {
        let group = Entity::new(EntityKind::Group {
            group_kind: cadkit::GroupKind::Other,
            name: None,
            origin: None,
            children: vec![
                Entity::line(Point3::ZERO, Point3::xy(1.0, 0.0)),
                Entity::circle(Point3::ZERO, 1.0),
            ],
        });
        let mut list = vec![group, Entity::circle(Point3::ZERO, 2.0)];
        retain_kinds_in(&mut list, &["line"], 0);
        assert_eq!(list.len(), 1);
        let EntityKind::Group { children, .. } = &list[0].kind else {
            unreachable!()
        };
        assert_eq!(children.len(), 1);
    }
}
