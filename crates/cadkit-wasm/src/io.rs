//! Format detection, header inspection, reading, DGN seed stripping and memory size.

use cadkit::ReadOptions;
use js_sys::Reflect;
use wasm_bindgen::prelude::*;

use crate::args;
use crate::document::CadDocument;
use crate::error::{bad_argument, from_cadkit_with, js_error};
use crate::options::{Call, Opts, read_options as parse_read_options};
use crate::value::{json_to_js, to_js};

/// Byte arguments that are not read with `ReadOptions` (seeds, detection) are still bounded.
pub(crate) fn input_ceiling() -> u64 {
    crate::options::ceilings().max_input_bytes
}

/// Detects the drawing format from the file header: `"dwg"`, `"dxf"`, `"dgn_v7"`,
/// `"dgn_v8"` or `"city_gml"`, or `undefined` when the bytes are not recognised.
#[wasm_bindgen(unchecked_return_type = "FormatName | undefined")]
pub fn detect(
    #[wasm_bindgen(unchecked_param_type = "BytesLike")] bytes: JsValue,
) -> Result<JsValue, JsValue> {
    let bytes = args::bytes("bytes", &bytes, input_ceiling())?;
    Ok(cadkit::detect(&bytes)
        .and_then(|f| serde_json::to_value(f).ok())
        .and_then(|v| v.as_str().map(JsValue::from_str))
        .unwrap_or(JsValue::UNDEFINED))
}

/// Inspects a drawing header without decoding entities. `options` are read options (object or
/// JSON text, see `ReadOptions`). The result keeps the snake_case keys of the Rust `Summary`.
#[wasm_bindgen(unchecked_return_type = "Summary")]
pub fn inspect(
    #[wasm_bindgen(unchecked_param_type = "BytesLike")] bytes: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")] options: Option<
        JsValue,
    >,
) -> Result<JsValue, JsValue> {
    let mut call = Call::new("inspect");
    let opts = parse_read_options(&mut call, "options", options.as_ref())?;
    let bytes = args::bytes("bytes", &bytes, opts.limits.max_input_bytes)?;
    let summary =
        cadkit::inspect_with(&bytes, &opts).map_err(|e| from_cadkit_with(&e, &opts.limits))?;
    to_js(&summary)
}

/// Reads a drawing (DWG, DXF, DGN V7/V8, CityGML 2.0) into a `CadDocument`.
///
/// `bytes` is a `Uint8Array`, `ArrayBuffer`, other typed array or `DataView`. `options`
/// (object or JSON text, see `ReadOptions`) set `keepRaw`, `codepage`, `preset` and the
/// resource limits. Defaults: 512 MiB input, 1 GiB decompressed, 1,500,000 objects, depth 64.
#[wasm_bindgen]
pub fn read(
    #[wasm_bindgen(unchecked_param_type = "BytesLike")] bytes: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")] options: Option<
        JsValue,
    >,
) -> Result<CadDocument, JsValue> {
    let mut call = Call::new("read");
    let opts = parse_read_options(&mut call, "options", options.as_ref())?;
    let bytes = args::bytes("bytes", &bytes, opts.limits.max_input_bytes)?;
    read_bytes(&bytes, &opts)
}

pub(crate) fn read_bytes(bytes: &[u8], opts: &ReadOptions) -> Result<CadDocument, JsValue> {
    let doc = cadkit::read_with(bytes, opts).map_err(|e| from_cadkit_with(&e, &opts.limits))?;
    Ok(CadDocument::new(doc))
}

/// Current size of this module's WebAssembly memory in bytes. The memory grows as documents
/// are read and never shrinks, even after `free()`; a host that sees it approach the 4 GiB
/// limit should recycle its worker (create a new module instance).
#[wasm_bindgen(js_name = memoryBytes)]
pub fn memory_bytes() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        core::arch::wasm32::memory_size::<0>() as f64 * 65_536.0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}

/// Copies a DGN V8 seed without its model's drawing content and without the document
/// properties that name people or organisations. `options` (`DgnStripSeedOptions`) are
/// `keepControls`, `keepRasterTemplate`, `keepCellDefinitions`, `modelName`, `timestampMs`
/// and `application`; here an unknown key is a `bad_argument` error (as in earlier releases),
/// so a misspelt option cannot keep what it meant to remove. `readOptions` sets the read
/// limits as for `read`. Returns `{ seed, report }`. Names in the kept tables remain: the
/// result is not anonymous.
#[wasm_bindgen(js_name = stripDgnSeed, unchecked_return_type = "StrippedDgnSeed")]
pub fn strip_dgn_seed(
    #[wasm_bindgen(unchecked_param_type = "BytesLike")] seed: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "DgnStripSeedOptions | string | null")]
    options: Option<JsValue>,
    #[wasm_bindgen(unchecked_optional_param_type = "ReadOptions | string | null")]
    read_options: Option<JsValue>,
) -> Result<JsValue, JsValue> {
    let mut call = Call::new("stripDgnSeed");
    let opts = strip_options(&mut call, options.as_ref())?;
    let read = parse_read_options(&mut call, "readOptions", read_options.as_ref())?;
    let seed = args::bytes("seed", &seed, read.limits.max_input_bytes)?;
    let stripped = cadkit::strip_dgn_seed(&seed, &opts, &read)
        .map_err(|e| from_cadkit_with(&e, &read.limits))?;
    let report =
        serde_json::to_value(&stripped.report).map_err(|e| js_error("invalid", &e.to_string()))?;
    let out = js_sys::Object::new();
    let bytes = js_sys::Uint8Array::from(stripped.bytes.as_slice());
    Reflect::set(&out, &"seed".into(), &bytes)?;
    Reflect::set(&out, &"report".into(), &json_to_js(&report)?)?;
    Ok(out.into())
}

fn strip_options(
    call: &mut Call,
    value: Option<&JsValue>,
) -> Result<cadkit::dgn::StripSeedOptions, JsValue> {
    let mut o = Opts::parse("options", value)?.strict_keys();
    let mut out = cadkit::dgn::StripSeedOptions::default();
    if let Some(b) = o.bool("keepControls")? {
        out.keep_controls = b;
    }
    if let Some(b) = o.bool("keepRasterTemplate")? {
        out.keep_raster_template = b;
    }
    if let Some(b) = o.bool("keepCellDefinitions")? {
        out.keep_cell_definitions = b;
    }
    if let Some(name) = o.nullable_string("modelName", 249)? {
        if name
            .as_deref()
            .is_some_and(|n| n.is_empty() || n.contains('\0'))
        {
            return Err(bad_argument(
                "option modelName must not be empty or contain NUL",
            ));
        }
        out.model_name = name;
    }
    if let Some(t) = o.number("timestampMs", 0.0, 1e15)? {
        out.timestamp_ms = t;
    }
    if let Some(app) = o.string("application", 255)? {
        out.application = app;
    }
    o.finish(call)?;
    Ok(out)
}
