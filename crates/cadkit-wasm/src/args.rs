//! Validation of positional arguments. wasm-bindgen's own conversions trust the JS caller
//! (a number passed where a string is expected reads out of bounds, an `ArrayBuffer` passed
//! as `&[u8]` arrives empty), so every export takes `JsValue` and converts it here, throwing
//! `bad_argument` errors that name the argument.

use js_sys::{ArrayBuffer, JsString, Reflect, SharedArrayBuffer, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

use crate::error::{bad_argument, limit_exceeded};

/// Largest integer a JS number holds exactly (`Number.MAX_SAFE_INTEGER`).
pub(crate) const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Longest string argument that is not a document (names, labels, units), in UTF-16 units.
pub(crate) const MAX_NAME_UNITS: u32 = 65_536;

/// Copies a byte argument into wasm memory: `Uint8Array` (and Node `Buffer`), any other
/// typed array or `DataView` (its bytes, not its elements), `ArrayBuffer` or
/// `SharedArrayBuffer`. The length is checked against `max_bytes` before anything is copied.
pub(crate) fn bytes(name: &str, value: &JsValue, max_bytes: u64) -> Result<Vec<u8>, JsValue> {
    let view = if ArrayBuffer::is_view(value) {
        // ArrayBuffer.isView covers typed arrays and DataView in every realm.
        let buffer = Reflect::get(value, &"buffer".into())?;
        let offset = Reflect::get(value, &"byteOffset".into())?
            .as_f64()
            .unwrap_or(0.0);
        let length = Reflect::get(value, &"byteLength".into())?
            .as_f64()
            .unwrap_or(0.0);
        check_len(name, length, max_bytes)?;
        Uint8Array::new_with_byte_offset_and_length(&buffer, offset as u32, length as u32)
    } else if value.is_instance_of::<ArrayBuffer>() || value.is_instance_of::<SharedArrayBuffer>() {
        let length = Reflect::get(value, &"byteLength".into())?
            .as_f64()
            .unwrap_or(0.0);
        check_len(name, length, max_bytes)?;
        Uint8Array::new(value)
    } else {
        return Err(bad_argument(&format!(
            "{name} must be a Uint8Array, ArrayBuffer, typed array or DataView"
        )));
    };
    Ok(view.to_vec())
}

fn check_len(name: &str, length: f64, max_bytes: u64) -> Result<(), JsValue> {
    if length > max_bytes as f64 {
        return Err(limit_exceeded(
            "maxInputBytes",
            Some(max_bytes),
            Some(length as u64),
            &format!("{name} has {length} bytes; maxInputBytes is {max_bytes}"),
        ));
    }
    Ok(())
}

/// A required string argument of at most `max_units` UTF-16 units.
pub(crate) fn string(name: &str, value: &JsValue, max_units: u32) -> Result<String, JsValue> {
    let Some(js) = value.dyn_ref::<JsString>() else {
        return Err(bad_argument(&format!("{name} must be a string")));
    };
    if js.length() > max_units {
        return Err(bad_argument(&format!(
            "{name} is longer than {max_units} UTF-16 units"
        )));
    }
    Ok(String::from(js))
}

/// An optional string argument: `undefined` and `null` give `None`.
pub(crate) fn opt_string(
    name: &str,
    value: Option<&JsValue>,
    max_units: u32,
) -> Result<Option<String>, JsValue> {
    match value {
        None => Ok(None),
        Some(v) if v.is_undefined() || v.is_null() => Ok(None),
        Some(v) => string(name, v, max_units).map(Some),
    }
}

/// A required non-negative integer argument up to `max` (inclusive).
pub(crate) fn index(name: &str, value: &JsValue, max: f64) -> Result<u64, JsValue> {
    match value.as_f64() {
        Some(n) if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= max => Ok(n as u64),
        _ => Err(bad_argument(&format!(
            "{name} must be an integer from 0 to {max}"
        ))),
    }
}

/// A required finite number.
pub(crate) fn finite(name: &str, value: &JsValue) -> Result<f64, JsValue> {
    match value.as_f64() {
        Some(n) if n.is_finite() => Ok(n),
        _ => Err(bad_argument(&format!("{name} must be a finite number"))),
    }
}

/// An optional boolean: `undefined` and `null` give `None`, anything but a boolean is an error.
pub(crate) fn opt_bool(name: &str, value: Option<&JsValue>) -> Result<Option<bool>, JsValue> {
    match value {
        None => Ok(None),
        Some(v) if v.is_undefined() || v.is_null() => Ok(None),
        Some(v) => v
            .as_bool()
            .map(Some)
            .ok_or_else(|| bad_argument(&format!("{name} must be a boolean"))),
    }
}

/// An optional integer from `0` to `max`: `undefined` and `null` give `None`.
pub(crate) fn opt_index(
    name: &str,
    value: Option<&JsValue>,
    max: f64,
) -> Result<Option<u64>, JsValue> {
    match value {
        None => Ok(None),
        Some(v) if v.is_undefined() || v.is_null() => Ok(None),
        Some(v) => index(name, v, max).map(Some),
    }
}

/// A model index that exists in a document with `count` models. A document without models
/// accepts 0, so that renderers can be called on an empty document.
pub(crate) fn model(value: &JsValue, count: usize) -> Result<usize, JsValue> {
    let n = index("model", value, MAX_SAFE_INTEGER)?;
    let n = usize::try_from(n).unwrap_or(usize::MAX);
    if n >= count && !(n == 0 && count == 0) {
        return Err(bad_argument(&format!(
            "model index {n} is out of range (the document has {count} models)"
        )));
    }
    Ok(n)
}

/// An entity path: an index or an array of at most 256 indices (see `WalkContext::path`).
pub(crate) fn path(name: &str, value: &JsValue) -> Result<Vec<usize>, JsValue> {
    const MAX_STEPS: u32 = 256;
    let step = |v: &JsValue| {
        index(name, v, MAX_SAFE_INTEGER).map(|n| usize::try_from(n).unwrap_or(usize::MAX))
    };
    if value.as_f64().is_some() {
        return Ok(vec![step(value)?]);
    }
    if !js_sys::Array::is_array(value) {
        return Err(bad_argument(&format!(
            "{name} must be an index or an array of indices"
        )));
    }
    let array: &js_sys::Array = value.unchecked_ref();
    let length = array.length();
    if length == 0 || length > MAX_STEPS {
        return Err(bad_argument(&format!(
            "{name} must have 1 to {MAX_STEPS} steps"
        )));
    }
    let mut out = Vec::with_capacity(length as usize);
    for item in array.iter() {
        out.push(step(&item)?);
    }
    Ok(out)
}
