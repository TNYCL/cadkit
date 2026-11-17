//! Renderer buffers of one model: `surfaceMesh()` (triangles of faces, with outlines and
//! wires as segments) and `lineBuffer()` (every curve as segments), built by cadkit-scene
//! and held in wasm memory by the `SurfaceMesh` and `LineBuffer` classes.
//!
//! The buffers are handed to JS as typed arrays. Every accessor copies (one copy into the JS
//! heap, none inside wasm memory), so a renderer reads each buffer once and keeps the copy;
//! `free()` releases the wasm side. The item table is columnar (typed arrays plus one path
//! pool) so that a mesh with a million faces does not create a million JS objects.

use std::collections::BTreeMap;

use cadkit::ops::{DEFAULT_MAX_WORK, DEFAULT_WALK_DEPTH, WalkOptions};
use cadkit_scene::document::{
    LineOptions, PolylineSurfaces, SurfaceOptions, line_buffer, surface_mesh,
};
use cadkit_scene::{LayerRange, SceneError, SceneItems};
use js_sys::{Array, Float32Array, Object, Reflect, Uint8Array, Uint32Array};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::args::{self, MAX_SAFE_INTEGER};
use crate::document::CadDocument;
use crate::error::{Details, bad_argument, error_with};
use crate::options::{Call, Opt, Opts};
use crate::value::json_to_js;

/// Highest triangle and segment budgets. In the finished mesh a triangle of an isolated face
/// costs 97 bytes of wasm memory (three vertices of 27 bytes, 16 bytes of indices and item),
/// a segment 34 and an item about 28, so 4,000,000 of each hold at most about 0.65 GB. The
/// build needs about a fifth more at its peak (measured: 1,000,000 triangles and 2,000,000
/// segments of 500,000 quadrilaterals hold 154 MB and grew the module by 185 MB).
pub(crate) const MAX_TRIANGLES: u64 = 4_000_000;
pub(crate) const MAX_SEGMENTS: u64 = 4_000_000;
/// The crate defaults, which these bindings keep.
const DEFAULT_TRIANGLES: u64 = 2_000_000;
const DEFAULT_SEGMENTS: u64 = 2_000_000;
/// Longest hidden-layer list accepted by `pick`.
const MAX_HIDDEN_LAYERS: u32 = 65_536;

/// The walk options shared by both builders: `maxDepth`, `expandInserts`, `maxWork`.
fn walk_options(o: &mut Opts) -> Result<WalkOptions, JsValue> {
    let mut walk = WalkOptions::new();
    walk.max_depth = o
        .uint_capped("maxDepth", 64)?
        .map_or(DEFAULT_WALK_DEPTH, |d| u32::try_from(d).unwrap_or(64));
    if let Some(b) = o.bool("expandInserts")? {
        walk.expand_inserts = b;
    }
    walk.max_work = o
        .uint("maxWork", 1, MAX_SAFE_INTEGER as u64)?
        .unwrap_or(DEFAULT_MAX_WORK);
    Ok(walk)
}

/// An sRGB colour option: `[r, g, b]` with integers 0 to 255 or `"#rrggbb"`; `null` is
/// `Some(None)` (no colour), a missing key `None` (the default).
fn color(o: &mut Opts, key: &str) -> Result<Option<Option<[u8; 3]>>, JsValue> {
    let wrong = || {
        bad_argument(&format!(
            "option {key} must be [r, g, b] with integers 0 to 255, \"#rrggbb\" or null"
        ))
    };
    let channel = |v: &Opt| match v {
        Opt::Num(n) if n.is_finite() && n.fract() == 0.0 && (0.0..=255.0).contains(n) => {
            Some(*n as u8)
        }
        _ => None,
    };
    match o.take(key)? {
        None => Ok(None),
        Some(Opt::Null) => Ok(Some(None)),
        Some(Opt::List(items)) => match items.as_slice() {
            [r, g, b] => match (channel(r), channel(g), channel(b)) {
                (Some(r), Some(g), Some(b)) => Ok(Some(Some([r, g, b]))),
                _ => Err(wrong()),
            },
            _ => Err(wrong()),
        },
        Some(Opt::Str(s)) => {
            let hex = s
                .strip_prefix('#')
                .filter(|h| h.len() == 6)
                .ok_or_else(wrong)?;
            let byte = |i: usize| {
                hex.get(i..i + 2)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            };
            match (byte(0), byte(2), byte(4)) {
                (Some(r), Some(g), Some(b)) => Ok(Some(Some([r, g, b]))),
                _ => Err(wrong()),
            }
        }
        Some(_) => Err(wrong()),
    }
}

/// A positive finite length option (drawing units).
fn positive(o: &mut Opts, key: &str) -> Result<Option<f64>, JsValue> {
    o.number(key, f64::MIN_POSITIVE, f64::MAX)
}

fn scene_error(e: &SceneError) -> JsValue {
    match e {
        SceneError::Precision { .. } => error_with(
            "limit_exceeded",
            &e.to_string(),
            Details {
                limit: Some("maxError"),
                ..Details::default()
            },
        ),
        // Options are validated before the builders run; anything left is an argument error.
        _ => bad_argument(&e.to_string()),
    }
}

/// JSON `type` tag of an `EntityKind::type_name` (`mtext` is `m_text` in JSON).
fn tag(type_name: &'static str) -> &'static str {
    if type_name == "mtext" {
        "m_text"
    } else {
        type_name
    }
}

fn skipped_json(skipped: &BTreeMap<&'static str, usize>) -> Value {
    Value::Object(
        skipped
            .iter()
            .map(|(k, v)| (tag(k).to_owned(), json!(v)))
            .collect(),
    )
}

fn layers_json(layers: &[LayerRange]) -> Value {
    Value::Array(
        layers
            .iter()
            .map(|l| {
                json!({
                    "name": l.name,
                    "firstTriangle": l.first_triangle,
                    "triangleCount": l.triangle_count,
                    "firstSegment": l.first_segment,
                    "segmentCount": l.segment_count,
                })
            })
            .collect(),
    )
}

fn set(target: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    Reflect::set(target, &key.into(), value).map(|_| ())
}

/// The columnar item table: one typed array per field plus the path pool.
fn items_table(items: &SceneItems) -> Result<JsValue, JsValue> {
    let n = items.items.len();
    let mut path_start = Vec::with_capacity(n);
    let mut path_length = Vec::with_capacity(n);
    let mut model_path_length = Vec::with_capacity(n);
    let mut layer = Vec::with_capacity(n);
    let mut kind = Vec::with_capacity(n);
    let mut surface = Vec::with_capacity(n);
    let mut kind_names: Vec<&'static str> = Vec::new();
    for item in &items.items {
        path_start.push(item.path_start);
        path_length.push(item.path_len);
        model_path_length.push(item.model_path_len);
        layer.push(item.layer);
        let name = tag(item.kind);
        let k = match kind_names.iter().position(|k| *k == name) {
            Some(k) => k,
            None => {
                kind_names.push(name);
                kind_names.len() - 1
            }
        };
        // There are about twenty entity kinds; an index beyond u8 cannot occur.
        kind.push(u8::try_from(k).unwrap_or(u8::MAX));
        surface.push(u8::from(item.surface));
    }
    let out = Object::new();
    set(&out, "count", &JsValue::from_f64(n as f64))?;
    set(
        &out,
        "pathStart",
        &Uint32Array::from(path_start.as_slice()).into(),
    )?;
    set(
        &out,
        "pathLength",
        &Uint32Array::from(path_length.as_slice()).into(),
    )?;
    set(
        &out,
        "modelPathLength",
        &Uint32Array::from(model_path_length.as_slice()).into(),
    )?;
    set(&out, "layer", &Uint32Array::from(layer.as_slice()).into())?;
    set(&out, "kind", &Uint8Array::from(kind.as_slice()).into())?;
    set(
        &out,
        "surface",
        &Uint8Array::from(surface.as_slice()).into(),
    )?;
    set(
        &out,
        "paths",
        &Uint32Array::from(items.paths.as_slice()).into(),
    )?;
    let names = Array::new();
    for k in kind_names {
        names.push(&JsValue::from_str(k));
    }
    set(&out, "kindNames", &names.into())?;
    Ok(out.into())
}

/// One item as an object, or `null` past the end.
fn item_json(
    items: &SceneItems,
    layers: &[LayerRange],
    index: &JsValue,
) -> Result<JsValue, JsValue> {
    let i = args::index("index", index, MAX_SAFE_INTEGER)?;
    let Some((entry, path)) = usize::try_from(i)
        .ok()
        .and_then(|i| Some((items.items.get(i)?, items.path(i)?)))
    else {
        return Ok(JsValue::NULL);
    };
    let model_path = path.get(..entry.model_path_len as usize).unwrap_or(path);
    json_to_js(&json!({
        "index": i,
        "path": path,
        "modelPath": model_path,
        "layer": layers.get(entry.layer as usize).map(|l| l.name.as_str()),
        "layerIndex": entry.layer,
        "kind": tag(entry.kind),
        "surface": entry.surface,
    }))
}

fn bounds_json(
    origin: [f64; 3],
    min: [f64; 3],
    max: [f64; 3],
    max_error: f64,
) -> Result<JsValue, JsValue> {
    json_to_js(&json!({ "origin": origin, "min": min, "max": max, "maxError": max_error }))
}

/// Three finite numbers (an array or a typed array).
fn vec3(name: &str, value: &JsValue) -> Result<[f64; 3], JsValue> {
    let wrong = || bad_argument(&format!("{name} must be an array of three finite numbers"));
    let array: Array = if Array::is_array(value) {
        value.clone().unchecked_into()
    } else if js_sys::ArrayBuffer::is_view(value) && !value.is_instance_of::<js_sys::DataView>() {
        let length = Reflect::get(value, &"length".into())?
            .as_f64()
            .unwrap_or(0.0);
        if length != 3.0 {
            return Err(wrong());
        }
        Array::from(value)
    } else {
        return Err(wrong());
    };
    if array.length() != 3 {
        return Err(wrong());
    }
    let mut out = [0.0; 3];
    for (slot, item) in out.iter_mut().zip(array.iter()) {
        *slot = item.as_f64().filter(|n| n.is_finite()).ok_or_else(wrong)?;
    }
    Ok(out)
}

/// Owned buffers of `CadDocument.surfaceMesh()`.
#[wasm_bindgen]
pub struct SurfaceMesh {
    mesh: cadkit_scene::SurfaceMesh,
}

/// Owned buffers of `CadDocument.lineBuffer()`.
#[wasm_bindgen]
pub struct LineBuffer {
    buffer: cadkit_scene::LineBuffer,
}

#[wasm_bindgen]
impl CadDocument {
    /// The faces of one model as triangles, with face outlines and wires as segments, in flat
    /// buffers relative to an f64 origin (see `SurfaceMeshOptions` and docs/wasm.md for the
    /// layout). Returns a `SurfaceMesh`; call `free()` on it when done.
    #[wasm_bindgen(js_name = surfaceMesh)]
    pub fn surface_mesh(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "SurfaceMeshOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<SurfaceMesh, JsValue> {
        let mut call = Call::new("surfaceMesh");
        let mut o = Opts::parse("options", options.as_ref())?.strict_keys();
        let mut s = SurfaceOptions::new();
        if let Some(m) = o.uint("model", 0, MAX_SAFE_INTEGER as u64)? {
            s.model = self.model_index(&JsValue::from_f64(m as f64))?;
        }
        s.walk = walk_options(&mut o)?;
        match o.string("polylines", 16)?.as_deref() {
            None => {}
            Some("auto") => s.polylines = PolylineSurfaces::Auto,
            Some("always") => s.polylines = PolylineSurfaces::Always,
            Some("never") => s.polylines = PolylineSurfaces::Never,
            Some(other) => {
                return Err(bad_argument(&format!(
                    "option polylines must be \"auto\", \"always\" or \"never\", got {other:?}"
                )));
            }
        }
        if let Some(p) = o.number("planarity", 0.0, f64::MAX)? {
            s.planarity = p;
        }
        if let Some(b) = o.bool("wires")? {
            s.wires = b;
        }
        if let Some(b) = o.bool("outlines")? {
            s.outlines = b;
        }
        if let Some(t) = positive(&mut o, "tolerance")? {
            s.tolerance = Some(t);
        }
        if let Some(e) = positive(&mut o, "maxError")? {
            s.max_error = Some(e);
        }
        if let Some(c) = color(&mut o, "neutralColor")? {
            s.neutral_color = c;
        }
        if let Some(c) = color(&mut o, "lineBackground")? {
            s.line_background = c;
        }
        let triangles = o
            .uint_capped("maxTriangles", MAX_TRIANGLES)?
            .unwrap_or(DEFAULT_TRIANGLES);
        s.max_triangles = usize::try_from(triangles).unwrap_or(0);
        let segments = o
            .uint_capped("maxSegments", MAX_SEGMENTS)?
            .unwrap_or(DEFAULT_SEGMENTS);
        s.max_segments = usize::try_from(segments).unwrap_or(0);
        if let Some(w) = o.uint("maxGeometryWork", 0, MAX_SAFE_INTEGER as u64)? {
            s.max_work = w;
        }
        o.finish(&mut call)?;
        let mesh = surface_mesh(&self.doc, &s).map_err(|e| scene_error(&e))?;
        Ok(SurfaceMesh { mesh })
    }

    /// The curves of one model as 2D or 3D line segments in flat buffers (see
    /// `LineBufferOptions`). Returns a `LineBuffer`; call `free()` on it when done.
    #[wasm_bindgen(js_name = lineBuffer)]
    pub fn line_buffer(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "LineBufferOptions | string | null")]
        options: Option<JsValue>,
    ) -> Result<LineBuffer, JsValue> {
        let mut call = Call::new("lineBuffer");
        let mut o = Opts::parse("options", options.as_ref())?.strict_keys();
        let mut l = LineOptions::new();
        if let Some(m) = o.uint("model", 0, MAX_SAFE_INTEGER as u64)? {
            l.model = self.model_index(&JsValue::from_f64(m as f64))?;
        }
        l.walk = walk_options(&mut o)?;
        if let Some(b) = o.bool("threeD")? {
            l.three_d = b;
        }
        if let Some(t) = positive(&mut o, "tolerance")? {
            l.tolerance = Some(t);
        }
        if let Some(e) = positive(&mut o, "maxError")? {
            l.max_error = Some(e);
        }
        if let Some(c) = color(&mut o, "background")? {
            l.background = c;
        }
        let segments = o
            .uint_capped("maxSegments", MAX_SEGMENTS)?
            .unwrap_or(DEFAULT_SEGMENTS);
        l.max_segments = usize::try_from(segments).unwrap_or(0);
        o.finish(&mut call)?;
        let buffer = line_buffer(&self.doc, &l).map_err(|e| scene_error(&e))?;
        Ok(LineBuffer { buffer })
    }
}

#[wasm_bindgen]
impl SurfaceMesh {
    /// x, y, z per vertex, relative to `bounds().origin`, drawing units (a copy).
    pub fn positions(&self) -> Float32Array {
        Float32Array::from(self.mesh.positions.as_slice())
    }

    /// Unit face normal per vertex (a copy).
    pub fn normals(&self) -> Float32Array {
        Float32Array::from(self.mesh.normals.as_slice())
    }

    /// sRGB per vertex, 3 bytes (a copy).
    pub fn colors(&self) -> Uint8Array {
        Uint8Array::from(self.mesh.colors.as_slice())
    }

    /// Three vertex indices per triangle, counter-clockwise about the face normal (a copy).
    pub fn indices(&self) -> Uint32Array {
        Uint32Array::from(self.mesh.indices.as_slice())
    }

    /// Item (index into `items()`) of each triangle (a copy).
    #[wasm_bindgen(js_name = triangleItems)]
    pub fn triangle_items(&self) -> Uint32Array {
        Uint32Array::from(self.mesh.triangle_items.as_slice())
    }

    /// Two end points per segment, x, y, z each, relative to the origin (a copy).
    pub fn segments(&self) -> Float32Array {
        Float32Array::from(self.mesh.segments.as_slice())
    }

    /// sRGB of both end points of each segment, 6 bytes per segment (a copy).
    #[wasm_bindgen(js_name = segmentColors)]
    pub fn segment_colors(&self) -> Uint8Array {
        Uint8Array::from(self.mesh.segment_colors.as_slice())
    }

    /// Item of each segment (a copy).
    #[wasm_bindgen(js_name = segmentItems)]
    pub fn segment_items(&self) -> Uint32Array {
        Uint32Array::from(self.mesh.segment_items.as_slice())
    }

    /// `{origin, min, max, maxError}`: the f64 origin in drawing units, the box around all
    /// positions relative to it, and the largest error of a stored coordinate (drawing units).
    #[wasm_bindgen(unchecked_return_type = "SceneBounds")]
    pub fn bounds(&self) -> Result<JsValue, JsValue> {
        let m = &self.mesh;
        bounds_json(m.origin, m.min, m.max, m.max_error)
    }

    /// The layer table: `[{name, firstTriangle, triangleCount, firstSegment, segmentCount}]`;
    /// triangles and segments of one layer are contiguous.
    #[wasm_bindgen(unchecked_return_type = "SceneLayerRange[]")]
    pub fn layers(&self) -> Result<JsValue, JsValue> {
        json_to_js(&layers_json(&self.mesh.layers))
    }

    /// Number of items.
    #[wasm_bindgen(js_name = itemCount)]
    pub fn item_count(&self) -> usize {
        self.mesh.items.len()
    }

    /// The item table as columns (`SceneItemTable`): typed arrays of one value per item and
    /// the pool of their entity paths.
    #[wasm_bindgen(unchecked_return_type = "SceneItemTable")]
    pub fn items(&self) -> Result<JsValue, JsValue> {
        items_table(&self.mesh.items)
    }

    /// Item `index` as an object (`SceneItem`), or `null` past the end.
    #[wasm_bindgen(unchecked_return_type = "SceneItem | null")]
    pub fn item(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] index: JsValue,
    ) -> Result<JsValue, JsValue> {
        item_json(&self.mesh.items, &self.mesh.layers, &index)
    }

    /// What was drawn and left out (`SurfaceMeshReport`).
    #[wasm_bindgen(unchecked_return_type = "SurfaceMeshReport")]
    pub fn report(&self) -> Result<JsValue, JsValue> {
        let r = &self.mesh.report;
        json_to_js(&json!({
            "faces": r.faces,
            "triangles": r.triangles,
            "segments": r.segments,
            "wires": r.wires,
            "duplicates": r.duplicates,
            "degenerate": r.degenerate,
            "flattened": r.flattened,
            "skipped": skipped_json(&r.skipped),
            "truncated": r.truncated,
        }))
    }

    /// The nearest triangle hit by the ray `origin + t * direction` (`t > 0`, both faces
    /// count), ignoring the layers whose indices (into `layers()`) are in `hiddenLayers`.
    /// `origin` is relative to `bounds().origin`, like the positions. Returns
    /// `{triangle, item, distance}` or `null`; `distance` is in drawing units when
    /// `direction` has unit length.
    #[wasm_bindgen(unchecked_return_type = "RayHit | null")]
    pub fn pick(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Vector3")] origin: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Vector3")] direction: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "number[] | null")] hidden_layers: Option<
            JsValue,
        >,
    ) -> Result<JsValue, JsValue> {
        let origin = vec3("origin", &origin)?;
        let direction = vec3("direction", &direction)?;
        if direction.iter().all(|d| *d == 0.0) {
            return Err(bad_argument("direction must not be zero"));
        }
        let hidden = hidden_layer_list(hidden_layers.as_ref(), self.mesh.layers.len())?;
        Ok(match self.mesh.pick(origin, direction, &hidden) {
            Some(hit) => json_to_js(&json!({
                "triangle": hit.triangle,
                "item": hit.item,
                "distance": hit.distance,
            }))?,
            None => JsValue::NULL,
        })
    }

    /// Bytes of wasm memory the buffers and the item table hold (approximate: allocator
    /// overhead is not counted).
    #[wasm_bindgen(js_name = byteLength)]
    pub fn byte_length(&self) -> f64 {
        let m = &self.mesh;
        let words = m.positions.len()
            + m.normals.len()
            + m.indices.len()
            + m.triangle_items.len()
            + m.segments.len()
            + m.segment_items.len();
        let bytes = 4 * words + m.colors.len() + m.segment_colors.len();
        (bytes + items_bytes(&m.items) + layers_bytes(&m.layers)) as f64
    }
}

#[wasm_bindgen]
impl LineBuffer {
    /// Coordinates per end point: 2 (x, y) or 3 (x, y, z).
    pub fn components(&self) -> u8 {
        self.buffer.components
    }

    /// Two end points per segment, `components()` f32 each, relative to the origin (a copy).
    pub fn segments(&self) -> Float32Array {
        Float32Array::from(self.buffer.segments.as_slice())
    }

    /// sRGB of both end points of each segment, 6 bytes per segment (a copy).
    #[wasm_bindgen(js_name = segmentColors)]
    pub fn segment_colors(&self) -> Uint8Array {
        Uint8Array::from(self.buffer.segment_colors.as_slice())
    }

    /// Item of each segment (a copy).
    #[wasm_bindgen(js_name = segmentItems)]
    pub fn segment_items(&self) -> Uint32Array {
        Uint32Array::from(self.buffer.segment_items.as_slice())
    }

    /// `{origin, min, max, maxError}` as for `SurfaceMesh.bounds()` (z is 0 in 2D).
    #[wasm_bindgen(unchecked_return_type = "SceneBounds")]
    pub fn bounds(&self) -> Result<JsValue, JsValue> {
        let b = &self.buffer;
        bounds_json(b.origin, b.min, b.max, b.max_error)
    }

    /// The layer table (triangle ranges are empty).
    #[wasm_bindgen(unchecked_return_type = "SceneLayerRange[]")]
    pub fn layers(&self) -> Result<JsValue, JsValue> {
        json_to_js(&layers_json(&self.buffer.layers))
    }

    /// Number of items.
    #[wasm_bindgen(js_name = itemCount)]
    pub fn item_count(&self) -> usize {
        self.buffer.items.len()
    }

    /// The item table as columns (`SceneItemTable`).
    #[wasm_bindgen(unchecked_return_type = "SceneItemTable")]
    pub fn items(&self) -> Result<JsValue, JsValue> {
        items_table(&self.buffer.items)
    }

    /// Item `index` as an object (`SceneItem`), or `null` past the end.
    #[wasm_bindgen(unchecked_return_type = "SceneItem | null")]
    pub fn item(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] index: JsValue,
    ) -> Result<JsValue, JsValue> {
        item_json(&self.buffer.items, &self.buffer.layers, &index)
    }

    /// What was drawn and left out (`LineBufferReport`).
    #[wasm_bindgen(unchecked_return_type = "LineBufferReport")]
    pub fn report(&self) -> Result<JsValue, JsValue> {
        let r = &self.buffer.report;
        json_to_js(&json!({
            "drawn": r.drawn,
            "segments": r.segments,
            "flattened": r.flattened,
            "degenerate": r.degenerate,
            "skipped": skipped_json(&r.skipped),
            "truncated": r.truncated,
        }))
    }

    /// Bytes of wasm memory the buffers and the item table hold (approximate).
    #[wasm_bindgen(js_name = byteLength)]
    pub fn byte_length(&self) -> f64 {
        let b = &self.buffer;
        let bytes = 4 * (b.segments.len() + b.segment_items.len()) + b.segment_colors.len();
        (bytes + items_bytes(&b.items) + layers_bytes(&b.layers)) as f64
    }
}

fn items_bytes(items: &SceneItems) -> usize {
    items.items.len() * std::mem::size_of::<cadkit_scene::SceneItem>() + 4 * items.paths.len()
}

fn layers_bytes(layers: &[LayerRange]) -> usize {
    layers
        .iter()
        .map(|l| std::mem::size_of::<LayerRange>() + l.name.len())
        .sum()
}

/// Layer indices of `pick`'s `hiddenLayers`; each must be an index of the layer table.
fn hidden_layer_list(value: Option<&JsValue>, layers: usize) -> Result<Vec<u32>, JsValue> {
    let Some(value) = value.filter(|v| !v.is_undefined() && !v.is_null()) else {
        return Ok(Vec::new());
    };
    if !Array::is_array(value) {
        return Err(bad_argument(
            "hiddenLayers must be an array of layer indices",
        ));
    }
    let array: &Array = value.unchecked_ref();
    if array.length() > MAX_HIDDEN_LAYERS {
        return Err(bad_argument(&format!(
            "hiddenLayers has more than {MAX_HIDDEN_LAYERS} items"
        )));
    }
    let mut out = Vec::with_capacity(array.length() as usize);
    for item in array.iter() {
        let i = args::index("hiddenLayers item", &item, MAX_SAFE_INTEGER)?;
        if i >= layers as u64 {
            return Err(bad_argument(&format!(
                "hiddenLayers item {i} is out of range (the mesh has {layers} layers)"
            )));
        }
        out.push(u32::try_from(i).unwrap_or(u32::MAX));
    }
    Ok(out)
}
