//! CityGML buildings from the surfaces of a model: `toCityGmlBuildings` (XML and report) and
//! `toCityGmlBuildingsDocument` (the native document and the report).

use cadkit::gml::{BuildingGrouping, BuildingOptions, BuildingReport};
use js_sys::{Object, Reflect, Uint8Array};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::args::MAX_SAFE_INTEGER;
use crate::citygml::CityGmlDocument;
use crate::document::CadDocument;
use crate::error::{bad_argument, from_cadkit_with};
use crate::export::citygml_export_keys;
use crate::options::{Call, Opts};

/// The options of both methods; returns the source model and the building options.
fn building_options(
    doc: &CadDocument,
    options: &JsValue,
    call: &mut Call,
) -> Result<(usize, BuildingOptions), JsValue> {
    let mut o = Opts::parse("options", Some(options))?.strict_keys();
    let srs = o
        .string("srsName", 4096)?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            bad_argument("option srsName is required and must name the CRS of the coordinates")
        })?;
    // CityGML 2.0 has boundary surfaces (walls, roofs, grounds) only from LoD 2.
    let lod = o
        .uint("lod", 2, 4)?
        .ok_or_else(|| bad_argument("option lod is required (2, 3 or 4)"))?;
    let model = match o.uint("model", 0, MAX_SAFE_INTEGER as u64)? {
        Some(m) => doc.model_index(&JsValue::from_f64(m as f64))?,
        None => 0,
    };
    let export = citygml_export_keys(&mut o, srs, u8::try_from(lod).unwrap_or(2), call)?;
    let mut b = BuildingOptions::new(export);
    match o.string("grouping", 16)?.as_deref() {
        None => {}
        Some("single") => b.grouping = BuildingGrouping::Single,
        Some("layer") => b.grouping = BuildingGrouping::Layer,
        Some("top_level") => b.grouping = BuildingGrouping::TopLevel,
        Some("connected") => b.grouping = BuildingGrouping::Connected,
        Some(other) => {
            return Err(bad_argument(&format!(
                "option grouping must be \"single\", \"layer\", \"top_level\" or \"connected\", got {other:?}"
            )));
        }
    }
    if let Some(t) = o.number("wallMinTiltDegrees", 0.0, 90.0)? {
        b.wall_min_tilt_degrees = t;
    }
    if let Some(t) = o.number("horizontalMaxTiltDegrees", 0.0, 90.0)? {
        b.horizontal_max_tilt_degrees = t;
    }
    if b.horizontal_max_tilt_degrees >= b.wall_min_tilt_degrees {
        return Err(bad_argument(&format!(
            "option horizontalMaxTiltDegrees ({}) must be below wallMinTiltDegrees ({})",
            b.horizontal_max_tilt_degrees, b.wall_min_tilt_degrees
        )));
    }
    if let Some(t) = o.number("groundTolerance", 0.0, f64::MAX)? {
        b.ground_tolerance = t;
    }
    if let Some(t) = o.number("weldTolerance", 0.0, f64::MAX)? {
        b.weld_tolerance = t;
    }
    if let Some(w) = o.uint("maxWork", 1, MAX_SAFE_INTEGER as u64)? {
        b.max_work = w;
    }
    o.finish(call)?;
    Ok((model, b))
}

/// The report with camelCase keys; `path` and `modelPath` address entities as
/// `entity(model, path)` takes them.
fn report_json(r: &BuildingReport) -> Value {
    let buildings: Vec<Value> = r
        .buildings
        .iter()
        .map(|b| {
            json!({
                "id": b.id,
                "layer": b.layer,
                "topLevel": b.top_level,
                "wallSurfaces": b.wall_surfaces,
                "roofSurfaces": b.roof_surfaces,
                "groundSurfaces": b.ground_surfaces,
                "unclassifiedFaces": b.unclassified_faces,
            })
        })
        .collect();
    let issues: Vec<Value> = r
        .issues
        .iter()
        .map(|i| {
            json!({
                "code": i.code,
                "path": i.path,
                "modelPath": i.path.get(..i.model_path_len).unwrap_or(&i.path),
                "meshFace": i.mesh_face,
                "entityId": i.entity_id,
                "layer": i.layer,
                "entityType": if i.entity_type == "mtext" { "m_text" } else { i.entity_type.as_str() },
                "buildingId": i.building_id,
                "polygonId": i.polygon_id,
            })
        })
        .collect();
    json!({
        "model": r.model,
        "buildings": buildings,
        "wallSurfaces": r.wall_surfaces,
        "roofSurfaces": r.roof_surfaces,
        "groundSurfaces": r.ground_surfaces,
        "unclassifiedFaces": r.unclassified_faces,
        "skippedFaces": r.skipped_faces,
        "skippedByCode": r.skipped_by_code,
        "ignoredEntities": r.ignored_entities,
        "closedPolylinesIgnored": r.closed_polylines_ignored,
        "issues": issues,
        "issuesTruncated": r.issues_truncated,
    })
}

fn result(key: &str, value: &JsValue, report: &BuildingReport) -> Result<JsValue, JsValue> {
    let out = Object::new();
    Reflect::set(&out, &key.into(), value)?;
    Reflect::set(
        &out,
        &"report".into(),
        &crate::value::json_to_js(&report_json(report))?,
    )?;
    Ok(out.into())
}

#[wasm_bindgen]
impl CadDocument {
    /// CityGML 2.0 `bldg:Building`s built from the faces of one model, as `{xml, report}`
    /// (`xml`: UTF-8 bytes, checked with geometry validation before they are returned).
    /// Options (`CityGmlBuildingOptions`): `srsName` and `lod` (2 to 4) are required;
    /// `model`, `grouping`, `wallMinTiltDegrees`, `horizontalMaxTiltDegrees`,
    /// `groundTolerance`, `weldTolerance`, `maxWork`, `closedPolylinesAsSurfaces`,
    /// `planarityTolerance`, `maxPlanarityDeviation` and the limit keys. Unknown option keys
    /// are errors. Faces are classified by tilt only: these are local geometric rules, not a
    /// check against any registry's acceptance criteria.
    #[wasm_bindgen(js_name = toCityGmlBuildings, unchecked_return_type = "CityGmlBuildingsResult")]
    pub fn to_citygml_buildings(
        &self,
        #[wasm_bindgen(unchecked_param_type = "CityGmlBuildingOptions | string")] options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("toCityGmlBuildings");
        let (model, b) = building_options(self, &options, &mut call)?;
        let limits = b.export.limits;
        let (xml, report) = cadkit::gml::write_buildings(&self.doc, model, &b)
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        result("xml", &Uint8Array::from(xml.as_slice()).into(), &report)
    }

    /// Like `toCityGmlBuildings`, returning `{document, report}` with the buildings as a
    /// native `CityGmlDocument` (not validated; call its `validate()`). Free the document.
    #[wasm_bindgen(
        js_name = toCityGmlBuildingsDocument,
        unchecked_return_type = "CityGmlBuildingsDocumentResult"
    )]
    pub fn to_citygml_buildings_document(
        &self,
        #[wasm_bindgen(unchecked_param_type = "CityGmlBuildingOptions | string")] options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let mut call = Call::new("toCityGmlBuildingsDocument");
        let (model, b) = building_options(self, &options, &mut call)?;
        let limits = b.export.limits;
        let (doc, report) = cadkit::gml::buildings_from_document(&self.doc, model, &b)
            .map_err(|e| from_cadkit_with(&e, &limits))?;
        result("document", &CityGmlDocument::new(doc).into(), &report)
    }
}
