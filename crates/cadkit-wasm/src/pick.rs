//! Picking: the entity closest to a point, for viewer selection.
//!
//! `pick` reports where the hit sits as a path (see `WalkContext::path`) that `entity()`
//! resolves, plus the model entity that owns it (the insert for a block part). The older
//! `pickEntity` / `pickEntityBounded` report `index`, which is the position of the hit in the
//! walk's visiting order (block contents and group children included), not an index of
//! `toJSON().models[m].entities`; they are kept for existing callers.

use std::collections::HashSet;

use cadkit::ops::{DEFAULT_WALK_DEPTH, WalkContext, WalkOptions, entity_bbox, entity_distance_xy};
use cadkit::{BBox, Document, Entity, EntityKind};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::args;
use crate::bounded_pick;
use crate::document::CadDocument;
use crate::error::{bad_argument, limit_exceeded};
use crate::options::{Call, Opts};
use crate::value::{json_to_js, parse};

/// Filters of a pick; names are `EntityKind::type_name` values (`"mtext"`), and the JSON tag
/// `"m_text"` is accepted for it.
#[derive(Default)]
pub(crate) struct PickOptions {
    pub(crate) excluded_layers: Vec<String>,
    pub(crate) excluded_kinds: Vec<String>,
}

pub(crate) struct PickCandidate<'a> {
    /// Visiting order of the walk; ties between equally close hits go to the later one.
    pub(crate) index: usize,
    pub(crate) entity: &'a Entity,
    pub(crate) context: WalkContext<'a>,
    pub(crate) bbox: BBox,
    pub(crate) distance: f64,
    area: f64,
}

/// Default and highest work budget of a pick (see `WalkOptions::max_work`).
const PICK_WORK: u64 = 5_000_000;
/// Highest JSON size of a bounded pick result.
const PICK_OUTPUT: u64 = 65_536;

/// Reads `excludedLayers` and `excludedKinds`: at most 20,000 names of at most 2,048 UTF-16
/// units each and 65,536 UTF-8 bytes per array.
pub(crate) fn filters(o: &mut Opts) -> Result<PickOptions, JsValue> {
    let layers = o.strings("excludedLayers", 20_000, 2048, 65_536)?;
    let kinds = o.strings("excludedKinds", 20_000, 2048, 65_536)?;
    Ok(PickOptions {
        excluded_layers: layers.unwrap_or_default(),
        excluded_kinds: kinds
            .unwrap_or_default()
            .into_iter()
            .map(|k| if k == "m_text" { "mtext".to_owned() } else { k })
            .collect(),
    })
}

pub(crate) fn pick_candidate_in_doc<'a>(
    doc: &'a Document,
    model: usize,
    point: (f64, f64),
    tolerance: f64,
    include_hidden_layers: bool,
    options: &PickOptions,
    max_work: u64,
) -> (Option<PickCandidate<'a>>, bool) {
    let (wx, wy) = point;
    if !(wx.is_finite() && wy.is_finite() && tolerance.is_finite() && tolerance >= 0.0) {
        return (None, false);
    }
    let walk_opts = WalkOptions::default()
        .with_max_depth(DEFAULT_WALK_DEPTH)
        .with_expand_inserts(true)
        .with_max_work(max_work);
    let mut visited_index = 0usize;
    let excluded_layers: HashSet<&str> =
        options.excluded_layers.iter().map(String::as_str).collect();
    let excluded_kinds: HashSet<&str> = options.excluded_kinds.iter().map(String::as_str).collect();
    let mut best: Option<PickCandidate<'a>> = None;
    let mut visit = |e: &'a Entity, ctx: &WalkContext<'a>| -> bool {
        let cur_index = visited_index;
        visited_index += 1;
        if excluded_layers.contains(ctx.layer_name(e))
            || excluded_kinds.contains(e.kind.type_name())
        {
            return true;
        }
        let Some(bb) = entity_bbox(e, &ctx.transform) else {
            return true;
        };
        if wx < bb.min.x - tolerance
            || wx > bb.max.x + tolerance
            || wy < bb.min.y - tolerance
            || wy > bb.max.y + tolerance
        {
            return true;
        }
        let Some(dist) = entity_distance_xy(e, &ctx.transform, (wx, wy)) else {
            return true;
        };
        if dist > tolerance {
            return true;
        }
        let area = bb.width().max(0.0) * bb.height().max(0.0);
        let replace = match &best {
            None => true,
            Some(prev) => {
                let tie_band = (tolerance * 0.05).max(1e-9);
                if (dist - prev.distance).abs() <= tie_band {
                    if (area - prev.area).abs() > 1e-9 * area.max(prev.area).max(1.0) {
                        area < prev.area
                    } else {
                        cur_index >= prev.index
                    }
                } else {
                    dist < prev.distance
                }
            }
        };
        if replace {
            best = Some(PickCandidate {
                index: cur_index,
                entity: e,
                context: ctx.clone(),
                bbox: bb,
                distance: dist,
                area,
            });
        }
        true
    };
    let stats = if include_hidden_layers {
        doc.walk_all_layers_while(model, &walk_opts, &mut visit)
    } else {
        doc.walk_while(model, &walk_opts, &mut visit)
    };
    (best, stats.truncated)
}

fn text_of(e: &Entity) -> Option<&str> {
    match &e.kind {
        EntityKind::Text { value, .. } => Some(value),
        EntityKind::MText { plain, value, .. } => {
            Some(if plain.is_empty() { value } else { plain })
        }
        EntityKind::Dimension { text, .. } => text.as_deref(),
        _ => None,
    }
}

pub(crate) fn block_of(e: &Entity) -> Option<&str> {
    match &e.kind {
        EntityKind::Insert { block, .. } => Some(block),
        EntityKind::Dimension { block, .. } => block.as_deref(),
        _ => None,
    }
}

pub(crate) fn hex_color(rgb: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

pub(crate) fn pick_entity_in_doc(
    doc: &Document,
    model: usize,
    wx: f64,
    wy: f64,
    tolerance: f64,
    include_hidden_layers: bool,
    options: &PickOptions,
) -> Option<Value> {
    let (best, _) = pick_candidate_in_doc(
        doc,
        model,
        (wx, wy),
        tolerance,
        include_hidden_layers,
        options,
        PICK_WORK,
    );
    best.map(|hit| {
        let e = hit.entity;
        let attributes: Vec<_> = e
            .attributes
            .iter()
            .map(|a| json!({ "tag": a.tag, "value": a.value }))
            .collect();
        let props = serde_json::to_value(&e.props).unwrap_or_else(|_| json!({}));
        let mut obj = json!({
            "index": hit.index,
            "id": e.id,
            "layer": hit.context.layer_name(e),
            "kind": e.kind.type_name(),
            "color": hex_color(e.resolved_color_in(doc, &hit.context)),
            "bbox": {
                "min": [hit.bbox.min.x, hit.bbox.min.y, hit.bbox.min.z],
                "max": [hit.bbox.max.x, hit.bbox.max.y, hit.bbox.max.z],
            },
            "distance": hit.distance,
            "attributes": attributes,
            "props": props,
        });
        if let Value::Object(map) = &mut obj {
            if let Some(t) = text_of(e) {
                map.insert("text".into(), Value::String(t.to_owned()));
            }
            if let Some(b) = block_of(e) {
                map.insert("block".into(), Value::String(b.to_owned()));
            }
        }
        obj
    })
}

/// `(x, y, tolerance)` from positional arguments.
fn point_args(x: &JsValue, y: &JsValue, tolerance: &JsValue) -> Result<(f64, f64, f64), JsValue> {
    let x = args::finite("x", x)?;
    let y = args::finite("y", y)?;
    let t = args::finite("tolerance", tolerance)?;
    if t < 0.0 {
        return Err(bad_argument("tolerance must not be negative"));
    }
    Ok((x, y, t))
}

#[wasm_bindgen]
impl CadDocument {
    /// The entity of `model` closest to the world point `(x, y)` within `tolerance` world units,
    /// or `null` when nothing is that close. Options (`PickRequest`): `tolerance` (required,
    /// world units), `maxOutputBytes` (JSON size of the result, default and at most 65,536),
    /// `maxWork` (traversal budget, default and at most 5,000,000), `includeHiddenLayers`,
    /// `excludedLayers`, `excludedKinds`.
    ///
    /// The result (`PickHit`) has `path` (resolves with `entity(model, path)` to the entity
    /// that was hit, a block part for geometry drawn from a block) and `modelPath` (the entity
    /// of the model that owns it: the outermost insert for a block part, otherwise `path`),
    /// the kind, layer and id of both, the owner's `block` name and `attributes`, the hit's
    /// `color`, `text`, model-space `bbox` and `distance`. Metadata is capped as in
    /// `pickEntityBounded` and `truncated` says so. An exhausted work or output budget throws
    /// `limit_exceeded`; it never returns a partial search as success.
    #[wasm_bindgen(unchecked_return_type = "PickHit | null")]
    pub fn pick(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] model: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] x: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] y: JsValue,
        #[wasm_bindgen(unchecked_param_type = "PickRequest | string")] options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("pick");
        let m = self.model_index(&model)?;
        let x = args::finite("x", &x)?;
        let y = args::finite("y", &y)?;
        let mut o = Opts::parse("options", Some(&options))?;
        let tolerance = o
            .number("tolerance", 0.0, f64::MAX)?
            .ok_or_else(|| bad_argument("option tolerance is required"))?;
        let max_output = o
            .uint("maxOutputBytes", 64, PICK_OUTPUT)?
            .unwrap_or(PICK_OUTPUT);
        let max_work = o.uint("maxWork", 1, PICK_WORK)?.unwrap_or(PICK_WORK);
        let hidden = o.bool("includeHiddenLayers")?.unwrap_or(false);
        let filters = filters(&mut o)?;
        o.finish(&mut call)?;
        let (hit, exhausted) =
            pick_candidate_in_doc(&self.doc, m, (x, y), tolerance, hidden, &filters, max_work);
        if exhausted {
            return Err(limit_exceeded(
                "maxWork",
                Some(max_work),
                None,
                "pick work budget exceeded",
            ));
        }
        let Some(hit) = hit else {
            return Ok(JsValue::NULL);
        };
        let max_output = max_output as usize;
        let text = bounded_pick::hit_summary(&self.doc, m, &hit, max_output).map_err(|()| {
            limit_exceeded(
                "maxOutputBytes",
                Some(max_output as u64),
                None,
                "pick output budget exceeded",
            )
        })?;
        parse(&text)
    }

    /// Deprecated: use `pick`. Picks the closest visible entity in `model` within `tolerance`
    /// world units of `(wx, wy)`, or `null`. `index` in the result is the hit's position in the
    /// walk's visiting order (block contents and group children included), not an index of
    /// `toJSON().models[model].entities`. Metadata (attributes, props) is copied in full.
    ///
    /// @deprecated Use `pick()`, whose result has entity paths.
    #[wasm_bindgen(js_name = pickEntity, unchecked_return_type = "PickedEntity | null")]
    pub fn pick_entity(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] model: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] wx: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] wy: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] tolerance: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "boolean | null")]
        include_hidden_layers: Option<JsValue>,
        #[wasm_bindgen(unchecked_optional_param_type = "PickOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("pickEntity");
        let m = self.model_index(&model)?;
        let (x, y, t) = point_args(&wx, &wy, &tolerance)?;
        let hidden = args::opt_bool("includeHiddenLayers", include_hidden_layers.as_ref())?;
        let mut o = Opts::parse("options", options.as_ref())?;
        let filters = filters(&mut o)?;
        o.finish(&mut call)?;
        match pick_entity_in_doc(&self.doc, m, x, y, t, hidden.unwrap_or(false), &filters) {
            Some(v) => json_to_js(&v),
            None => Ok(JsValue::NULL),
        }
    }

    /// Deprecated: use `pick`. Picks like `pickEntity` with metadata capped before it is
    /// copied. `options.maxOutputBytes` (required, at most 65,536) bounds the complete UTF-8
    /// JSON result; `maxWork` (at most 5,000,000) the traversal. Exhaustion throws
    /// `limit_exceeded`; `null` means no hit in a completed search. `index` is the walk's
    /// visiting order, as in `pickEntity`.
    ///
    /// @deprecated Use `pick()`, whose result has entity paths.
    #[wasm_bindgen(js_name = pickEntityBounded, unchecked_return_type = "BoundedPickedEntity | null")]
    pub fn pick_entity_bounded(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] model: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] wx: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] wy: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] tolerance: JsValue,
        #[wasm_bindgen(unchecked_param_type = "BoundedPickOptions | string")] options: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "boolean | null")]
        include_hidden_layers: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("pickEntityBounded");
        let m = self.model_index(&model)?;
        let (x, y, t) = point_args(&wx, &wy, &tolerance)?;
        let hidden = args::opt_bool("includeHiddenLayers", include_hidden_layers.as_ref())?;
        let mut o = Opts::parse("options", Some(&options))?;
        let max_output = o
            .uint("maxOutputBytes", 1, PICK_OUTPUT)?
            .ok_or_else(|| bad_argument("option maxOutputBytes is required"))?
            as usize;
        let max_work = o.uint("maxWork", 1, PICK_WORK)?.unwrap_or(PICK_WORK);
        let filters = filters(&mut o)?;
        o.finish(&mut call)?;
        let (hit, exhausted) = pick_candidate_in_doc(
            &self.doc,
            m,
            (x, y),
            t,
            hidden.unwrap_or(false),
            &filters,
            max_work,
        );
        let output_error = || {
            limit_exceeded(
                "maxOutputBytes",
                Some(max_output as u64),
                None,
                "pick output budget exceeded",
            )
        };
        if exhausted {
            return Err(limit_exceeded(
                "maxWork",
                Some(max_work),
                None,
                "pick work budget exceeded",
            ));
        }
        let Some(hit) = hit else {
            if max_output < 4 {
                return Err(output_error());
            }
            return Ok(JsValue::NULL);
        };
        let text =
            bounded_pick::summary(&self.doc, &hit, max_output).map_err(|()| output_error())?;
        parse(&text)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use cadkit::{Layer, Point3};

    #[test]
    fn pick_entity_in_doc_selects_closest_and_respects_hidden_layers() {
        let mut doc = Document::new();
        doc.layers.push({
            let mut layer = Layer::new("FROZEN").with_frozen(true);
            layer.plottable = false;
            layer
        });
        let mut border = Entity::polyline(
            [
                Point3::xy(0.0, 0.0),
                Point3::xy(100.0, 0.0),
                Point3::xy(100.0, 100.0),
                Point3::xy(0.0, 100.0),
            ],
            true,
        )
        .on_layer("0");
        border.id = Some(1);
        let mut inner = Entity::line(Point3::xy(20.0, 30.0), Point3::xy(40.0, 30.0)).on_layer("0");
        inner.id = Some(2);
        let mut frozen_line =
            Entity::line(Point3::xy(20.0, 50.0), Point3::xy(40.0, 50.0)).on_layer("FROZEN");
        frozen_line.id = Some(3);
        doc.model_space_mut()
            .entities
            .extend([border, inner, frozen_line]);

        let hit =
            pick_entity_in_doc(&doc, 0, 30.0, 30.5, 2.0, false, &PickOptions::default()).unwrap();
        assert_eq!(hit["id"], 2);
        assert_eq!(hit["kind"], "line");

        assert!(
            pick_entity_in_doc(&doc, 0, 30.0, 50.0, 1.0, false, &PickOptions::default()).is_none()
        );
        let hidden_hit =
            pick_entity_in_doc(&doc, 0, 30.0, 50.0, 1.0, true, &PickOptions::default()).unwrap();
        assert_eq!(hidden_hit["id"], 3);
        assert_eq!(hidden_hit["layer"], "FROZEN");
        let opts = PickOptions {
            excluded_layers: vec!["FROZEN".into()],
            ..Default::default()
        };
        assert!(pick_entity_in_doc(&doc, 0, 30.0, 50.0, 1.0, true, &opts).is_none());
    }

    #[test]
    fn hit_paths_resolve_to_the_hit_and_its_owner() {
        let mut doc = Document::new();
        let mut part = Entity::line(Point3::xy(0.0, 0.0), Point3::xy(10.0, 0.0));
        part.id = Some(7);
        let mut block = cadkit::Block::new("B");
        block
            .entities
            .push(Entity::circle(Point3::xy(50.0, 50.0), 1.0));
        block.entities.push(part);
        doc.blocks.push(block);
        doc.model_space_mut()
            .entities
            .push(Entity::line(Point3::xy(0.0, 90.0), Point3::xy(1.0, 90.0)));
        let mut insert = Entity::insert("B", Point3::xy(0.0, 20.0));
        insert.id = Some(9);
        doc.model_space_mut().entities.push(insert);
        let (hit, _) = pick_candidate_in_doc(
            &doc,
            0,
            (5.0, 20.0),
            0.5,
            false,
            &PickOptions::default(),
            1000,
        );
        let hit = hit.unwrap();
        let path = hit.context.path();
        assert_eq!(path, vec![1, 1]);
        assert_eq!(hit.context.model_path_len(), 1);
        assert_eq!(doc.entity_at(0, &path).and_then(|e| e.id), Some(7));
        let text = bounded_pick::hit_summary(&doc, 0, &hit, 65_536).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["path"], json!([1, 1]));
        assert_eq!(v["modelPath"], json!([1]));
        assert_eq!(v["id"], 7);
        assert_eq!(v["modelId"], 9);
        assert_eq!(v["modelKind"], "insert");
        assert_eq!(v["block"], "B");
    }
}
