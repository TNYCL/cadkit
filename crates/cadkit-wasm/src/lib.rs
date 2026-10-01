//! WebAssembly bindings for cadkit.
//!
//! Data crosses the boundary as JSON text parsed on the JS side (`JSON.parse` via `js-sys`),
//! not through `serde-wasm-bindgen`: the document model is already `Serialize`, the JSON shape
//! is then identical to `cadkit-cli` / `cadkit-py` output, and one dependency fewer keeps the
//! `.wasm` small. Bulk outputs (SVG, DXF, JSON) are plain strings.
//!
//! Errors are thrown as JS `Error` objects with a `kind` property: `unknown_format`,
//! `unsupported`, `invalid`, `truncated`, `limit_exceeded`, `io` or `bad_argument`.

use cadkit::{Document, DxfVersion, ReadOptions, SvgOptions};
use js_sys::Reflect;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

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

/// Detects the drawing format from the file header.
///
/// Returns `"dwg"`, `"dxf"`, `"dgn_v7"` or `"dgn_v8"`, or `undefined` when unrecognised.
#[wasm_bindgen]
pub fn detect(bytes: &[u8]) -> Option<String> {
    cadkit::detect(bytes).and_then(|f| format_name(f).as_str().map(str::to_owned))
}

/// Reads a drawing. `options` may contain `keepRaw`, `codepage`, `maxInputBytes`,
/// `maxDecompressedBytes`, `maxObjects` and `maxDepth`. Defaults are conservative for wasm32:
/// 512 MiB input, 1 GiB decompressed, 5 million objects, depth 64.
#[wasm_bindgen]
pub fn read(bytes: &[u8], options: Option<JsValue>) -> Result<CadDocument, JsValue> {
    let mut opts = ReadOptions::default();
    // wasm32 has a 4 GiB address space and allocation failure traps (panic=abort), so the
    // desktop defaults are too generous here.
    opts.limits.max_input_bytes = 512 << 20;
    opts.limits.max_decompressed_bytes = 1 << 30;
    opts.limits.max_objects = 5_000_000;
    if let Some(o) = options {
        if let Some(n) = get(&o, "maxInputBytes").and_then(|v| v.as_f64()) {
            opts.limits.max_input_bytes = n as u64;
        }
        if let Some(n) = get(&o, "maxDecompressedBytes").and_then(|v| v.as_f64()) {
            opts.limits.max_decompressed_bytes = n as u64;
        }
        if let Some(n) = get(&o, "maxObjects").and_then(|v| v.as_f64()) {
            opts.limits.max_objects = n as u64;
        }
        if let Some(n) = get(&o, "maxDepth").and_then(|v| v.as_f64()) {
            opts.limits.max_depth = n as u32;
        }
        if let Some(v) = get(&o, "keepRaw") {
            opts.keep_raw = v.is_truthy();
        }
        if let Some(v) = get(&o, "codepage") {
            opts.fallback_codepage = v.as_string();
        }
    }
    let doc = cadkit::read_with(bytes, &opts).map_err(|e| from_cadkit(&e))?;
    Ok(CadDocument { doc })
}

/// A parsed drawing. Call `free()` when done to release its memory.
#[wasm_bindgen]
pub struct CadDocument {
    doc: Document,
}

#[wasm_bindgen]
impl CadDocument {
    /// Summary: format, version, application, units, models with entity counts, layer and
    /// warning counts.
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
    pub fn warnings(&self) -> Result<JsValue, JsValue> {
        let v: Vec<Value> = self
            .doc
            .warnings
            .iter()
            .map(|w| json!({ "code": w.code, "message": w.message, "offset": w.offset }))
            .collect();
        value_to_js(&Value::Array(v))
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
    /// (CSS color, or `null`/`"none"` for transparent), `text`, `expandBlocks`.
    #[wasm_bindgen(js_name = toSvg)]
    pub fn to_svg(&self, options: Option<JsValue>) -> Result<String, JsValue> {
        let mut o = SvgOptions::default();
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
            if js.is_object() {
                if let Ok(v) = Reflect::get(&js, &JsValue::from_str("background")) {
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
        }
        if o.model >= self.doc.models.len() && !self.doc.models.is_empty() {
            return Err(js_error("bad_argument", "model index out of range"));
        }
        Ok(cadkit::to_svg(&self.doc, &o))
    }

    /// Writes the drawing as ASCII DXF. `version` is `"r12"`, `"r2000"`, `"r2004"`, `"r2007"`,
    /// `"r2010"`, `"r2013"` or `"r2018"` (default).
    #[wasm_bindgen(js_name = toDxf)]
    pub fn to_dxf(&self, version: Option<String>) -> Result<String, JsValue> {
        let v = match version.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("r2018") => DxfVersion::R2018,
            Some("r12") => DxfVersion::R12,
            Some("r2000") => DxfVersion::R2000,
            Some("r2004") => DxfVersion::R2004,
            Some("r2007") => DxfVersion::R2007,
            Some("r2010") => DxfVersion::R2010,
            Some("r2013") => DxfVersion::R2013,
            Some(other) => {
                return Err(js_error(
                    "bad_argument",
                    &format!("unknown DXF version {other:?}"),
                ));
            }
        };
        cadkit::to_dxf(&self.doc, v).map_err(|e| from_cadkit(&e))
    }
}
