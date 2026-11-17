//! JS errors thrown by the bindings.
//!
//! Every error is a JS `Error` with a `kind` from the closed list [`KINDS`] and, when the
//! cause has one, `offset` (absolute byte offset in the input), `limit` (the option that
//! bounds what was exceeded), `limitValue` (its value in this call) and `needed` (bytes or
//! items that would have been required). Hosts branch on `kind`, never on the message text.

use cadkit::{LimitKind, Limits};
use js_sys::Reflect;
use wasm_bindgen::JsValue;

/// Every `kind` an error thrown by this module can have. The list is closed: a new kind is a
/// breaking change of the JS API.
pub(crate) const KINDS: [&str; 7] = [
    "bad_argument",
    "unknown_format",
    "unsupported",
    "invalid",
    "truncated",
    "limit_exceeded",
    "io",
];

/// Extra numeric facts attached to an error as properties.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Details {
    pub offset: Option<u64>,
    pub limit: Option<&'static str>,
    pub limit_value: Option<u64>,
    pub needed: Option<u64>,
}

/// A JS `Error` with `kind`, `message` and the given details as properties.
pub(crate) fn error_with(kind: &'static str, message: &str, details: Details) -> JsValue {
    // Every kind comes from this module; anything else would break the documented list.
    let kind = if KINDS.contains(&kind) {
        kind
    } else {
        "invalid"
    };
    let err = js_sys::Error::new(&dedupe_prefix(message));
    // Setting a property on a fresh, unfrozen Error object cannot fail; ignore the Result.
    let _ = Reflect::set(&err, &"kind".into(), &kind.into());
    let set = |key: &str, value: Option<u64>| {
        if let Some(v) = value {
            // Offsets and counts above 2^53 lose precision as JS numbers; they cannot occur
            // with inputs below the 512 MiB ceiling.
            let _ = Reflect::set(&err, &key.into(), &JsValue::from_f64(v as f64));
        }
    };
    set("offset", details.offset);
    set("limitValue", details.limit_value);
    set("needed", details.needed);
    if let Some(limit) = details.limit {
        let _ = Reflect::set(&err, &"limit".into(), &limit.into());
    }
    err.into()
}

/// A JS `Error` with only `kind` and `message`.
pub(crate) fn js_error(kind: &'static str, message: &str) -> JsValue {
    error_with(kind, message, Details::default())
}

/// A `bad_argument` error; `message` names the argument or option.
pub(crate) fn bad_argument(message: &str) -> JsValue {
    js_error("bad_argument", message)
}

/// A `limit_exceeded` error raised by the bindings themselves.
pub(crate) fn limit_exceeded(
    limit: &'static str,
    limit_value: Option<u64>,
    needed: Option<u64>,
    message: &str,
) -> JsValue {
    error_with(
        "limit_exceeded",
        message,
        Details {
            limit: Some(limit),
            limit_value,
            needed,
            ..Details::default()
        },
    )
}

/// Option name of the limit behind a [`LimitKind`], as a host would raise it.
pub(crate) fn limit_name(kind: LimitKind) -> &'static str {
    match kind {
        LimitKind::InputBytes => "maxInputBytes",
        LimitKind::DecompressedBytes => "maxDecompressedBytes",
        LimitKind::Objects => "maxObjects",
        LimitKind::Depth => "maxDepth",
        LimitKind::StringBytes => "maxStringBytes",
        LimitKind::Vertices => "maxVertices",
        LimitKind::TotalVertices => "maxTotalVertices",
        LimitKind::Work => "maxWork",
        LimitKind::Output => "maxOutputBytes",
        LimitKind::Capacity => "capacity",
        _ => "other",
    }
}

fn limit_value(kind: LimitKind, limits: &Limits) -> Option<u64> {
    match kind {
        LimitKind::InputBytes => Some(limits.max_input_bytes),
        LimitKind::DecompressedBytes => Some(limits.max_decompressed_bytes),
        LimitKind::Objects => Some(limits.max_objects),
        LimitKind::Depth => Some(u64::from(limits.max_depth)),
        LimitKind::StringBytes => Some(u64::from(limits.max_string_bytes)),
        LimitKind::Vertices => Some(u64::from(limits.max_vertices)),
        LimitKind::TotalVertices => Some(limits.max_total_vertices),
        _ => None,
    }
}

/// Maps a cadkit error without knowing the limits of the call.
pub(crate) fn from_cadkit(e: &cadkit::Error) -> JsValue {
    convert(e, None)
}

/// Maps a cadkit error; a limit error also carries the value of the limit in `limits`.
pub(crate) fn from_cadkit_with(e: &cadkit::Error, limits: &Limits) -> JsValue {
    convert(e, Some(limits))
}

fn convert(e: &cadkit::Error, limits: Option<&Limits>) -> JsValue {
    let mut details = Details::default();
    let kind = match e {
        cadkit::Error::Truncated { offset, needed } => {
            details.offset = Some(*offset);
            details.needed = Some(*needed);
            "truncated"
        }
        cadkit::Error::Invalid { offset, .. } => {
            details.offset = Some(*offset);
            "invalid"
        }
        cadkit::Error::Unsupported(_) => "unsupported",
        cadkit::Error::LimitExceeded { limit, .. } => {
            details.limit = Some(limit_name(*limit));
            details.limit_value = limits.and_then(|l| limit_value(*limit, l));
            "limit_exceeded"
        }
        cadkit::Error::UnknownFormat => "unknown_format",
        cadkit::Error::Io(_) => "io",
        // Schema version mismatches and future variants: the input cannot be used as given.
        _ => "invalid",
    };
    error_with(kind, &e.to_string(), details)
}

/// Removes a leading `"<prefix>: "` that the rest of the message repeats
/// (`"unsupported: unsupported: x"` and `"unsupported: unsupported DXF code page"` lose the
/// first `"unsupported: "`): some messages already start with the word the error kind's
/// display adds.
pub(crate) fn dedupe_prefix(message: &str) -> String {
    let mut text = message;
    while let Some((head, rest)) = text.split_once(": ") {
        let repeated = rest
            .get(..head.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(head))
            && rest
                .get(head.len()..)
                .is_some_and(|after| after.starts_with(':') || after.starts_with(' '));
        if head.is_empty() || head.len() > 40 || !repeated {
            break;
        }
        text = rest;
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubled_prefixes_collapse() {
        assert_eq!(
            dedupe_prefix("unsupported: unsupported: x"),
            "unsupported: x"
        );
        assert_eq!(
            dedupe_prefix("unsupported: unsupported: unsupported: x: y"),
            "unsupported: x: y"
        );
        assert_eq!(
            dedupe_prefix("invalid data at offset 4: x"),
            "invalid data at offset 4: x"
        );
        assert_eq!(dedupe_prefix("a: b"), "a: b");
        assert_eq!(
            dedupe_prefix("unsupported: unsupported DXF output code page `x`"),
            "unsupported DXF output code page `x`"
        );
        assert_eq!(
            dedupe_prefix("unsupported: unsupportedness"),
            "unsupported: unsupportedness"
        );
        assert_eq!(dedupe_prefix(""), "");
    }

    #[test]
    fn every_limit_kind_has_an_option_name() {
        for kind in [
            LimitKind::InputBytes,
            LimitKind::DecompressedBytes,
            LimitKind::Objects,
            LimitKind::Depth,
            LimitKind::StringBytes,
            LimitKind::Vertices,
            LimitKind::TotalVertices,
        ] {
            assert!(limit_name(kind).starts_with("max"));
            assert!(limit_value(kind, &Limits::default()).is_some());
        }
    }
}
