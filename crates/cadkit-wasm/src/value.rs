//! Rust values to JS values. Data crosses the boundary as JSON text parsed with `JSON.parse`
//! (not `serde-wasm-bindgen`): the model is already `Serialize`, the JSON shape then equals the
//! CLI and Python output, and the module needs one dependency fewer.

use serde::Serialize;
use std::io::{self, Write};
use wasm_bindgen::JsValue;

use crate::error::js_error;

/// JSON text to a JS value.
pub(crate) fn parse(text: &str) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(text)
}

/// A serializable Rust value to a JS value.
pub(crate) fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    let text = serde_json::to_string(value).map_err(|e| js_error("invalid", &e.to_string()))?;
    parse(&text)
}

/// A `serde_json::Value` to a JS value.
pub(crate) fn json_to_js(value: &serde_json::Value) -> Result<JsValue, JsValue> {
    parse(&value.to_string())
}

/// A JSON writer that refuses to grow past `limit` bytes, so an output budget holds before
/// the text is allocated.
pub(crate) struct CappedWriter {
    pub(crate) bytes: Vec<u8>,
    pub(crate) limit: usize,
}

impl CappedWriter {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    /// Serializes `value`; `false` (and the writer unchanged) when it does not fit.
    pub(crate) fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> bool {
        let start = self.bytes.len();
        if serde_json::to_writer(&mut *self, value).is_err() {
            self.bytes.truncate(start);
            return false;
        }
        true
    }

    /// Appends raw JSON punctuation; `false` when it does not fit.
    pub(crate) fn raw(&mut self, text: &str) -> bool {
        self.write_all(text.as_bytes()).is_ok()
    }

    pub(crate) fn into_string(self) -> String {
        // Only serde_json output and ASCII punctuation are written.
        String::from_utf8(self.bytes).unwrap_or_default()
    }
}

impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("output budget exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_writer_rolls_back_a_value_that_does_not_fit() {
        let mut w = CappedWriter::new(10);
        assert!(w.raw("["));
        assert!(w.push(&"abc"));
        assert!(!w.push(&"a long string"));
        assert!(w.raw("]"));
        assert_eq!(w.into_string(), "[\"abc\"]");
    }
}
