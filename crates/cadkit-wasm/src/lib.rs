//! WebAssembly bindings for cadkit.
//!
//! Data crosses the boundary as JSON text parsed on the JS side (`JSON.parse` via `js-sys`),
//! not through `serde-wasm-bindgen`: the document model is already `Serialize`, the JSON shape
//! is then identical to `cadkit-cli` / `cadkit-py` output, and one dependency fewer keeps the
//! `.wasm` small. Bulk outputs (SVG, DXF, JSON) are plain strings.
//!
//! Errors are thrown as JS `Error` objects with a `kind` property: `unknown_format`,
//! `unsupported`, `invalid`, `truncated`, `limit_exceeded`, `io` or `bad_argument`.

use cadkit::ops::{DEFAULT_WALK_DEPTH, WalkOptions, entity_bbox, entity_distance_xy};
use cadkit::{BBox, Document, DxfVersion, Entity, EntityKind, LengthUnit, ReadOptions, SvgOptions};
use js_sys::Reflect;
use serde_json::{Map, Value, json};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TS_DEFINITIONS: &'static str = r#"
export type FormatName = "dwg" | "dxf" | "dgn_v7" | "dgn_v8" | "city_gml";
export type DxfVersionName = "r12" | "r2000" | "r2004" | "r2007" | "r2010" | "r2013" | "r2018";

export interface ReadOptions {
  keepRaw?: boolean;
  codepage?: string;
  maxInputBytes?: number;
  maxDecompressedBytes?: number;
  maxObjects?: number;
  maxDepth?: number;
}

export interface SvgOptions {
  model?: number;
  widthPx?: number;
  strokePx?: number;
  background?: string | null;
  text?: boolean;
  images?: boolean;
  expandBlocks?: boolean;
  includeHiddenLayers?: boolean;
}

export interface Summary {
  format: FormatName | "unknown";
  version: string;
  application: string | null;
  codepage: string | null;
  units: { unit: string; meters_per_unit: number | null } | null;
  is_3d: boolean | null;
}

export interface ModelInfo {
  index: number;
  name: string;
  entities: number;
  is3d: boolean;
}

export interface DocumentInfo {
  format: FormatName | "unknown";
  version: string;
  application: string | null;
  units: string;
  metersPerUnit: number | null;
  models: ModelInfo[];
  layerCount: number;
  blockCount: number;
  warningCount: number;
}

export interface LayerInfo {
  name: string;
  color: { type: string; index?: number; r?: number; g?: number; b?: number };
  visible: boolean;
  frozen: boolean;
  locked: boolean;
}

export interface WarningInfo {
  code: string;
  message: string;
  offset: number | null;
}

export interface PickOptions {
  excludedLayers?: string[];
  excludedKinds?: string[];
}

export interface PickedEntity {
  index: number;
  id: number | null;
  layer: string;
  kind: string;
  color: string;
  bbox: { min: [number, number, number]; max: [number, number, number] };
  distance: number;
  text?: string;
  block?: string;
  attributes: Array<{ tag: string; value: unknown }>;
  props: Record<string, unknown>;
}

export interface DgnWriteOptions {
  clear_seed_model?: boolean;
  clearSeedModel?: boolean;
  codepage?: string | null;
  planarity_tolerance?: number;
  planarityTolerance?: number;
  limits?: Record<string, number>;
}

export interface GmlValidationOptions {
  geometry?: boolean;
  planarity_tolerance?: number;
  planarityTolerance?: number;
  limits?: Record<string, number>;
}

export interface CityGmlReportResult {
  xml: string;
  report: {
    exported_entities: number;
    metadata_only_entities: number;
    issues: Array<{ path: number[]; code: string; message: string }>;
  };
}
"#;

fn js_error(kind: &str, message: &str) -> JsValue {
    let err = js_sys::Error::new(message);
    // Setting a property on a fresh Error object cannot fail in practice; ignore the Result.
    let _ = Reflect::set(&err, &JsValue::from_str("kind"), &JsValue::from_str(kind));
    err.into()
}

fn from_cadkit(e: &cadkit::Error) -> JsValue {
    let kind = match e {
        cadkit::Error::Truncated { .. } => "truncated",
        cadkit::Error::Invalid { .. } => "invalid",
        cadkit::Error::Unsupported(_) => "unsupported",
        cadkit::Error::LimitExceeded(_) => "limit_exceeded",
        cadkit::Error::UnknownFormat => "unknown_format",
        cadkit::Error::Io(_) => "io",
        _ => "invalid",
    };
    js_error(kind, &e.to_string())
}

fn parse_json(text: &str) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(text)
}

fn value_to_js(v: &Value) -> Result<JsValue, JsValue> {
    parse_json(&v.to_string())
}

/// Property `key` of `obj`, `None` when `obj` is not an object or the value is null/undefined.
fn get(obj: &JsValue, key: &str) -> Option<JsValue> {
    if !obj.is_object() {
        return None;
    }
    Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
}

fn format_name(f: cadkit::Format) -> Value {
    serde_json::to_value(f).unwrap_or(Value::Null)
}

fn camel_to_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn normalize_option_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut normalized = Map::with_capacity(map.len());
            for (k, v) in map {
                let snake = camel_to_snake(&k);
                let key = match snake.as_str() {
                    "codepage" => "codepage".to_owned(),
                    _ => snake,
                };
                normalized.insert(key, normalize_option_keys(v));
            }
            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_option_keys).collect()),
        other => other,
    }
}

/// Parses an optional JS argument that may be `undefined`, `null`, a JSON string, or a plain JS
/// object (accepting either `snake_case` or `camelCase` keys).
fn decode_js_options<T: serde::de::DeserializeOwned + Default>(
    options: Option<JsValue>,
) -> Result<T, JsValue> {
    let Some(js) = options.filter(|v| !v.is_undefined() && !v.is_null()) else {
        return Ok(T::default());
    };
    let text = if let Some(s) = js.as_string() {
        if s.trim().is_empty() {
            return Ok(T::default());
        }
        s
    } else if js.is_object() {
        js_sys::JSON::stringify(&js)
            .map_err(|_| js_error("bad_argument", "options object could not be serialized"))?
            .as_string()
            .unwrap_or_default()
    } else {
        return Err(js_error(
            "bad_argument",
            "options must be an object or a JSON string",
        ));
    };
    let raw: Value =
        serde_json::from_str(&text).map_err(|e| js_error("bad_argument", &e.to_string()))?;
    serde_json::from_value(normalize_option_keys(raw))
        .map_err(|e| js_error("bad_argument", &e.to_string()))
}

fn parse_read_options(options: Option<JsValue>) -> Result<ReadOptions, JsValue> {
    let mut opts = ReadOptions::default();
    // wasm32 has a 4 GiB address space and allocation failure traps (panic=abort), so the
    // desktop defaults are too generous here.
    opts.limits.max_input_bytes = 512 << 20;
    opts.limits.max_decompressed_bytes = 1 << 30;
    opts.limits.max_objects = 5_000_000;
    let Some(o) = options.filter(|v| !v.is_undefined() && !v.is_null()) else {
        return Ok(opts);
    };
    if let Some(s) = o.as_string() {
        if !s.trim().is_empty() {
            let raw: Value =
                serde_json::from_str(&s).map_err(|e| js_error("bad_argument", &e.to_string()))?;
            let mut normalized = normalize_option_keys(raw);
            if let Value::Object(map) = &mut normalized
                && let Some(cp) = map.remove("codepage")
            {
                map.entry("fallback_codepage").or_insert(cp);
            }
            let parsed: ReadOptions = serde_json::from_value(normalized)
                .map_err(|e| js_error("bad_argument", &e.to_string()))?;
            opts = parsed;
            opts.limits = wasm_limits(opts.limits);
        }
        return Ok(opts);
    }
    if let Some(n) = get(&o, "maxInputBytes")
        .or_else(|| get(&o, "max_input_bytes"))
        .and_then(|v| v.as_f64())
    {
        opts.limits.max_input_bytes = n as u64;
    }
    if let Some(n) = get(&o, "maxDecompressedBytes")
        .or_else(|| get(&o, "max_decompressed_bytes"))
        .and_then(|v| v.as_f64())
    {
        opts.limits.max_decompressed_bytes = n as u64;
    }
    if let Some(n) = get(&o, "maxObjects")
        .or_else(|| get(&o, "max_objects"))
        .and_then(|v| v.as_f64())
    {
        opts.limits.max_objects = n as u64;
    }
    if let Some(n) = get(&o, "maxDepth")
        .or_else(|| get(&o, "max_depth"))
        .and_then(|v| v.as_f64())
    {
        opts.limits.max_depth = n as u32;
    }
    if let Some(v) = get(&o, "keepRaw").or_else(|| get(&o, "keep_raw")) {
        opts.keep_raw = v.is_truthy();
    }
    if let Some(v) = get(&o, "codepage").or_else(|| get(&o, "fallback_codepage")) {
        opts.fallback_codepage = v.as_string();
    }
    if let Some(lim) = get(&o, "limits") {
        if let Some(n) = get(&lim, "max_input_bytes")
            .or_else(|| get(&lim, "maxInputBytes"))
            .and_then(|v| v.as_f64())
        {
            opts.limits.max_input_bytes = n as u64;
        }
        if let Some(n) = get(&lim, "max_decompressed_bytes")
            .or_else(|| get(&lim, "maxDecompressedBytes"))
            .and_then(|v| v.as_f64())
        {
            opts.limits.max_decompressed_bytes = n as u64;
        }
        if let Some(n) = get(&lim, "max_objects")
            .or_else(|| get(&lim, "maxObjects"))
            .and_then(|v| v.as_f64())
        {
            opts.limits.max_objects = n as u64;
        }
        if let Some(n) = get(&lim, "max_depth")
            .or_else(|| get(&lim, "maxDepth"))
            .and_then(|v| v.as_f64())
        {
            opts.limits.max_depth = n as u32;
        }
    }
    Ok(opts)
}

fn parse_dxf_version(version: Option<&str>) -> Result<DxfVersion, JsValue> {
    match version.map(str::to_ascii_lowercase).as_deref() {
        None | Some("r2018") => Ok(DxfVersion::R2018),
        Some("r12") => Ok(DxfVersion::R12),
        Some("r2000") => Ok(DxfVersion::R2000),
        Some("r2004") => Ok(DxfVersion::R2004),
        Some("r2007") => Ok(DxfVersion::R2007),
        Some("r2010") => Ok(DxfVersion::R2010),
        Some("r2013") => Ok(DxfVersion::R2013),
        Some(other) => Err(js_error(
            "bad_argument",
            &format!("unknown DXF version {other:?}"),
        )),
    }
}

/// Detects the drawing format from the file header.
///
/// Returns `"dwg"`, `"dxf"`, `"dgn_v7"`, `"dgn_v8"`, or `"city_gml"`, or `undefined` when unrecognised.
#[wasm_bindgen]
pub fn detect(bytes: &[u8]) -> Option<String> {
    cadkit::detect(bytes).and_then(|f| format_name(f).as_str().map(str::to_owned))
}

/// Inspects a drawing header without decoding full entity streams.
///
/// `options` may be an object (`ReadOptions`) or a JSON string.
#[wasm_bindgen(unchecked_return_type = "Summary")]
pub fn inspect(
    bytes: &[u8],
    #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")] options: Option<
        JsValue,
    >,
) -> Result<JsValue, JsValue> {
    let opts = parse_read_options(options)?;
    let summary = cadkit::inspect_with(bytes, &opts).map_err(|e| from_cadkit(&e))?;
    let val = serde_json::to_value(&summary).map_err(|e| js_error("invalid", &e.to_string()))?;
    value_to_js(&val)
}

/// Reads a drawing. `options` may contain `keepRaw`, `codepage`, `maxInputBytes`,
/// `maxDecompressedBytes`, `maxObjects` and `maxDepth` (either as a JS object or JSON string).
/// Defaults are conservative for wasm32: 512 MiB input, 1 GiB decompressed, 5 million objects, depth 64.
#[wasm_bindgen]
pub fn read(
    bytes: &[u8],
    #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")] options: Option<
        JsValue,
    >,
) -> Result<CadDocument, JsValue> {
    let opts = parse_read_options(options)?;
    let doc = cadkit::read_with(bytes, &opts).map_err(|e| from_cadkit(&e))?;
    Ok(CadDocument { doc })
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct PickOptions {
    excluded_layers: Vec<String>,
    excluded_kinds: Vec<String>,
}

struct PickCandidate {
    index: usize,
    id: Option<u64>,
    layer: String,
    kind: &'static str,
    rgb: (u8, u8, u8),
    bbox: BBox,
    distance: f64,
    area: f64,
    text: Option<String>,
    block: Option<String>,
    attributes: Vec<Value>,
    props: Value,
}

fn pick_entity_in_doc(
    doc: &Document,
    model: usize,
    wx: f64,
    wy: f64,
    tolerance: f64,
    include_hidden_layers: bool,
    options: &PickOptions,
) -> Option<Value> {
    if !(wx.is_finite() && wy.is_finite() && tolerance.is_finite() && tolerance >= 0.0) {
        return None;
    }
    let walk_opts = WalkOptions::default()
        .with_max_depth(DEFAULT_WALK_DEPTH)
        .with_expand_inserts(true)
        .with_max_work(5_000_000);
    let mut visited_index = 0usize;
    let mut best: Option<PickCandidate> = None;
    let mut visit = |e: &Entity, ctx: &cadkit::ops::WalkContext<'_>| -> bool {
        let cur_index = visited_index;
        visited_index += 1;
        if options
            .excluded_layers
            .iter()
            .any(|l| l == ctx.layer_name(e))
            || options
                .excluded_kinds
                .iter()
                .any(|k| k == e.kind.type_name())
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
            let text = match &e.kind {
                EntityKind::Text { value, .. } => Some(value.clone()),
                EntityKind::MText { plain, value, .. } => Some(if plain.is_empty() {
                    value.clone()
                } else {
                    plain.clone()
                }),
                EntityKind::Dimension { text, .. } => text.clone(),
                _ => None,
            };
            let block = match &e.kind {
                EntityKind::Insert { block, .. } => Some(block.clone()),
                EntityKind::Dimension { block, .. } => block.clone(),
                _ => None,
            };
            let attributes = e
                .attributes
                .iter()
                .map(|a| json!({ "tag": a.tag, "value": a.value }))
                .collect();
            let props = serde_json::to_value(&e.props).unwrap_or_else(|_| json!({}));
            best = Some(PickCandidate {
                index: cur_index,
                id: e.id,
                layer: ctx.layer_name(e).to_owned(),
                kind: e.kind.type_name(),
                rgb: e.resolved_color_in(doc, ctx),
                bbox: bb,
                distance: dist,
                area,
                text,
                block,
                attributes,
                props,
            });
        }
        true
    };
    if include_hidden_layers {
        doc.walk_all_layers_while(model, &walk_opts, &mut visit);
    } else {
        doc.walk_while(model, &walk_opts, &mut visit);
    }
    best.map(|hit| {
        let mut obj = json!({
            "index": hit.index,
            "id": hit.id,
            "layer": hit.layer,
            "kind": hit.kind,
            "color": format!("#{:02x}{:02x}{:02x}", hit.rgb.0, hit.rgb.1, hit.rgb.2),
            "bbox": {
                "min": [hit.bbox.min.x, hit.bbox.min.y, hit.bbox.min.z],
                "max": [hit.bbox.max.x, hit.bbox.max.y, hit.bbox.max.z],
            },
            "distance": hit.distance,
            "attributes": hit.attributes,
            "props": hit.props,
        });
        if let Value::Object(map) = &mut obj {
            if let Some(t) = hit.text {
                map.insert("text".into(), Value::String(t));
            }
            if let Some(b) = hit.block {
                map.insert("block".into(), Value::String(b));
            }
        }
        obj
    })
}

/// A parsed drawing. Call `free()` when done to release its memory.
#[wasm_bindgen]
pub struct CadDocument {
    doc: Document,
}

#[wasm_bindgen]
impl CadDocument {
    /// Constructs a format-neutral document from its JSON string representation.
    #[wasm_bindgen(js_name = fromJsonString)]
    pub fn from_json_string(text: &str) -> Result<CadDocument, JsValue> {
        let doc = cadkit::export::document_from_json(text, &wasm_limits(cadkit::Limits::default()))
            .map_err(|e| from_cadkit(&e))?;
        Ok(CadDocument { doc })
    }

    /// Reads CityGML 2.0 XML bytes directly into a format-neutral `CadDocument`, optionally
    /// filtering geometry to `lod`. `options` may be a JS `ReadOptions` object or JSON string.
    #[wasm_bindgen(js_name = fromCityGml)]
    pub fn from_citygml(
        bytes: &[u8],
        lod: Option<u8>,
        options: Option<JsValue>,
    ) -> Result<CadDocument, JsValue> {
        let mut opts = parse_read_options(options)?;
        opts.limits = wasm_limits(opts.limits);
        let native = cadkit::gml::read_native(bytes, &opts).map_err(|e| from_cadkit(&e))?;
        let doc = cadkit::gml::to_document(
            &native,
            &cadkit::gml::ImportOptions::default().with_lod(lod),
            &opts,
        )
        .map_err(|e| from_cadkit(&e))?;
        Ok(CadDocument { doc })
    }

    /// Expands `Insert` block references in-place across all models into world-space entities.
    /// Returns `{ truncated, work }`.
    #[wasm_bindgen(js_name = explodeInserts)]
    pub fn explode_inserts(&mut self) -> Result<JsValue, JsValue> {
        let stats = self.doc.explode_inserts(DEFAULT_WALK_DEPTH);
        value_to_js(&json!({
            "truncated": stats.truncated,
            "work": stats.work,
        }))
    }

    /// Converts drawing coordinates, block definitions, linetype dash patterns, and fixed text
    /// heights in-place to `unit` (e.g. `"mm"`, `"meter"`, `"ft"`), returning the scale factor.
    #[wasm_bindgen(js_name = convertUnits)]
    pub fn convert_units(&mut self, unit: &str) -> Result<f64, JsValue> {
        let target = LengthUnit::from_name(unit)
            .ok_or_else(|| js_error("bad_argument", &format!("unknown length unit {unit:?}")))?;
        self.doc.convert_units(target).map_err(|e| from_cadkit(&e))
    }

    /// Writes DGN V8 bytes using `seed`; `options` may be a JS object or a JSON string following
    /// the Rust `dgn::WriteOptions` contract.
    #[wasm_bindgen(js_name = toDgn)]
    pub fn to_dgn(
        &self,
        seed: &[u8],
        #[wasm_bindgen(unchecked_optional_param_type = "DgnWriteOptions | string | null")] options: Option<JsValue>,
    ) -> Result<Vec<u8>, JsValue> {
        let mut opts: cadkit::dgn::WriteOptions = decode_js_options(options)?;
        opts.limits = wasm_limits(opts.limits);
        cadkit::to_dgn(&self.doc, seed, &opts).map_err(|e| from_cadkit(&e))
    }

    /// Exports generic CityGML 2.0 geometry with an explicit CRS and LoD without axis or CRS transformation.
    #[wasm_bindgen(js_name = toCityGml)]
    pub fn to_citygml(&self, srs_name: String, lod: u8) -> Result<Vec<u8>, JsValue> {
        cadkit::to_citygml(
            &self.doc,
            &cadkit::gml::ExportOptions::new(srs_name, lod)
                .with_limits(wasm_limits(cadkit::Limits::default())),
        )
        .map_err(|e| from_cadkit(&e))
    }

    /// Returns JSON with XML and diagnostics; metadata-only fallback must be explicit.
    #[wasm_bindgen(js_name = toCityGmlWithReport)]
    pub fn to_citygml_with_report(
        &self,
        srs_name: String,
        lod: u8,
        metadata_only: bool,
    ) -> Result<String, JsValue> {
        let (bytes, report) = cadkit::gml::write_document_with_report(
            &self.doc,
            &cadkit::gml::ExportOptions::new(srs_name, lod)
                .with_limits(wasm_limits(cadkit::Limits::default())),
            if metadata_only {
                cadkit::gml::UnsupportedGeometry::MetadataOnly
            } else {
                cadkit::gml::UnsupportedGeometry::Reject
            },
        )
        .map_err(|e| from_cadkit(&e))?;
        let xml = String::from_utf8(bytes).map_err(|e| js_error("export", &e.to_string()))?;
        serde_json::to_string(&serde_json::json!({"xml": xml, "report": report}))
            .map_err(|e| js_error("export", &e.to_string()))
    }

    /// Summary: format, version, application, units, models with entity counts, layer and
    /// warning counts.
    #[wasm_bindgen(unchecked_return_type = "DocumentInfo")]
    pub fn info(&self) -> Result<JsValue, JsValue> {
        let d = &self.doc;
        let models: Vec<Value> = d
            .models
            .iter()
            .enumerate()
            .map(|(i, m)| json!({ "index": i, "name": m.name, "entities": m.entities.len(), "is3d": m.is_3d }))
            .collect();
        let v = json!({
            "format": format_name(d.source.format),
            "version": d.source.version,
            "application": d.source.application,
            "units": serde_json::to_value(d.units.unit).unwrap_or(Value::Null),
            "metersPerUnit": d.units.meters_per_unit,
            "models": models,
            "layerCount": d.layers.len(),
            "blockCount": d.blocks.len(),
            "warningCount": d.warnings.len(),
        });
        value_to_js(&v)
    }

    /// Layers as an array of `{name, color, visible, frozen, locked}`.
    #[wasm_bindgen(unchecked_return_type = "LayerInfo[]")]
    pub fn layers(&self) -> Result<JsValue, JsValue> {
        let v: Vec<Value> = self
            .doc
            .layers
            .iter()
            .map(|l| {
                json!({
                    "name": l.name,
                    "color": serde_json::to_value(l.color).unwrap_or(Value::Null),
                    "visible": l.visible,
                    "frozen": l.frozen,
                    "locked": l.locked,
                })
            })
            .collect();
        value_to_js(&Value::Array(v))
    }

    /// Warnings as an array of `{code, message, offset}`.
    #[wasm_bindgen(unchecked_return_type = "WarningInfo[]")]
    pub fn warnings(&self) -> Result<JsValue, JsValue> {
        let v: Vec<Value> = self
            .doc
            .warnings
            .iter()
            .map(|w| json!({ "code": w.code, "message": w.message, "offset": w.offset }))
            .collect();
        value_to_js(&Value::Array(v))
    }

    /// Picks the closest visible entity in `model` within `tolerance` world units of `(wx, wy)`,
    /// returning a compact summary object or `null` when nothing hits.
    #[wasm_bindgen(js_name = pickEntity, unchecked_return_type = "PickedEntity | null")]
    pub fn pick_entity(
        &self,
        model: usize,
        wx: f64,
        wy: f64,
        tolerance: f64,
        #[wasm_bindgen(unchecked_optional_param_type = "boolean | null")]
        include_hidden_layers: Option<bool>,
        #[wasm_bindgen(unchecked_optional_param_type = "PickOptions | null")] options: Option<
            JsValue,
        >,
    ) -> Result<JsValue, JsValue> {
        if model >= self.doc.models.len() && !self.doc.models.is_empty() {
            return Err(js_error("bad_argument", "model index out of range"));
        }
        if !(wx.is_finite() && wy.is_finite() && tolerance.is_finite() && tolerance >= 0.0) {
            return Err(js_error(
                "bad_argument",
                "wx, wy and tolerance must be finite (tolerance >= 0)",
            ));
        }
        match pick_entity_in_doc(
            &self.doc,
            model,
            wx,
            wy,
            tolerance,
            include_hidden_layers.unwrap_or(false),
            &decode_js_options(options)?,
        ) {
            Some(v) => value_to_js(&v),
            None => Ok(JsValue::NULL),
        }
    }

    /// The full document as a JS object (so `JSON.stringify(doc)` works).
    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> Result<JsValue, JsValue> {
        parse_json(&self.to_json_string(false)?)
    }

    /// The full document as JSON text.
    #[wasm_bindgen(js_name = toJsonString)]
    pub fn to_json_string(&self, pretty: bool) -> Result<String, JsValue> {
        cadkit::to_json(&self.doc, pretty).map_err(|e| from_cadkit(&e))
    }

    /// Renders one model as SVG. Options: `model`, `widthPx`, `strokePx`, `background`
    /// (CSS color, or `null`/`"none"` for transparent), `text`, `images`, `expandBlocks`,
    /// `includeHiddenLayers`.
    #[wasm_bindgen(js_name = toSvg)]
    pub fn to_svg(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "SvgOptions | null")] options: Option<
            JsValue,
        >,
    ) -> Result<String, JsValue> {
        let mut o = SvgOptions::default();
        let mut include_hidden_layers = false;
        if let Some(js) = options {
            if let Some(v) = get(&js, "model") {
                match v.as_f64() {
                    Some(n) if n >= 0.0 && n.fract() == 0.0 => o.model = n as usize,
                    _ => {
                        return Err(js_error(
                            "bad_argument",
                            "model must be a non-negative integer",
                        ));
                    }
                }
            }
            if let Some(n) = get(&js, "widthPx").and_then(|v| v.as_f64()) {
                o.width_px = n;
            }
            if let Some(n) = get(&js, "strokePx").and_then(|v| v.as_f64()) {
                o.stroke_px = n;
            }
            if js.is_object()
                && let Ok(v) = Reflect::get(&js, &JsValue::from_str("background"))
            {
                if v.is_null() {
                    o.background = None;
                } else if let Some(s) = v.as_string() {
                    o.background = if s.is_empty() || s == "none" {
                        None
                    } else {
                        Some(s)
                    };
                }
            }
            if let Some(v) = get(&js, "text") {
                o.text = v.is_truthy();
            }
            if let Some(v) = get(&js, "images") {
                o.images = v.is_truthy();
            }
            if let Some(v) = get(&js, "expandBlocks") {
                o.expand_blocks = v.is_truthy();
            }
            if let Some(v) = get(&js, "includeHiddenLayers") {
                include_hidden_layers = v.is_truthy();
            }
        }
        if o.model >= self.doc.models.len() && !self.doc.models.is_empty() {
            return Err(js_error("bad_argument", "model index out of range"));
        }
        Ok(if include_hidden_layers {
            cadkit::to_svg_all_layers(&self.doc, &o)
        } else {
            cadkit::to_svg(&self.doc, &o)
        })
    }

    /// Writes the drawing as ASCII DXF. `version` is `"r12"`, `"r2000"`, `"r2004"`, `"r2007"`,
    /// `"r2010"`, `"r2013"` or `"r2018"` (default).
    #[wasm_bindgen(js_name = toDxf)]
    pub fn to_dxf(&self, version: Option<String>) -> Result<String, JsValue> {
        let v = parse_dxf_version(version.as_deref())?;
        cadkit::to_dxf(&self.doc, v).map_err(|e| from_cadkit(&e))
    }

    /// Writes the drawing as DXF bytes with an optional legacy 8-bit `codepage` (such as
    /// `"windows-1254"` or `"DOS857"`).
    #[wasm_bindgen(js_name = toDxfBytes)]
    pub fn to_dxf_bytes(
        &self,
        version: Option<String>,
        codepage: Option<String>,
    ) -> Result<Vec<u8>, JsValue> {
        let v = parse_dxf_version(version.as_deref())?;
        let opts = cadkit::dxf::WriteOptions::new(v).with_codepage(codepage);
        cadkit::to_dxf_bytes(&self.doc, &opts).map_err(|e| from_cadkit(&e))
    }
}

/// Native CityGML 2.0 document preserving the object graph, IDs, references, and XML extensions.
#[wasm_bindgen]
pub struct CityGmlDocument {
    doc: cadkit::gml::CityGmlDocument,
}

/// Reads CityGML 2.0 bytes into a lossless native `CityGmlDocument`; `options` may be a JS
/// object or a JSON string following the Rust `ReadOptions` contract.
#[wasm_bindgen(js_name = readCityGml)]
pub fn read_citygml(
    bytes: &[u8],
    #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")] options: Option<
        JsValue,
    >,
) -> Result<CityGmlDocument, JsValue> {
    let mut opts = parse_read_options(options)?;
    opts.limits = wasm_limits(opts.limits);
    Ok(CityGmlDocument {
        doc: cadkit::gml::read_native(bytes, &opts).map_err(|e| from_cadkit(&e))?,
    })
}

#[wasm_bindgen]
impl CityGmlDocument {
    /// Local TKGM checks; requires city-model-tender or digital-building-registration.
    /// Never establishes official acceptance.
    #[wasm_bindgen(js_name = preflightTkgm)]
    pub fn preflight_tkgm(&self, profile: &str) -> Result<JsValue, JsValue> {
        let profile = profile
            .parse::<cadkit::gml::tkgm::Profile>()
            .map_err(|e| from_cadkit(&e))?;
        let report = cadkit::gml::tkgm::preflight(
            &self.doc,
            profile,
            &wasm_limits(cadkit::Limits::default()),
        )
        .map_err(|e| from_cadkit(&e))?;
        let text =
            serde_json::to_string(&report).map_err(|e| js_error("invalid", &e.to_string()))?;
        parse_json(&text)
    }
    /// Constructs and validates a typed native `CityGmlDocument` from its JSON string.
    #[wasm_bindgen(js_name = fromJsonString)]
    pub fn from_json_string(text: &str) -> Result<CityGmlDocument, JsValue> {
        let doc = cadkit::gml::read_native_json(
            text,
            &ReadOptions::default().with_limits(wasm_limits(cadkit::Limits::default())),
        )
        .map_err(|e| from_cadkit(&e))?;
        Ok(CityGmlDocument { doc })
    }
    /// Serializes the lossless native model to an editable JSON string.
    #[wasm_bindgen(js_name = toJsonString)]
    pub fn to_json_string(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.doc).map_err(|e| js_error("invalid", &e.to_string()))
    }
    /// Serializes UTF-8 CityGML 2.0 XML bytes; `options` may be a JS object or JSON string
    /// following the Rust `gml::ValidationOptions` contract.
    #[wasm_bindgen(js_name = toGml)]
    pub fn to_gml(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "GmlValidationOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<Vec<u8>, JsValue> {
        let mut opts: cadkit::gml::ValidationOptions = decode_js_options(options)?;
        opts.limits = wasm_limits(opts.limits);
        cadkit::gml::write(&self.doc, &opts).map_err(|e| from_cadkit(&e))
    }
    /// Projects native CityGML geometry into a neutral `CadDocument`, optionally filtered to `lod`.
    #[wasm_bindgen(js_name = toDocument)]
    pub fn to_document(&self, lod: Option<u8>) -> Result<CadDocument, JsValue> {
        Ok(CadDocument {
            doc: cadkit::gml::to_document(
                &self.doc,
                &cadkit::gml::ImportOptions::default().with_lod(lod),
                &ReadOptions::default().with_limits(wasm_limits(cadkit::Limits::default())),
            )
            .map_err(|e| from_cadkit(&e))?,
        })
    }
    /// Runs structural and geometric validation checks and returns summary counters (`ValidationOptions` object or JSON).
    pub fn validate(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "GmlValidationOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut opts: cadkit::gml::ValidationOptions = decode_js_options(options)?;
        opts.limits = wasm_limits(opts.limits);
        let report = cadkit::gml::validate(&self.doc, &opts).map_err(|e| from_cadkit(&e))?;
        let text =
            serde_json::to_string(&report).map_err(|e| js_error("invalid", &e.to_string()))?;
        parse_json(&text)
    }
}

// Cap limits to conservative ceilings so desktop defaults cannot trigger large allocations in wasm32's 4 GiB address space.
fn wasm_limits(mut limits: cadkit::Limits) -> cadkit::Limits {
    limits.max_input_bytes = limits.max_input_bytes.min(512 << 20);
    limits.max_decompressed_bytes = limits.max_decompressed_bytes.min(1 << 30);
    limits.max_objects = limits.max_objects.min(5_000_000);
    limits.max_depth = limits.max_depth.min(64);
    limits
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use cadkit::{Layer, Point3};

    #[test]
    fn camel_case_options_normalize_to_snake_case() {
        let raw = json!({
            "clearSeedModel": true,
            "planarityTolerance": 0.05,
            "limits": { "maxObjects": 42 }
        });
        let norm = normalize_option_keys(raw);
        let opts: cadkit::dgn::WriteOptions = serde_json::from_value(norm).unwrap();
        assert!(opts.clear_seed_model);
        assert!((opts.planarity_tolerance - 0.05).abs() < 1e-12);
        assert_eq!(opts.limits.max_objects, 42);
    }

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
}
