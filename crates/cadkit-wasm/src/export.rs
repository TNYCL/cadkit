//! Writers: JSON is in `document.rs`; here SVG, DXF (with a report of what was left out),
//! DGN V8 and CityGML 2.0.

use cadkit::{DxfVersion, SvgOptions, SvgOrigin};
use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::args::{self, MAX_SAFE_INTEGER};
use crate::document::CadDocument;
use crate::error::{bad_argument, from_cadkit, from_cadkit_with, js_error};
use crate::options::{Call, Opt, Opts, default_write_limits, limits};
use crate::value::{json_to_js, to_js};

fn dxf_version(value: Option<&JsValue>) -> Result<DxfVersion, JsValue> {
    let name = args::opt_string("version", value, 16)?;
    match name.map(|v| v.to_ascii_lowercase()).as_deref() {
        None | Some("r2018") => Ok(DxfVersion::R2018),
        Some("r12") => Ok(DxfVersion::R12),
        Some("r2000") => Ok(DxfVersion::R2000),
        Some("r2004") => Ok(DxfVersion::R2004),
        Some("r2007") => Ok(DxfVersion::R2007),
        Some("r2010") => Ok(DxfVersion::R2010),
        Some("r2013") => Ok(DxfVersion::R2013),
        Some(other) => Err(bad_argument(&format!(
            "unknown DXF version {other:?} (r12, r2000, r2004, r2007, r2010, r2013, r2018)"
        ))),
    }
}

/// SVG options shared by `toSvg` and `toSvgBounded`; returns the options and
/// `includeHiddenLayers`.
fn svg_options(doc: &CadDocument, o: &mut Opts) -> Result<(SvgOptions, bool), JsValue> {
    let mut s = SvgOptions::default();
    if let Some(m) = o.uint("model", 0, MAX_SAFE_INTEGER as u64)? {
        s.model = doc.model_index(&JsValue::from_f64(m as f64))?;
    }
    if let Some(w) = o.number("widthPx", f64::MIN_POSITIVE, 1e7)? {
        s.width_px = w;
    }
    if let Some(w) = o.number("strokePx", 0.0, 1e6)? {
        s.stroke_px = w;
    }
    // `null`, `""` and `"none"` give a transparent background.
    if let Some(bg) = o.nullable_string("background", 256)? {
        s.background = bg.filter(|b| !b.is_empty() && b != "none");
    }
    if let Some(b) = o.bool("text")? {
        s.text = b;
    }
    if let Some(b) = o.bool("images")? {
        s.images = b;
    }
    if let Some(b) = o.bool("expandBlocks")? {
        s.expand_blocks = b;
    }
    if let Some(origin) = o.take("origin")? {
        s.origin = svg_origin(&origin)?;
    }
    if let Some(b) = o.bool("entityRefs")? {
        s.entity_refs = b;
    }
    // The core ignores values out of range; here they are argument errors, so that a typo
    // does not silently fall back to the default rendering.
    if let Some(p) = o.nullable_number("pointPx", f64::MIN_POSITIVE, 1000.0)? {
        s.point_px = p;
    }
    if let Some(r) = o.nullable_number("capHeightRatio", 0.05, 20.0)? {
        s.cap_height_ratio = r;
    }
    if let Some(f) = o.nullable_string("fontFamily", 1024)? {
        s.font_family = f.filter(|f| !f.is_empty());
    }
    if let Some(b) = o.bool("hatchNativeSpacing")? {
        s.hatch_native_spacing = b;
    }
    if let Some(b) = o.bool("layoutViewports")? {
        s.layout_viewports = b;
    }
    let hidden = o.bool("includeHiddenLayers")?.unwrap_or(false);
    Ok((s, hidden))
}

/// `origin`: `"auto"`, `"none"`, `null` (none), `[x, y]` or `{x, y}` in drawing units.
fn svg_origin(value: &Opt) -> Result<SvgOrigin, JsValue> {
    let wrong = || {
        bad_argument(
            "option origin must be \"auto\", \"none\", null, [x, y] or {x, y} with finite numbers",
        )
    };
    let finite = |v: &Opt| match v {
        Opt::Num(n) if n.is_finite() => Some(*n),
        _ => None,
    };
    match value {
        Opt::Null => Ok(SvgOrigin::None),
        Opt::Str(s) if s == "auto" => Ok(SvgOrigin::Auto),
        Opt::Str(s) if s == "none" => Ok(SvgOrigin::None),
        Opt::List(items) => match items.as_slice() {
            [x, y] => match (finite(x), finite(y)) {
                (Some(x), Some(y)) => Ok(SvgOrigin::Explicit { x, y }),
                _ => Err(wrong()),
            },
            _ => Err(wrong()),
        },
        Opt::Obj(entries) => {
            let get = |key: &str| {
                entries
                    .iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| finite(v))
            };
            if entries.len() != 2 {
                return Err(wrong());
            }
            match (get("x"), get("y")) {
                (Some(x), Some(y)) => Ok(SvgOrigin::Explicit { x, y }),
                _ => Err(wrong()),
            }
        }
        _ => Err(wrong()),
    }
}

/// `{ text | bytes, warnings }` as a JS object.
fn with_warnings(
    key: &str,
    value: &JsValue,
    warnings: &[cadkit::Warning],
) -> Result<JsValue, JsValue> {
    let out = Object::new();
    Reflect::set(&out, &key.into(), value)?;
    let list: Vec<serde_json::Value> = warnings
        .iter()
        .map(|w| serde_json::json!({ "code": w.code, "message": w.message, "offset": w.offset }))
        .collect();
    Reflect::set(
        &out,
        &"warnings".into(),
        &json_to_js(&serde_json::Value::Array(list))?,
    )?;
    Ok(out.into())
}

fn citygml_args(srs_name: &JsValue, lod: &JsValue) -> Result<(String, u8), JsValue> {
    let srs = args::string("srsName", srs_name, 4096)?;
    if srs.trim().is_empty() {
        return Err(bad_argument("srsName must name the CRS of the coordinates"));
    }
    let lod = args::index("lod", lod, 4.0)?;
    Ok((srs, u8::try_from(lod).unwrap_or(4)))
}

/// The surface keys of `gml::ExportOptions` (`closedPolylinesAsSurfaces`,
/// `planarityTolerance`, `maxPlanarityDeviation`) and the limit keys, on top of
/// `ExportOptions::new(srs, lod)` with the default write limits.
pub(crate) fn citygml_export_keys(
    o: &mut Opts,
    srs: String,
    lod: u8,
    call: &mut Call,
) -> Result<cadkit::gml::ExportOptions, JsValue> {
    let mut e = cadkit::gml::ExportOptions::new(srs, lod);
    e.limits = limits(o, default_write_limits(), call)?;
    if let Some(b) = o.bool("closedPolylinesAsSurfaces")? {
        e.closed_polylines_as_surfaces = b;
    }
    if let Some(t) = o.number("planarityTolerance", f64::MIN_POSITIVE, f64::MAX)? {
        e.planarity_tolerance = t;
    }
    if let Some(t) = o.number("maxPlanarityDeviation", f64::MIN_POSITIVE, f64::MAX)? {
        e.max_planarity_deviation = t;
    }
    Ok(e)
}

fn citygml_export_options(
    name: &'static str,
    srs_name: &JsValue,
    lod: &JsValue,
    options: Option<&JsValue>,
) -> Result<cadkit::gml::ExportOptions, JsValue> {
    let mut call = Call::new(name);
    let (srs, lod) = citygml_args(srs_name, lod)?;
    let mut o = Opts::parse("options", options)?;
    let e = citygml_export_keys(&mut o, srs, lod, &mut call)?;
    o.finish(&mut call)?;
    Ok(e)
}

#[wasm_bindgen]
impl CadDocument {
    /// Renders one model as SVG. Options (`SvgOptions`): `model`, `widthPx`, `strokePx`,
    /// `background` (CSS color; `null` or `"none"` for transparent), `text`, `images`,
    /// `expandBlocks`, `includeHiddenLayers`, and for viewers `origin` (coordinates relative
    /// to a point, drawing units), `entityRefs` (`data-e` entity paths), `pointPx`,
    /// `capHeightRatio`, `fontFamily`, `hatchNativeSpacing` and `layoutViewports`. Very large
    /// drawings are cut at an internal body size with a marker; use `toSvgBounded` for
    /// untrusted input.
    #[wasm_bindgen(js_name = toSvg)]
    pub fn to_svg(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "SvgOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<String, JsValue> {
        let mut call = Call::new("toSvg");
        let mut o = Opts::parse("options", options.as_ref())?;
        let (s, hidden) = svg_options(self, &mut o)?;
        o.finish(&mut call)?;
        Ok(if hidden {
            cadkit::to_svg_all_layers(&self.doc, &s)
        } else {
            cadkit::to_svg(&self.doc, &s)
        })
    }

    /// Renders a complete SVG within a required UTF-8 byte budget (`maxOutputBytes`, 1 to
    /// 2^32 - 1, all markup included) and an optional traversal budget (`maxWork`). Throws
    /// `limit_exceeded` instead of returning a partial drawing.
    #[wasm_bindgen(js_name = toSvgBounded)]
    pub fn to_svg_bounded(
        &self,
        #[wasm_bindgen(unchecked_param_type = "BoundedSvgOptions | string")] options: JsValue,
    ) -> Result<String, JsValue> {
        let mut call = Call::new("toSvgBounded");
        let mut o = Opts::parse("options", Some(&options))?;
        let max_output = o
            .uint("maxOutputBytes", 1, u64::from(u32::MAX))?
            .ok_or_else(|| bad_argument("option maxOutputBytes is required"))?;
        let max_work = o.uint("maxWork", 1, MAX_SAFE_INTEGER as u64)?;
        let (mut s, hidden) = svg_options(self, &mut o)?;
        o.finish(&mut call)?;
        s.max_output_bytes = usize::try_from(max_output).unwrap_or(usize::MAX);
        if let Some(w) = max_work {
            s.max_work = w;
        }
        cadkit::to_svg_bounded(&self.doc, &s, hidden).map_err(|e| from_cadkit(&e))
    }

    /// Writes the drawing as DXF text. `version`: `"r12"`, `"r2000"`, `"r2004"`, `"r2007"`,
    /// `"r2010"`, `"r2013"` or `"r2018"` (default). What DXF cannot represent is left out
    /// silently; `toDxfWithReport` lists it.
    #[wasm_bindgen(js_name = toDxf)]
    pub fn to_dxf(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "DxfVersionName | null")] version: Option<
            JsValue,
        >,
    ) -> Result<String, JsValue> {
        let v = dxf_version(version.as_ref())?;
        cadkit::to_dxf(&self.doc, v).map_err(|e| from_cadkit(&e))
    }

    /// Like `toDxf`, returning `{text, warnings}`: one warning (`{code, message, offset}`) for
    /// everything that was not written, such as entities DXF has no record for.
    #[wasm_bindgen(js_name = toDxfWithReport, unchecked_return_type = "DxfTextResult")]
    pub fn to_dxf_with_report(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "DxfVersionName | null")] version: Option<
            JsValue,
        >,
    ) -> Result<JsValue, JsValue> {
        let v = dxf_version(version.as_ref())?;
        let (text, warnings) =
            cadkit::dxf::write_with_report(&self.doc, v).map_err(|e| from_cadkit(&e))?;
        with_warnings("text", &JsValue::from_str(&text), &warnings)
    }

    /// Writes the drawing as DXF bytes; `codepage` (such as `"windows-1254"` or `"DOS857"`)
    /// selects a legacy 8-bit encoding for versions before R2007.
    #[wasm_bindgen(js_name = toDxfBytes)]
    pub fn to_dxf_bytes(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "DxfVersionName | null")] version: Option<
            JsValue,
        >,
        #[wasm_bindgen(unchecked_optional_param_type = "string | null")] codepage: Option<JsValue>,
    ) -> Result<Vec<u8>, JsValue> {
        let v = dxf_version(version.as_ref())?;
        let codepage = args::opt_string("codepage", codepage.as_ref(), 64)?;
        let opts = cadkit::dxf::WriteOptions::new(v).with_codepage(codepage);
        cadkit::to_dxf_bytes(&self.doc, &opts).map_err(|e| from_cadkit(&e))
    }

    /// Like `toDxfBytes`, returning `{bytes, warnings}` (see `toDxfWithReport`).
    #[wasm_bindgen(js_name = toDxfBytesWithReport, unchecked_return_type = "DxfBytesResult")]
    pub fn to_dxf_bytes_with_report(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "DxfVersionName | null")] version: Option<
            JsValue,
        >,
        #[wasm_bindgen(unchecked_optional_param_type = "string | null")] codepage: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let v = dxf_version(version.as_ref())?;
        let codepage = args::opt_string("codepage", codepage.as_ref(), 64)?;
        let opts = cadkit::dxf::WriteOptions::new(v).with_codepage(codepage);
        let (bytes, warnings) =
            cadkit::dxf::write_bytes_with_report(&self.doc, &opts).map_err(|e| from_cadkit(&e))?;
        with_warnings(
            "bytes",
            &Uint8Array::from(bytes.as_slice()).into(),
            &warnings,
        )
    }

    /// Writes generic CityGML 2.0 objects (UTF-8 XML bytes) with the CRS name `srsName` of the
    /// existing coordinates and `lod` (0 to 4). No reprojection and no axis swap are done.
    /// Options (`CityGmlExportOptions`): `closedPolylinesAsSurfaces`, `planarityTolerance`,
    /// `maxPlanarityDeviation` and the limit keys.
    #[wasm_bindgen(js_name = toCityGml)]
    pub fn to_citygml(
        &self,
        #[wasm_bindgen(unchecked_param_type = "string")] srs_name: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] lod: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "CityGmlExportOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<Vec<u8>, JsValue> {
        let e = citygml_export_options("toCityGml", &srs_name, &lod, options.as_ref())?;
        cadkit::to_citygml(&self.doc, &e).map_err(|err| from_cadkit_with(&err, &e.limits))
    }

    /// Like `toCityGml`, returning `{xml, report}` as an object. With `metadataOnly` false an
    /// entity without CityGML geometry is an `unsupported` error; with true it is kept as JSON
    /// metadata and listed in `report.issues`. `options` as for `toCityGml`.
    #[wasm_bindgen(js_name = toCityGmlReport, unchecked_return_type = "CityGmlReportResult")]
    pub fn to_citygml_report(
        &self,
        #[wasm_bindgen(unchecked_param_type = "string")] srs_name: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] lod: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "boolean | null")] metadata_only: Option<
            JsValue,
        >,
        #[wasm_bindgen(unchecked_optional_param_type = "CityGmlExportOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let (xml, report) = self.citygml_with_report(
            "toCityGmlReport",
            &srs_name,
            &lod,
            metadata_only.as_ref(),
            options.as_ref(),
        )?;
        let out = Object::new();
        Reflect::set(&out, &"xml".into(), &JsValue::from_str(&xml))?;
        Reflect::set(&out, &"report".into(), &to_js(&report)?)?;
        Ok(out.into())
    }

    /// The result of `toCityGmlReport` as JSON text.
    ///
    /// @deprecated Use `toCityGmlReport()`, which returns an object.
    #[wasm_bindgen(js_name = toCityGmlWithReport)]
    pub fn to_citygml_with_report(
        &self,
        #[wasm_bindgen(unchecked_param_type = "string")] srs_name: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] lod: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "boolean | null")] metadata_only: Option<
            JsValue,
        >,
    ) -> Result<String, JsValue> {
        let (xml, report) = self.citygml_with_report(
            "toCityGmlWithReport",
            &srs_name,
            &lod,
            metadata_only.as_ref(),
            None,
        )?;
        serde_json::to_string(&serde_json::json!({ "xml": xml, "report": report }))
            .map_err(|e| js_error("invalid", &e.to_string()))
    }
}

impl CadDocument {
    fn citygml_with_report(
        &self,
        name: &'static str,
        srs_name: &JsValue,
        lod: &JsValue,
        metadata_only: Option<&JsValue>,
        options: Option<&JsValue>,
    ) -> Result<(String, cadkit::gml::ExportReport), JsValue> {
        let e = citygml_export_options(name, srs_name, lod, options)?;
        let metadata_only = args::opt_bool("metadataOnly", metadata_only)?.unwrap_or(false);
        let limits = e.limits;
        let (bytes, report) = cadkit::gml::write_document_with_report(
            &self.doc,
            &e,
            if metadata_only {
                cadkit::gml::UnsupportedGeometry::MetadataOnly
            } else {
                cadkit::gml::UnsupportedGeometry::Reject
            },
        )
        .map_err(|e| from_cadkit_with(&e, &limits))?;
        // The writer emits UTF-8; a failure here would be a writer bug, reported as invalid.
        let xml = String::from_utf8(bytes).map_err(|e| js_error("invalid", &e.to_string()))?;
        Ok((xml, report))
    }
}
