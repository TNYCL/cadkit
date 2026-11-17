//! DGN V8 writing: `toDgn`, `toDgnWithReport` and `prepareForDgn`.
//!
//! A document that did not come from the seed's own file usually needs
//! [`cadkit::dgn::V8Seed::prepare_document`] first (units, 2D/3D, block references,
//! symbology): the `prepare` option of the writers does it on a copy, `prepareForDgn` returns
//! that copy as a document of its own.

use cadkit::dgn::{DimensionPolicy, SeedPrepareOptions, V8Seed, WriteOptions};
use cadkit::{Document, ModelKind, ReadOptions, Warning};
use js_sys::{Object, Reflect, Uint8Array};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::args::{self, MAX_SAFE_INTEGER};
use crate::document::CadDocument;
use crate::error::{bad_argument, from_cadkit_with};
use crate::options::{BoolOr, Call, Opts, default_write_limits, limits};
use crate::value::json_to_js;

/// `dimensionPolicy`: `"strict"`, `"promote_2d_to_3d"` or `"project_to_seed"`.
fn dimension_policy(o: &mut Opts) -> Result<Option<DimensionPolicy>, JsValue> {
    match o.string("dimensionPolicy", 32)?.as_deref() {
        None => Ok(None),
        Some("strict") => Ok(Some(DimensionPolicy::Strict)),
        Some("promote_2d_to_3d") => Ok(Some(DimensionPolicy::Promote2dTo3d)),
        Some("project_to_seed") => Ok(Some(DimensionPolicy::ProjectToSeed)),
        Some(other) => Err(bad_argument(&format!(
            "option dimensionPolicy must be \"strict\", \"promote_2d_to_3d\" or \"project_to_seed\", got {other:?}"
        ))),
    }
}

/// The keys of a preparation: `dimensionPolicy`, `explodeInserts`, `convertUnits`,
/// `normalizeForDgn`.
fn prepare_keys(o: &mut Opts, p: &mut SeedPrepareOptions) -> Result<(), JsValue> {
    if let Some(d) = dimension_policy(o)? {
        p.dimension_policy = d;
    }
    if let Some(b) = o.bool("explodeInserts")? {
        p.explode_inserts = b;
    }
    if let Some(b) = o.bool("convertUnits")? {
        p.convert_units = b;
    }
    if let Some(b) = o.bool("normalizeForDgn")? {
        p.normalize_for_dgn = b;
    }
    Ok(())
}

/// `modelIndex`, checked against the document.
fn model_index(doc: &CadDocument, o: &mut Opts) -> Result<Option<usize>, JsValue> {
    let Some(m) = o.uint("modelIndex", 0, MAX_SAFE_INTEGER as u64)? else {
        return Ok(None);
    };
    let count = doc.doc.models.len();
    match usize::try_from(m) {
        Ok(m) if m < count => Ok(Some(m)),
        _ => Err(bad_argument(&format!(
            "option modelIndex {m} is out of range (the document has {count} models)"
        ))),
    }
}

/// Read options of the seed: the write limits and the write code page.
fn seed_read_options(write: &WriteOptions) -> ReadOptions {
    let mut read = ReadOptions::default()
        .with_limits(write.limits)
        .with_keep_raw(true);
    read.fallback_codepage = write.codepage.clone();
    read
}

/// What a writer call asked for.
struct Request {
    write: WriteOptions,
    prepare: Option<SeedPrepareOptions>,
}

fn request(doc: &CadDocument, o: &mut Opts, call: &mut Call) -> Result<Request, JsValue> {
    let mut w = WriteOptions::default();
    w.limits = limits(o, default_write_limits(), call)?;
    if let Some(b) = o.bool("clearSeedModel")? {
        w.clear_seed_model = b;
    }
    if let Some(cp) = o.nullable_string("codepage", 64)? {
        w.codepage = cp;
    }
    if let Some(t) = o.number("timestampMs", 0.0, 1e15)? {
        w.timestamp_ms = t;
    }
    if let Some(t) = o.number("planarityTolerance", 0.0, f64::MAX)? {
        w.planarity_tolerance = t;
    }
    if let Some(b) = o.bool("preserveSeedRasters")? {
        w.preserve_seed_rasters = b;
    }
    if let Some(b) = o.bool("preserveSeedControls")? {
        w.preserve_seed_controls = b;
    }
    w.model_index = model_index(doc, o)?;
    let policy = dimension_policy(o)?;
    if let Some(d) = policy {
        w.dimension_policy = d;
    }
    if let Some(b) = o.bool("estimateTextLength")? {
        w.estimate_text_length = b;
    }
    // The preparation selects the same model and, unless `prepare` names its own, follows
    // the top-level dimension policy.
    let base = || {
        let mut p = SeedPrepareOptions::new();
        p.model_index = w.model_index;
        if let Some(d) = policy {
            p.dimension_policy = d;
        }
        p
    };
    let prepare = match o.bool_or_object("prepare")? {
        None | Some(BoolOr::Bool(false)) => None,
        Some(BoolOr::Bool(true)) => Some(base()),
        Some(BoolOr::Object(mut nested)) => {
            let mut p = base();
            prepare_keys(&mut nested, &mut p)?;
            nested.finish(call)?;
            Some(p)
        }
    };
    Ok(Request { write: w, prepare })
}

/// The written bytes, the source model written and the warnings.
struct Written {
    bytes: Vec<u8>,
    model_index: usize,
    warnings: Vec<Warning>,
}

impl CadDocument {
    fn write_dgn(
        &self,
        seed: &JsValue,
        options: Option<&JsValue>,
        call: &mut Call,
        strict: bool,
    ) -> Result<Written, JsValue> {
        let mut o = Opts::parse("options", options)?;
        if strict {
            o = o.strict_keys();
        }
        let req = request(self, &mut o, call)?;
        o.finish(call)?;
        let limits = req.write.limits;
        let seed = args::bytes("seed", seed, limits.max_input_bytes)?;
        let parsed = V8Seed::parse(&seed, &seed_read_options(&req.write))
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        drop(seed);
        let Some(prepare) = req.prepare else {
            let r = cadkit::dgn::write_v8_with_seed_report(&self.doc, &parsed, &req.write)
                .map_err(|e| from_cadkit_with(&e, &limits))?;
            return Ok(Written {
                bytes: r.bytes,
                model_index: r.model_index,
                warnings: r.warnings,
            });
        };
        let prepared = parsed
            .prepare_document(&self.doc, &prepare)
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        let source = prepare
            .model_index
            .unwrap_or_else(|| first_design_model(&self.doc));
        let mut write = req.write.clone();
        // The prepared copy holds the one model that was selected.
        write.model_index = None;
        let r = cadkit::dgn::write_v8_with_seed_report(&prepared, &parsed, &write)
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        let mut warnings = not_written(&self.doc, source);
        warnings.extend(r.warnings);
        Ok(Written {
            bytes: r.bytes,
            model_index: source,
            warnings,
        })
    }
}

fn first_design_model(doc: &Document) -> usize {
    doc.models
        .iter()
        .position(|m| m.kind == ModelKind::Model)
        .unwrap_or(0)
}

/// `dgn.model_not_written` for every model of `doc` but `written`, as the writer reports
/// them: the preparation keeps only the selected model, so the writer cannot see the others.
fn not_written(doc: &Document, written: usize) -> Vec<Warning> {
    doc.models
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != written)
        .map(|(i, m)| {
            let kind = if m.kind == ModelKind::Model {
                "model"
            } else {
                "layout"
            };
            Warning::new(
                "dgn.model_not_written",
                format!(
                    "{kind} {i} ({:?}, {} top-level entities) was not written",
                    m.name,
                    m.entities.len()
                ),
            )
            .with_object(i as u64)
        })
        .collect()
}

#[wasm_bindgen]
impl CadDocument {
    /// Writes one model as DGN V8 bytes into a copy of `seed` (a single-model DGN V8 file).
    /// Options (`DgnWriteOptions`): `clearSeedModel`, `codepage`, `timestampMs`,
    /// `planarityTolerance`, `preserveSeedRasters`, `preserveSeedControls`, `modelIndex`,
    /// `dimensionPolicy`, `estimateTextLength`, `prepare` and the limit keys. Without
    /// `modelIndex` the only model is written, or the only design model beside paper layouts.
    #[wasm_bindgen(js_name = toDgn)]
    pub fn to_dgn(
        &self,
        #[wasm_bindgen(unchecked_param_type = "BytesLike")] seed: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "DgnWriteOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<Vec<u8>, JsValue> {
        let mut call = Call::new("toDgn");
        Ok(self
            .write_dgn(&seed, options.as_ref(), &mut call, false)?
            .bytes)
    }

    /// Like `toDgn`, returning `{bytes, modelIndex, warnings}`: the source model written and
    /// one warning `{code, message, offset, object}` per model left out
    /// (`dgn.model_not_written`, `object`: its index), per dimension change
    /// (`dgn.model_dimension_changed`) and for estimated text lengths
    /// (`dgn.text_length_estimated`). Unknown option keys are errors.
    #[wasm_bindgen(js_name = toDgnWithReport, unchecked_return_type = "DgnWriteResult")]
    pub fn to_dgn_with_report(
        &self,
        #[wasm_bindgen(unchecked_param_type = "BytesLike")] seed: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "DgnWriteOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("toDgnWithReport");
        let w = self.write_dgn(&seed, options.as_ref(), &mut call, true)?;
        let out = Object::new();
        Reflect::set(
            &out,
            &"bytes".into(),
            &Uint8Array::from(w.bytes.as_slice()).into(),
        )?;
        Reflect::set(
            &out,
            &"modelIndex".into(),
            &JsValue::from_f64(w.model_index as f64),
        )?;
        let warnings: Vec<Value> = w
            .warnings
            .iter()
            .map(|w| json!({ "code": w.code, "message": w.message, "offset": w.offset, "object": w.object }))
            .collect();
        Reflect::set(
            &out,
            &"warnings".into(),
            &json_to_js(&Value::Array(warnings))?,
        )?;
        Ok(out.into())
    }

    /// A new document: one model of this one prepared for `seed` (converted to the seed's
    /// master units and dimension, inserts exploded, symbology normalized for DGN V8), as
    /// `toDgn` writes it with `prepare`. Options (`DgnPrepareOptions`): `modelIndex`,
    /// `dimensionPolicy`, `explodeInserts`, `convertUnits`, `normalizeForDgn`, `codepage`
    /// (of the seed) and the limit keys. Unknown option keys are errors. Free the result.
    #[wasm_bindgen(js_name = prepareForDgn)]
    pub fn prepare_for_dgn(
        &self,
        #[wasm_bindgen(unchecked_param_type = "BytesLike")] seed: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "DgnPrepareOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<CadDocument, JsValue> {
        let mut call = Call::new("prepareForDgn");
        let mut o = Opts::parse("options", options.as_ref())?.strict_keys();
        let mut w = WriteOptions::default();
        w.limits = limits(&mut o, default_write_limits(), &mut call)?;
        if let Some(cp) = o.nullable_string("codepage", 64)? {
            w.codepage = cp;
        }
        let mut p = SeedPrepareOptions::new();
        p.model_index = model_index(self, &mut o)?;
        prepare_keys(&mut o, &mut p)?;
        o.finish(&mut call)?;
        let limits = w.limits;
        let seed = args::bytes("seed", &seed, limits.max_input_bytes)?;
        let parsed = V8Seed::parse(&seed, &seed_read_options(&w))
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        let prepared = parsed
            .prepare_document(&self.doc, &p)
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        Ok(CadDocument::new(prepared))
    }
}
