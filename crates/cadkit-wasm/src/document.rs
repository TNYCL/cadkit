//! The `CadDocument` class: construction, summary, layers, warnings and full JSON.
//! Other methods of the class live in the module of their topic (access, pick, edit, export,
//! citygml); wasm-bindgen merges the `impl` blocks into one JS class.

use cadkit::Document;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::args;
use crate::error::from_cadkit_with;
use crate::options::{Call, read_options};
use crate::value::{json_to_js, parse};

/// A parsed drawing in wasm memory. Call `free()` when done: the memory is not reclaimed by
/// the JS garbage collector before that (and the module's memory never shrinks).
#[wasm_bindgen]
pub struct CadDocument {
    pub(crate) doc: Document,
}

impl CadDocument {
    pub(crate) fn new(doc: Document) -> Self {
        Self { doc }
    }

    pub(crate) fn model_index(&self, value: &JsValue) -> Result<usize, JsValue> {
        args::model(value, self.doc.models.len())
    }
}

#[wasm_bindgen]
impl CadDocument {
    /// Builds a document from its JSON text (the shape of `toJSON()`, docs/schema).
    /// `options` set the limits as for `read` (`keepRaw` and `codepage` have no effect).
    #[wasm_bindgen(js_name = fromJsonString)]
    pub fn from_json_string(
        #[wasm_bindgen(unchecked_param_type = "string")] text: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<CadDocument, JsValue> {
        let mut call = Call::new("CadDocument.fromJsonString");
        let limits = read_options(&mut call, "options", options.as_ref())?.limits;
        let max = u32::try_from(limits.max_input_bytes).unwrap_or(u32::MAX);
        let text = args::string("text", &text, max)?;
        let doc = cadkit::export::document_from_json(&text, &limits)
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        Ok(CadDocument::new(doc))
    }

    /// A deep copy of this document (a second document in wasm memory, about as large as this
    /// one). Free both.
    #[wasm_bindgen(js_name = clone)]
    pub fn clone_document(&self) -> CadDocument {
        CadDocument::new(self.doc.clone())
    }

    /// Summary: format, version, application, units, models with entity counts, layer, block
    /// and warning counts.
    #[wasm_bindgen(unchecked_return_type = "DocumentInfo")]
    pub fn info(&self) -> Result<JsValue, JsValue> {
        let d = &self.doc;
        let models: Vec<Value> = d
            .models
            .iter()
            .enumerate()
            .map(|(i, m)| json!({ "index": i, "name": m.name, "entities": m.entities.len(), "is3d": m.is_3d }))
            .collect();
        json_to_js(&json!({
            "format": serde_json::to_value(d.source.format).unwrap_or(Value::Null),
            "version": d.source.version,
            "application": d.source.application,
            "units": serde_json::to_value(d.units.unit).unwrap_or(Value::Null),
            "metersPerUnit": d.units.meters_per_unit,
            "models": models,
            "layerCount": d.layers.len(),
            "blockCount": d.blocks.len(),
            "warningCount": d.warnings.len(),
        }))
    }

    /// Layers as `{name, color, visible, frozen, locked}`.
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
        json_to_js(&Value::Array(v))
    }

    /// Warnings recorded while reading, as `{code, message, offset}`.
    #[wasm_bindgen(unchecked_return_type = "WarningInfo[]")]
    pub fn warnings(&self) -> Result<JsValue, JsValue> {
        let v: Vec<Value> = self
            .doc
            .warnings
            .iter()
            .map(|w| json!({ "code": w.code, "message": w.message, "offset": w.offset }))
            .collect();
        json_to_js(&Value::Array(v))
    }

    /// The whole document as a JS object (so `JSON.stringify(doc)` works). For large drawings
    /// prefer `entities()`, `entity()` and `stats()`, which copy only what is asked for.
    #[wasm_bindgen(js_name = toJSON, unchecked_return_type = "DocumentJson")]
    pub fn to_json(&self) -> Result<JsValue, JsValue> {
        parse(&self.to_json_text(false)?)
    }

    /// The whole document as JSON text; `pretty` indents it.
    #[wasm_bindgen(js_name = toJsonString)]
    pub fn to_json_string(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "boolean | null")] pretty: Option<JsValue>,
    ) -> Result<String, JsValue> {
        let pretty = args::opt_bool("pretty", pretty.as_ref())?.unwrap_or(false);
        self.to_json_text(pretty)
    }
}

impl CadDocument {
    fn to_json_text(&self, pretty: bool) -> Result<String, JsValue> {
        cadkit::to_json(&self.doc, pretty).map_err(|e| crate::error::from_cadkit(&e))
    }
}
