//! The one options parser used by every export that takes options.
//!
//! Options arrive as a plain JS object or as JSON text with the same keys. Keys are camelCase;
//! the snake_case spelling of each key is accepted as well (the JSON form of the Rust option
//! structs). A value of the wrong type, `NaN`, a negative or fractional number where an integer
//! is needed, or an out-of-range value is a `bad_argument` error that names the option. An
//! unknown key is reported once per call through `console.warn` and otherwise ignored; it will
//! become an error in the next minor release.
//!
//! Resource limits ([`cadkit::Limits`]) are capped by the wasm ceilings ([`ceilings`]) on every
//! path: a larger value is lowered to the ceiling, because wasm32 has a 4 GiB address space and
//! an allocation failure aborts the module instead of returning an error.

use cadkit::{Limits, ReadOptions};
use js_sys::{Array, JsString, Object};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::args::MAX_SAFE_INTEGER;
use crate::error::bad_argument;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = warn)]
    fn console_warn(message: &str);
}

/// Longest JSON text accepted as an options argument.
const MAX_JSON_UNITS: u32 = 1 << 20;
/// Most keys in one options object, items in one array, nesting levels, string units in total.
const MAX_KEYS: u32 = 256;
const MAX_ITEMS: u32 = 20_000;
const MAX_NESTING: usize = 4;
const MAX_TOTAL_UNITS: u64 = 4 << 20;
/// Most values (numbers, strings, items) in one options argument. Arrays can share their items
/// in JS, so the copy is counted, not the JS object graph.
const MAX_TOTAL_VALUES: u64 = 100_000;
/// Longest single option string, in UTF-16 units.
const MAX_STRING_UNITS: u32 = 65_536;

/// One exported call: names the function in messages and collects unknown option keys,
/// reported in one `console.warn` when the call ends (on success or error).
pub(crate) struct Call {
    name: &'static str,
    unknown: Vec<String>,
}

impl Call {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            unknown: Vec::new(),
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        if self.unknown.is_empty() {
            return;
        }
        let keys: Vec<String> = self.unknown.iter().map(|k| format!("\"{k}\"")).collect();
        let message = format!(
            "cadkit-wasm: {}(): unknown option {} ignored; unknown options will be rejected from the next minor release",
            self.name,
            keys.join(", ")
        );
        #[cfg(target_arch = "wasm32")]
        console_warn(&message);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = message;
    }
}

/// An option value converted from JS or JSON.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Opt {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Opt>),
    Obj(Vec<(String, Opt)>),
}

impl Opt {
    fn type_name(&self) -> &'static str {
        match self {
            Opt::Null => "null",
            Opt::Bool(_) => "a boolean",
            Opt::Num(_) => "a number",
            Opt::Str(_) => "a string",
            Opt::List(_) => "an array",
            Opt::Obj(_) => "an object",
        }
    }
}

struct Budget {
    units: u64,
    values: u64,
}

fn from_js(value: &JsValue, key: &str, depth: usize, budget: &mut Budget) -> Result<Opt, JsValue> {
    budget.values += 1;
    if budget.values > MAX_TOTAL_VALUES {
        return Err(bad_argument(&format!(
            "options hold more than {MAX_TOTAL_VALUES} values"
        )));
    }
    if value.is_null() {
        return Ok(Opt::Null);
    }
    if let Some(b) = value.as_bool() {
        return Ok(Opt::Bool(b));
    }
    if let Some(n) = value.as_f64() {
        return Ok(Opt::Num(n));
    }
    if let Some(s) = value.dyn_ref::<JsString>() {
        let units = s.length();
        if units > MAX_STRING_UNITS {
            return Err(bad_argument(&format!(
                "option {key} is longer than {MAX_STRING_UNITS} UTF-16 units"
            )));
        }
        budget.units += u64::from(units);
        if budget.units > MAX_TOTAL_UNITS {
            return Err(bad_argument("options hold too much text"));
        }
        return Ok(Opt::Str(String::from(s)));
    }
    if depth >= MAX_NESTING {
        return Err(bad_argument(&format!("option {key} is nested too deeply")));
    }
    if Array::is_array(value) {
        let array: &Array = value.unchecked_ref();
        if array.length() > MAX_ITEMS {
            return Err(bad_argument(&format!(
                "option {key} has more than {MAX_ITEMS} items"
            )));
        }
        let mut items = Vec::with_capacity(array.length() as usize);
        for item in array.iter() {
            items.push(from_js(&item, key, depth + 1, budget)?);
        }
        return Ok(Opt::List(items));
    }
    if is_plain_object(value) {
        return object_from_js(value, key, depth, budget);
    }
    Err(bad_argument(&format!(
        "option {key} must be a boolean, number, string, array or plain object"
    )))
}

fn is_plain_object(value: &JsValue) -> bool {
    if !value.is_object() || value.is_function() {
        return false;
    }
    let proto = Object::get_prototype_of(value);
    proto.is_null() || Object::get_prototype_of(&proto).is_null()
}

fn object_from_js(
    value: &JsValue,
    key: &str,
    depth: usize,
    budget: &mut Budget,
) -> Result<Opt, JsValue> {
    let object: &Object = value.unchecked_ref();
    let keys = Object::keys(object);
    if keys.length() > MAX_KEYS {
        return Err(bad_argument(&format!(
            "{key} has more than {MAX_KEYS} keys"
        )));
    }
    let mut entries = Vec::with_capacity(keys.length() as usize);
    for k in keys.iter() {
        let Some(name) = k.as_string() else { continue };
        let v = js_sys::Reflect::get(value, &k)?;
        if v.is_undefined() {
            // `{ maxObjects: undefined }` means "not set", as for optional properties in TS.
            continue;
        }
        let child = from_js(&v, &name, depth + 1, budget)?;
        entries.push((name, child));
    }
    Ok(Opt::Obj(entries))
}

fn from_json(value: Value, key: &str, depth: usize) -> Result<Opt, JsValue> {
    Ok(match value {
        Value::Null => Opt::Null,
        Value::Bool(b) => Opt::Bool(b),
        Value::Number(n) => Opt::Num(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => Opt::Str(s),
        Value::Array(_) | Value::Object(_) if depth >= MAX_NESTING => {
            return Err(bad_argument(&format!("option {key} is nested too deeply")));
        }
        Value::Array(items) => {
            if items.len() > MAX_ITEMS as usize {
                return Err(bad_argument(&format!(
                    "option {key} has more than {MAX_ITEMS} items"
                )));
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(from_json(item, key, depth + 1)?);
            }
            Opt::List(out)
        }
        Value::Object(map) => {
            if map.len() > MAX_KEYS as usize {
                return Err(bad_argument(&format!(
                    "{key} has more than {MAX_KEYS} keys"
                )));
            }
            let mut out = Vec::with_capacity(map.len());
            for (k, v) in map {
                let child = from_json(v, &k, depth + 1)?;
                out.push((k, child));
            }
            Opt::Obj(out)
        }
    })
}

/// The parsed options of one argument, read key by key.
pub(crate) struct Opts {
    /// `"options"` for a top-level argument, `"options.limits"` for a nested object.
    path: String,
    entries: Vec<(String, Opt)>,
    used: Vec<bool>,
    /// An unknown key is an error instead of a warning (options that were strict already).
    strict_keys: bool,
}

/// What [`Opts::parse`] accepts as the "no options" value.
fn is_absent(value: &JsValue) -> bool {
    value.is_undefined() || value.is_null()
}

impl Opts {
    /// Parses an options argument: `undefined`, `null` or `""` (no options), a plain object, or
    /// JSON text of an object.
    pub(crate) fn parse(arg: &str, value: Option<&JsValue>) -> Result<Self, JsValue> {
        let mut out = Self {
            path: arg.to_owned(),
            entries: Vec::new(),
            used: Vec::new(),
            strict_keys: false,
        };
        let Some(value) = value.filter(|v| !is_absent(v)) else {
            return Ok(out);
        };
        let opt = if let Some(text) = value.dyn_ref::<JsString>() {
            if text.length() > MAX_JSON_UNITS {
                return Err(bad_argument(&format!(
                    "{arg} JSON text is longer than {MAX_JSON_UNITS} UTF-16 units"
                )));
            }
            let text = String::from(text);
            if text.trim().is_empty() {
                return Ok(out);
            }
            let json: Value = serde_json::from_str(&text)
                .map_err(|e| bad_argument(&format!("{arg} is not valid JSON: {e}")))?;
            if !json.is_object() {
                return Err(bad_argument(&format!(
                    "{arg} JSON text must hold an object"
                )));
            }
            from_json(json, arg, 0)?
        } else if is_plain_object(value) {
            object_from_js(
                value,
                arg,
                0,
                &mut Budget {
                    units: 0,
                    values: 0,
                },
            )?
        } else {
            return Err(bad_argument(&format!(
                "{arg} must be an object or a JSON string"
            )));
        };
        if let Opt::Obj(entries) = opt {
            out.used = vec![false; entries.len()];
            out.entries = entries;
        }
        Ok(out)
    }

    /// Makes unknown keys errors instead of warnings.
    pub(crate) fn strict_keys(mut self) -> Self {
        self.strict_keys = true;
        self
    }

    fn name(&self, key: &str) -> String {
        if self.path == "options" {
            key.to_owned()
        } else {
            format!("{}.{key}", self.path.trim_start_matches("options."))
        }
    }

    /// Takes the value of `key` (camelCase) or of its snake_case spelling.
    pub(crate) fn take(&mut self, key: &str) -> Result<Option<Opt>, JsValue> {
        let snake = camel_to_snake(key);
        let mut found: Option<usize> = None;
        for (i, (k, _)) in self.entries.iter().enumerate() {
            if k == key || *k == snake {
                if found.is_some() {
                    return Err(bad_argument(&format!(
                        "option {} is given twice ({key} and {snake})",
                        self.name(key)
                    )));
                }
                found = Some(i);
            }
        }
        let Some(i) = found else { return Ok(None) };
        if let Some(u) = self.used.get_mut(i) {
            *u = true;
        }
        Ok(self.entries.get(i).map(|(_, v)| v.clone()))
    }

    fn wrong(&self, key: &str, expected: &str, got: &Opt) -> JsValue {
        bad_argument(&format!(
            "option {} must be {expected}, got {}",
            self.name(key),
            match got {
                Opt::Num(n) if n.is_nan() => "NaN",
                Opt::Num(n) if !n.is_finite() => "an infinite number",
                other => other.type_name(),
            }
        ))
    }

    /// A boolean option; `null` counts as not given.
    pub(crate) fn bool(&mut self, key: &str) -> Result<Option<bool>, JsValue> {
        match self.take(key)? {
            None | Some(Opt::Null) => Ok(None),
            Some(Opt::Bool(b)) => Ok(Some(b)),
            Some(other) => Err(self.wrong(key, "a boolean", &other)),
        }
    }

    /// A non-negative integer option, lowered to `ceiling` when it is larger.
    pub(crate) fn uint_capped(&mut self, key: &str, ceiling: u64) -> Result<Option<u64>, JsValue> {
        Ok(self
            .uint(key, 0, MAX_SAFE_INTEGER as u64)?
            .map(|n| n.min(ceiling)))
    }

    /// An integer option in `min..=max`; anything else is an error.
    pub(crate) fn uint(&mut self, key: &str, min: u64, max: u64) -> Result<Option<u64>, JsValue> {
        match self.take(key)? {
            None | Some(Opt::Null) => Ok(None),
            Some(Opt::Num(n))
                if n.is_finite() && n.fract() == 0.0 && n >= min as f64 && n <= max as f64 =>
            {
                Ok(Some(n as u64))
            }
            Some(other) => Err(self.wrong(key, &format!("an integer from {min} to {max}"), &other)),
        }
    }

    /// A finite number option in `min..=max`.
    pub(crate) fn number(&mut self, key: &str, min: f64, max: f64) -> Result<Option<f64>, JsValue> {
        match self.take(key)? {
            None | Some(Opt::Null) => Ok(None),
            Some(Opt::Num(n)) if n.is_finite() && n >= min && n <= max => Ok(Some(n)),
            Some(other) => Err(self.wrong(key, &range_text(min, max), &other)),
        }
    }

    /// A finite number option in `min..=max` that may be `null` on purpose: `Some(None)` for
    /// `null`.
    pub(crate) fn nullable_number(
        &mut self,
        key: &str,
        min: f64,
        max: f64,
    ) -> Result<Option<Option<f64>>, JsValue> {
        match self.take(key)? {
            None => Ok(None),
            Some(Opt::Null) => Ok(Some(None)),
            Some(Opt::Num(n)) if n.is_finite() && n >= min && n <= max => Ok(Some(Some(n))),
            Some(other) => {
                Err(self.wrong(key, &format!("{} or null", range_text(min, max)), &other))
            }
        }
    }

    /// A string option of at most `max_units` UTF-16 units; `null` counts as not given.
    pub(crate) fn string(
        &mut self,
        key: &str,
        max_units: usize,
    ) -> Result<Option<String>, JsValue> {
        Ok(self.nullable_string(key, max_units)?.flatten())
    }

    /// A string option that may be `null` on purpose: `Some(None)` for `null`.
    pub(crate) fn nullable_string(
        &mut self,
        key: &str,
        max_units: usize,
    ) -> Result<Option<Option<String>>, JsValue> {
        match self.take(key)? {
            None => Ok(None),
            Some(Opt::Null) => Ok(Some(None)),
            Some(Opt::Str(s)) if s.encode_utf16().count() <= max_units => Ok(Some(Some(s))),
            Some(Opt::Str(_)) => Err(bad_argument(&format!(
                "option {} is longer than {max_units} UTF-16 units",
                self.name(key)
            ))),
            Some(other) => Err(self.wrong(key, "a string", &other)),
        }
    }

    /// An array of strings: at most `max_items` items of at most `max_units` UTF-16 units each
    /// and `max_bytes` UTF-8 bytes together.
    pub(crate) fn strings(
        &mut self,
        key: &str,
        max_items: usize,
        max_units: usize,
        max_bytes: usize,
    ) -> Result<Option<Vec<String>>, JsValue> {
        let items = match self.take(key)? {
            None | Some(Opt::Null) => return Ok(None),
            Some(Opt::List(items)) => items,
            Some(other) => return Err(self.wrong(key, "an array of strings", &other)),
        };
        if items.len() > max_items {
            return Err(bad_argument(&format!(
                "option {} has more than {max_items} items",
                self.name(key)
            )));
        }
        let mut out = Vec::with_capacity(items.len());
        let mut bytes = 0usize;
        for item in items {
            let Opt::Str(s) = item else {
                return Err(self.wrong(key, "an array of strings", &item));
            };
            if s.encode_utf16().count() > max_units {
                return Err(bad_argument(&format!(
                    "an item of option {} is longer than {max_units} UTF-16 units",
                    self.name(key)
                )));
            }
            bytes += s.len();
            if bytes > max_bytes {
                return Err(bad_argument(&format!(
                    "option {} holds more than {max_bytes} bytes of text",
                    self.name(key)
                )));
            }
            out.push(s);
        }
        Ok(Some(out))
    }

    /// An array of finite numbers.
    pub(crate) fn numbers(
        &mut self,
        key: &str,
        max_items: usize,
    ) -> Result<Option<Vec<f64>>, JsValue> {
        match self.take(key)? {
            None | Some(Opt::Null) => Ok(None),
            Some(other) => numbers_of(&other, max_items)
                .map(Some)
                .ok_or_else(|| self.wrong(key, "an array of finite numbers", &other)),
        }
    }

    /// A nested options object, read with the same rules.
    pub(crate) fn object(&mut self, key: &str) -> Result<Option<Opts>, JsValue> {
        let path = format!("{}.{key}", self.path);
        let strict_keys = self.strict_keys;
        match self.take(key)? {
            None | Some(Opt::Null) => Ok(None),
            Some(Opt::Obj(entries)) => Ok(Some(Opts {
                path,
                used: vec![false; entries.len()],
                entries,
                strict_keys,
            })),
            Some(other) => Err(self.wrong(key, "an object", &other)),
        }
    }

    /// Ends reading: unknown keys go to `call` (or are an error with [`Self::strict_keys`]).
    pub(crate) fn finish(self, call: &mut Call) -> Result<(), JsValue> {
        let unknown: Vec<String> = self
            .entries
            .iter()
            .zip(&self.used)
            .filter(|(_, used)| !**used)
            .map(|((k, _), _)| self.name(k))
            .collect();
        if unknown.is_empty() {
            return Ok(());
        }
        if self.strict_keys {
            return Err(bad_argument(&format!(
                "unknown option {} for {}()",
                unknown.join(", "),
                call.name
            )));
        }
        for k in unknown {
            if !call.unknown.contains(&k) {
                call.unknown.push(k);
            }
        }
        Ok(())
    }
}

/// An option that is either a boolean or a nested options object (see [`Opts::bool_or_object`]).
pub(crate) enum BoolOr {
    Bool(bool),
    Object(Opts),
}

impl Opts {
    /// A boolean option or a nested options object; `null` counts as not given.
    pub(crate) fn bool_or_object(&mut self, key: &str) -> Result<Option<BoolOr>, JsValue> {
        let path = format!("{}.{key}", self.path);
        let strict_keys = self.strict_keys;
        match self.take(key)? {
            None | Some(Opt::Null) => Ok(None),
            Some(Opt::Bool(b)) => Ok(Some(BoolOr::Bool(b))),
            Some(Opt::Obj(entries)) => Ok(Some(BoolOr::Object(Opts {
                path,
                used: vec![false; entries.len()],
                entries,
                strict_keys,
            }))),
            Some(other) => Err(self.wrong(key, "a boolean or an object", &other)),
        }
    }
}

/// The finite numbers of an [`Opt::List`], or `None` when it is something else.
pub(crate) fn numbers_of(opt: &Opt, max_items: usize) -> Option<Vec<f64>> {
    let Opt::List(items) = opt else { return None };
    if items.len() > max_items {
        return None;
    }
    items
        .iter()
        .map(|i| match i {
            Opt::Num(n) if n.is_finite() => Some(*n),
            _ => None,
        })
        .collect()
}

/// "a number from 0 to 1", "a number greater than 0 and at most 1000", "a finite number
/// greater than 0": the accepted range of a number option, without printing the smallest
/// positive or the largest finite f64.
fn range_text(min: f64, max: f64) -> String {
    let unbounded = max == f64::MAX;
    match (min == f64::MIN_POSITIVE, unbounded) {
        (true, true) => "a finite number greater than 0".to_owned(),
        (true, false) => format!("a number greater than 0 and at most {max}"),
        (false, true) => format!("a finite number of at least {min}"),
        (false, false) => format!("a number from {min} to {max}"),
    }
}

/// `maxObjects` -> `max_objects`.
pub(crate) fn camel_to_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

// --- Limits -------------------------------------------------------------------------------

/// The highest value each limit can be raised to in this module.
pub(crate) const fn ceilings() -> Limits {
    Limits::new()
        .with_max_input_bytes(512 << 20)
        .with_max_decompressed_bytes(1 << 30)
        .with_max_objects(5_000_000)
        .with_max_depth(64)
        .with_max_string_bytes(64 << 20)
        .with_max_vertices(10_000_000)
        .with_max_total_vertices(50_000_000)
}

/// Default read limits of this module. A document costs about 800 bytes of wasm memory per
/// entity, so 1.5 million objects (about 1.2 GB) leave room in the 4 GiB address space for the
/// input bytes, exports and a second document.
pub(crate) const fn default_read_limits() -> Limits {
    ceilings().with_max_objects(1_500_000)
}

/// Every limit lowered to its ceiling.
pub(crate) fn capped(limits: Limits) -> Limits {
    let c = ceilings();
    limits
        .with_max_input_bytes(limits.max_input_bytes.min(c.max_input_bytes))
        .with_max_decompressed_bytes(limits.max_decompressed_bytes.min(c.max_decompressed_bytes))
        .with_max_objects(limits.max_objects.min(c.max_objects))
        .with_max_depth(limits.max_depth.min(c.max_depth))
        .with_max_string_bytes(limits.max_string_bytes.min(c.max_string_bytes))
        .with_max_vertices(limits.max_vertices.min(c.max_vertices))
        .with_max_total_vertices(limits.max_total_vertices.min(c.max_total_vertices))
}

/// Default limits of writers and validators: the desktop defaults under the ceilings.
pub(crate) fn default_write_limits() -> Limits {
    capped(Limits::default())
}

/// Reads `preset` and the seven limit keys, at the top level and in a nested `limits`
/// object (which wins), on top of `base`. The result is capped by [`ceilings`].
pub(crate) fn limits(opts: &mut Opts, base: Limits, call: &mut Call) -> Result<Limits, JsValue> {
    let mut limits = match opts.string("preset", 16)?.as_deref() {
        None => base,
        Some("default") => base,
        Some("strict") => Limits::strict(),
        Some(other) => {
            return Err(bad_argument(&format!(
                "option preset must be \"default\" or \"strict\", got {other:?}"
            )));
        }
    };
    limits = limit_keys(opts, limits)?;
    if let Some(mut nested) = opts.object("limits")? {
        limits = limit_keys(&mut nested, limits)?;
        nested.finish(call)?;
    }
    Ok(capped(limits))
}

fn limit_keys(opts: &mut Opts, mut l: Limits) -> Result<Limits, JsValue> {
    let c = ceilings();
    let u32_cap =
        |n: u64, ceiling: u32| u32::try_from(n.min(u64::from(ceiling))).unwrap_or(ceiling);
    if let Some(n) = opts.uint_capped("maxInputBytes", c.max_input_bytes)? {
        l.max_input_bytes = n;
    }
    if let Some(n) = opts.uint_capped("maxDecompressedBytes", c.max_decompressed_bytes)? {
        l.max_decompressed_bytes = n;
    }
    if let Some(n) = opts.uint_capped("maxObjects", c.max_objects)? {
        l.max_objects = n;
    }
    if let Some(n) = opts.uint_capped("maxDepth", u64::from(c.max_depth))? {
        l.max_depth = u32_cap(n, c.max_depth);
    }
    if let Some(n) = opts.uint_capped("maxStringBytes", u64::from(c.max_string_bytes))? {
        l.max_string_bytes = u32_cap(n, c.max_string_bytes);
    }
    if let Some(n) = opts.uint_capped("maxVertices", u64::from(c.max_vertices))? {
        l.max_vertices = u32_cap(n, c.max_vertices);
    }
    if let Some(n) = opts.uint_capped("maxTotalVertices", c.max_total_vertices)? {
        l.max_total_vertices = n;
    }
    Ok(l)
}

/// `ReadOptions`: `preset`, the limit keys, `keepRaw`, `codepage` (alias `fallbackCodepage`).
pub(crate) fn read_options(
    call: &mut Call,
    arg: &str,
    value: Option<&JsValue>,
) -> Result<ReadOptions, JsValue> {
    let mut opts = Opts::parse(arg, value)?;
    let mut out = ReadOptions::default().with_limits(default_read_limits());
    out.limits = limits(&mut opts, out.limits, call)?;
    if let Some(keep) = opts.bool("keepRaw")? {
        out.keep_raw = keep;
    }
    let codepage = opts.nullable_string("codepage", 64)?;
    let fallback = opts.nullable_string("fallbackCodepage", 64)?;
    if codepage.is_some() && fallback.is_some() {
        return Err(bad_argument(
            "options codepage and fallbackCodepage are the same option; give one",
        ));
    }
    if let Some(cp) = codepage.or(fallback) {
        out.fallback_codepage = cp;
    }
    opts.finish(call)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(entries: Vec<(&str, Opt)>) -> Opts {
        Opts {
            path: "options".into(),
            used: vec![false; entries.len()],
            entries: entries
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            strict_keys: false,
        }
    }

    #[test]
    fn keys_accept_both_spellings_and_unknown_ones_are_collected() {
        let mut o = opts(vec![
            ("max_objects", Opt::Num(10.0)),
            ("maxObject", Opt::Num(3.0)),
        ]);
        let l = limit_keys(&mut o, default_read_limits()).unwrap();
        assert_eq!(l.max_objects, 10);
        let mut call = Call::new("read");
        o.finish(&mut call).unwrap();
        assert_eq!(call.unknown, vec!["maxObject".to_owned()]);
        call.unknown.clear();
    }

    #[test]
    fn limits_are_capped() {
        let mut o = opts(vec![
            ("maxInputBytes", Opt::Num(1e15)),
            ("maxDepth", Opt::Num(1000.0)),
        ]);
        let l = limit_keys(&mut o, default_read_limits()).unwrap();
        assert_eq!(l.max_input_bytes, 512 << 20);
        assert_eq!(l.max_depth, 64);
        assert_eq!(default_read_limits().max_objects, 1_500_000);
        assert!(capped(Limits::default()).max_total_vertices <= ceilings().max_total_vertices);
    }

    #[test]
    fn ranges_are_stated_without_extreme_floats() {
        assert_eq!(range_text(0.0, 1.0), "a number from 0 to 1");
        assert_eq!(
            range_text(f64::MIN_POSITIVE, 1000.0),
            "a number greater than 0 and at most 1000"
        );
        assert_eq!(
            range_text(f64::MIN_POSITIVE, f64::MAX),
            "a finite number greater than 0"
        );
        assert_eq!(range_text(0.0, f64::MAX), "a finite number of at least 0");
    }

    #[test]
    fn camel_case_maps_to_snake_case() {
        assert_eq!(camel_to_snake("maxTotalVertices"), "max_total_vertices");
        assert_eq!(camel_to_snake("model"), "model");
    }

    #[test]
    fn numbers_of_rejects_non_finite_items() {
        assert_eq!(
            numbers_of(&Opt::List(vec![Opt::Num(1.0), Opt::Num(2.0)]), 4),
            Some(vec![1.0, 2.0])
        );
        assert_eq!(numbers_of(&Opt::List(vec![Opt::Num(f64::NAN)]), 4), None);
        assert_eq!(numbers_of(&Opt::List(vec![Opt::Num(1.0); 5]), 4), None);
    }
}
