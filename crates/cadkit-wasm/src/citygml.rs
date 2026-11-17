//! CityGML: the lossless native `CityGmlDocument` class and its conversion to a
//! `CadDocument`.

use wasm_bindgen::prelude::*;

use crate::args;
use crate::document::CadDocument;
use crate::error::{bad_argument, from_cadkit_with, js_error};
use crate::options::{Call, Opts, default_write_limits, limits, read_options};
use crate::value::to_js;

/// Native CityGML 2.0 document that keeps the object graph, ids, references and XML
/// extensions. Call `free()` when done.
#[wasm_bindgen]
pub struct CityGmlDocument {
    doc: cadkit::gml::CityGmlDocument,
}

impl CityGmlDocument {
    pub(crate) fn new(doc: cadkit::gml::CityGmlDocument) -> Self {
        Self { doc }
    }
}

fn profile_arg(profile: &JsValue) -> Result<cadkit::gml::tkgm::Profile, JsValue> {
    let name = args::string("profile", profile, 64)?;
    match name.as_str() {
        "city-model-tender" | "digital-building-registration" => name
            .parse::<cadkit::gml::tkgm::Profile>()
            .map_err(|e| crate::error::from_cadkit(&e)),
        _ => Err(bad_argument(
            "profile must be \"city-model-tender\" or \"digital-building-registration\"",
        )),
    }
}

/// Reads CityGML 2.0 bytes into a lossless native `CityGmlDocument`. `options` are read
/// options as for `read`.
#[wasm_bindgen(js_name = readCityGml)]
pub fn read_citygml(
    #[wasm_bindgen(unchecked_param_type = "BytesLike")] bytes: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")] options: Option<
        JsValue,
    >,
) -> Result<CityGmlDocument, JsValue> {
    let mut call = Call::new("readCityGml");
    let opts = read_options(&mut call, "options", options.as_ref())?;
    let bytes = args::bytes("bytes", &bytes, opts.limits.max_input_bytes)?;
    Ok(CityGmlDocument {
        doc: cadkit::gml::read_native(&bytes, &opts)
            .map_err(|e| from_cadkit_with(&e, &opts.limits))?,
    })
}

/// `gml::ValidationOptions`: `geometry`, `tolerance`, `planarityTolerance`,
/// `maxGeometryWork` and the limit keys.
fn validation_options(
    call: &mut Call,
    value: Option<&JsValue>,
) -> Result<cadkit::gml::ValidationOptions, JsValue> {
    let mut o = Opts::parse("options", value)?;
    let mut v = cadkit::gml::ValidationOptions::default();
    v.limits = limits(&mut o, default_write_limits(), call)?;
    if let Some(b) = o.bool("geometry")? {
        v.geometry = b;
    }
    if let Some(t) = o.number("tolerance", 0.0, f64::MAX)? {
        v.tolerance = t;
    }
    if let Some(t) = o.number("planarityTolerance", 0.0, f64::MAX)? {
        v.planarity_tolerance = t;
    }
    if let Some(w) = o.uint("maxGeometryWork", 0, args::MAX_SAFE_INTEGER as u64)? {
        v.max_geometry_work = w;
    }
    o.finish(call)?;
    Ok(v)
}

fn lod_arg(value: Option<&JsValue>) -> Result<Option<u8>, JsValue> {
    Ok(args::opt_index("lod", value, 4.0)?.map(|l| u8::try_from(l).unwrap_or(4)))
}

#[wasm_bindgen]
impl CityGmlDocument {
    /// Local TKGM structural checks for `profile` (`"city-model-tender"` or
    /// `"digital-building-registration"`). Never establishes official acceptance
    /// (`acceptance_verified` is always `false`).
    #[wasm_bindgen(js_name = preflightTkgm, unchecked_return_type = "TkgmReport")]
    pub fn preflight_tkgm(
        &self,
        #[wasm_bindgen(unchecked_param_type = "TkgmProfile")] profile: JsValue,
    ) -> Result<JsValue, JsValue> {
        let profile = profile_arg(&profile)?;
        let limits = default_write_limits();
        let report = cadkit::gml::tkgm::preflight(&self.doc, profile, &limits)
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        to_js(&report)
    }

    /// `preflightTkgm` with `id` and `building_id` (the `gml:id`s of the element and of its
    /// building) on every issue, so a caller can point at the object. These ids come from the
    /// file and may encode client data (parcel or registry numbers): keep such reports with
    /// the file's owner. Local checks only; `acceptance_verified` is always `false`.
    #[wasm_bindgen(js_name = preflightTkgmLocated, unchecked_return_type = "TkgmReport")]
    pub fn preflight_tkgm_located(
        &self,
        #[wasm_bindgen(unchecked_param_type = "TkgmProfile")] profile: JsValue,
    ) -> Result<JsValue, JsValue> {
        let profile = profile_arg(&profile)?;
        let limits = default_write_limits();
        let report = cadkit::gml::tkgm::preflight_located(&self.doc, profile, &limits)
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        to_js(&report)
    }

    /// Polygon and solid-shell geometry issues after structural validation, each with the
    /// `gml:id`s that locate it: `[{offset, code, message, element, id, objectId,
    /// buildingId}]` (`GmlGeometryIssue`). `options` as for `validate` (`geometry` has no
    /// effect). The ids come from the file and may encode client data.
    #[wasm_bindgen(js_name = geometryIssues, unchecked_return_type = "GmlGeometryIssue[]")]
    pub fn geometry_issues(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "GmlValidationOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("CityGmlDocument.geometryIssues");
        let opts = validation_options(&mut call, options.as_ref())?;
        let issues = cadkit::gml::located_geometry_issues(&self.doc, &opts)
            .map_err(|e| from_cadkit_with(&e, &opts.limits))?;
        let list: Vec<serde_json::Value> = issues
            .iter()
            .map(|i| {
                serde_json::json!({
                    "offset": i.offset,
                    "code": i.code,
                    "message": i.message,
                    "element": i.element,
                    "id": i.id,
                    "objectId": i.object_id,
                    "buildingId": i.building_id,
                })
            })
            .collect();
        crate::value::json_to_js(&serde_json::Value::Array(list))
    }

    /// Builds and validates a native document from its JSON text (`toJsonString()`).
    #[wasm_bindgen(js_name = fromJsonString)]
    pub fn from_json_string(
        #[wasm_bindgen(unchecked_param_type = "string")] text: JsValue,
    ) -> Result<CityGmlDocument, JsValue> {
        let read = cadkit::ReadOptions::default().with_limits(default_write_limits());
        let max = u32::try_from(read.limits.max_input_bytes).unwrap_or(u32::MAX);
        let text = args::string("text", &text, max)?;
        let doc = cadkit::gml::read_native_json(&text, &read)
            .map_err(|e| from_cadkit_with(&e, &read.limits))?;
        Ok(CityGmlDocument { doc })
    }

    /// The lossless native model as editable JSON text.
    #[wasm_bindgen(js_name = toJsonString)]
    pub fn to_json_string(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.doc).map_err(|e| js_error("invalid", &e.to_string()))
    }

    /// CityGML 2.0 XML as UTF-8 bytes. `options` (`GmlValidationOptions`) control the checks
    /// run before writing.
    #[wasm_bindgen(js_name = toGml)]
    pub fn to_gml(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "GmlValidationOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<Vec<u8>, JsValue> {
        let mut call = Call::new("CityGmlDocument.toGml");
        let opts = validation_options(&mut call, options.as_ref())?;
        cadkit::gml::write(&self.doc, &opts).map_err(|e| from_cadkit_with(&e, &opts.limits))
    }

    /// Projects the native geometry into a new `CadDocument`, optionally only `lod` (0 to 4).
    #[wasm_bindgen(js_name = toDocument)]
    pub fn to_document(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "number | null")] lod: Option<JsValue>,
    ) -> Result<CadDocument, JsValue> {
        let lod = lod_arg(lod.as_ref())?;
        let read = cadkit::ReadOptions::default().with_limits(default_write_limits());
        let doc = cadkit::gml::to_document(
            &self.doc,
            &cadkit::gml::ImportOptions::default().with_lod(lod),
            &read,
        )
        .map_err(|e| from_cadkit_with(&e, &read.limits))?;
        Ok(CadDocument::new(doc))
    }

    /// Structural and geometric checks; returns counters (`GmlValidationReport`). Options as for
    /// `toGml`.
    #[wasm_bindgen(unchecked_return_type = "GmlValidationReport")]
    pub fn validate(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "GmlValidationOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("CityGmlDocument.validate");
        let opts = validation_options(&mut call, options.as_ref())?;
        let report = cadkit::gml::validate(&self.doc, &opts)
            .map_err(|e| from_cadkit_with(&e, &opts.limits))?;
        to_js(&report)
    }
}

#[wasm_bindgen]
impl CadDocument {
    /// Reads CityGML 2.0 bytes straight into a `CadDocument`, optionally only `lod` (0 to 4).
    /// `options` are read options as for `read`.
    #[wasm_bindgen(js_name = fromCityGml)]
    pub fn from_citygml(
        #[wasm_bindgen(unchecked_param_type = "BytesLike")] bytes: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "number | null")] lod: Option<JsValue>,
        #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<CadDocument, JsValue> {
        let mut call = Call::new("CadDocument.fromCityGml");
        let lod = lod_arg(lod.as_ref())?;
        let opts = read_options(&mut call, "options", options.as_ref())?;
        let bytes = args::bytes("bytes", &bytes, opts.limits.max_input_bytes)?;
        let native = cadkit::gml::read_native(&bytes, &opts)
            .map_err(|e| from_cadkit_with(&e, &opts.limits))?;
        let doc = cadkit::gml::to_document(
            &native,
            &cadkit::gml::ImportOptions::default().with_lod(lod),
            &opts,
        )
        .map_err(|e| from_cadkit_with(&e, &opts.limits))?;
        Ok(CadDocument::new(doc))
    }
}
