//! Stable C ABI over the `cadkit` facade.
//!
//! The public contract is `include/cadkit.h` (hand-maintained; `tests/header.rs` checks that
//! every exported symbol is declared there) and the header-only C++17 wrapper
//! `include/cadkit.hpp`.
//!
//! Design rules:
//!
//! * Every function returns a [`cadkit_status`]; output goes through out-pointers.
//! * Every pointer argument is null-checked; no panic crosses the FFI boundary
//!   (`catch_unwind` turns it into [`cadkit_status::Panic`]).
//! * The last error message is thread-local and owned by the library.
//! * Documents are immutable after creation. Live handles are tracked in a registry so that a
//!   second `cadkit_document_free` or a use after free is reported as
//!   [`cadkit_status::InvalidHandle`] instead of corrupting memory (best effort: a freed
//!   address that is reused by a new document is indistinguishable from the new one).

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, OnceLock};

use cadkit::{Document, DxfVersion, Error, Format, Limits, ReadOptions, SvgOptions};

pub use cadkit;

/// Library version as a NUL-terminated string.
static VERSION: &CStr =
    match CStr::from_bytes_with_nul(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes()) {
        Ok(v) => v,
        Err(_) => c"unknown",
    };

/// Result code of every fallible call. Values are part of the ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum cadkit_status {
    /// Success.
    Ok = 0,
    /// A required pointer argument was NULL.
    NullPointer = 1,
    /// An argument was out of range or not valid UTF-8.
    InvalidArgument = 2,
    /// File could not be read.
    Io = 3,
    /// Input ended early.
    Truncated = 4,
    /// Input violates the format.
    InvalidData = 5,
    /// Valid input using a feature that is not implemented.
    Unsupported = 6,
    /// A configured resource limit was exceeded.
    LimitExceeded = 7,
    /// Input is not a recognized drawing format.
    UnknownFormat = 8,
    /// An internal panic was caught at the boundary (a bug; please report).
    Panic = 9,
    /// Unexpected internal failure.
    Internal = 10,
    /// The document handle is not (or no longer) a live document.
    InvalidHandle = 11,
}

/// Detected file format. Values are part of the ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum cadkit_format {
    /// Not recognized.
    Unknown = 0,
    /// DWG.
    Dwg = 1,
    /// DXF (ASCII or binary).
    Dxf = 2,
    /// DGN V7.
    DgnV7 = 3,
    /// DGN V8.
    DgnV8 = 4,
    /// CityGML 2.0.
    CityGml = 5,
}

/// DXF output version. Values are part of the ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum cadkit_dxf_version {
    /// R12 (AC1009).
    R12 = 0,
    /// 2000 (AC1015).
    R2000 = 1,
    /// 2004 (AC1018).
    R2004 = 2,
    /// 2007 (AC1021).
    R2007 = 3,
    /// 2010 (AC1024).
    R2010 = 4,
    /// 2013 (AC1027).
    R2013 = 5,
    /// 2018 (AC1032).
    R2018 = 6,
}

/// Read options. Zero in a numeric field selects the library default; `NULL` options select
/// all defaults. Initialize with [`cadkit_options_init`] to be forward compatible.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct cadkit_options {
    /// Largest accepted input in bytes (0 = default).
    pub max_input_bytes: u64,
    /// Largest total decompressed size in bytes (0 = default).
    pub max_decompressed_bytes: u64,
    /// Most records per document (0 = default).
    pub max_objects: u64,
    /// Deepest nesting (0 = default).
    pub max_depth: u32,
    /// Longest single string in bytes (0 = default).
    pub max_string_bytes: u32,
    /// Most vertices in one entity (0 = default).
    pub max_vertices: u32,
    /// Non-zero keeps original record bytes in the model.
    pub keep_raw: i32,
    /// Code page label for 8-bit text without a declared one, e.g. `"windows-1254"`; may be NULL.
    pub fallback_codepage: *const c_char,
}

/// SVG options. Initialize with [`cadkit_svg_options_init`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct cadkit_svg_options {
    /// Index of the model to render.
    pub model_index: u32,
    /// Output width in CSS pixels (> 0).
    pub width_px: f64,
    /// Stroke width in CSS pixels (> 0).
    pub stroke_px: f64,
    /// CSS background color, or NULL for transparent.
    pub background: *const c_char,
    /// Non-zero renders text entities.
    pub text: i32,
    /// Non-zero expands block references.
    pub expand_blocks: i32,
}

/// Summary of a document (plain data, no ownership).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct cadkit_info {
    /// Source format.
    pub format: cadkit_format,
    /// Number of models (model space, layouts, DGN models).
    pub model_count: u64,
    /// Number of layers / levels.
    pub layer_count: u64,
    /// Number of block definitions.
    pub block_count: u64,
    /// Number of top-level entities over all models.
    pub entity_count: u64,
    /// Number of reader warnings.
    pub warning_count: u64,
}

/// Opaque document handle. Create with `cadkit_read_*`, release with [`cadkit_document_free`].
pub struct cadkit_document {
    doc: Document,
}

// ---------------------------------------------------------------------------------------------
// internal state

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn live_handles() -> MutexGuard<'static, HashSet<usize>> {
    static LIVE: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn set_error(message: &str) {
    let cleaned: Vec<u8> = message.bytes().filter(|b| *b != 0).collect();
    let c = CString::new(cleaned).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = c);
}

fn clear_error() {
    LAST_ERROR.with(|e| *e.borrow_mut() = CString::default());
}

fn fail(status: cadkit_status, message: &str) -> cadkit_status {
    set_error(message);
    status
}

fn map_error(err: &Error) -> cadkit_status {
    let status = match err {
        Error::Truncated { .. } => cadkit_status::Truncated,
        Error::Invalid { .. } => cadkit_status::InvalidData,
        Error::Unsupported(_) => cadkit_status::Unsupported,
        Error::LimitExceeded(_) => cadkit_status::LimitExceeded,
        Error::UnknownFormat => cadkit_status::UnknownFormat,
        Error::Io(_) => cadkit_status::Io,
        _ => cadkit_status::Internal,
    };
    fail(status, &err.to_string())
}

/// Runs `f`, clearing the last error first and converting panics into a status.
fn guarded(f: impl FnOnce() -> cadkit_status) -> cadkit_status {
    clear_error();
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => status,
        Err(_) => fail(
            cadkit_status::Panic,
            "internal panic caught at the FFI boundary",
        ),
    }
}

fn into_c_string(s: String) -> CString {
    // JSON/SVG/DXF output should not contain NUL, but the C side cannot represent it.
    CString::new(s).unwrap_or_else(|e| {
        let cleaned: Vec<u8> = e.into_vec().into_iter().filter(|b| *b != 0).collect();
        CString::new(cleaned).unwrap_or_default()
    })
}

/// Reads an optional NUL-terminated UTF-8 string.
///
/// # Safety
/// `p` must be NULL or point to a NUL-terminated string that outlives `'a`.
unsafe fn opt_str<'a>(p: *const c_char, what: &str) -> Result<Option<&'a str>, cadkit_status> {
    if p.is_null() {
        return Ok(None);
    }
    // SAFETY: non-null and NUL-terminated per the function contract.
    let c = unsafe { CStr::from_ptr(p) };
    match c.to_str() {
        Ok(s) => Ok(Some(s)),
        Err(_) => Err(fail(
            cadkit_status::InvalidArgument,
            &format!("{what} is not valid UTF-8"),
        )),
    }
}

/// Converts C options to the facade's.
///
/// # Safety
/// `opts` must be NULL or point to a readable `cadkit_options` whose string is valid.
unsafe fn convert_options(opts: *const cadkit_options) -> Result<ReadOptions, cadkit_status> {
    let mut out = ReadOptions::default();
    // SAFETY: NULL is handled by `as_ref`; otherwise readable per the function contract.
    let Some(o) = (unsafe { opts.as_ref() }) else {
        return Ok(out);
    };
    let d = Limits::default();
    out.limits = Limits {
        max_input_bytes: if o.max_input_bytes == 0 {
            d.max_input_bytes
        } else {
            o.max_input_bytes
        },
        max_decompressed_bytes: if o.max_decompressed_bytes == 0 {
            d.max_decompressed_bytes
        } else {
            o.max_decompressed_bytes
        },
        max_objects: if o.max_objects == 0 {
            d.max_objects
        } else {
            o.max_objects
        },
        max_depth: if o.max_depth == 0 {
            d.max_depth
        } else {
            o.max_depth
        },
        max_string_bytes: if o.max_string_bytes == 0 {
            d.max_string_bytes
        } else {
            o.max_string_bytes
        },
        max_vertices: if o.max_vertices == 0 {
            d.max_vertices
        } else {
            o.max_vertices
        },
        // Not exposed in the C options struct yet (keeps the ABI stable); default applies.
        max_total_vertices: d.max_total_vertices,
    };
    out.keep_raw = o.keep_raw != 0;
    // SAFETY: the string pointer is NULL or NUL-terminated per the function contract.
    out.fallback_codepage =
        unsafe { opt_str(o.fallback_codepage, "fallback_codepage") }?.map(str::to_owned);
    Ok(out)
}

fn to_c_format(f: Option<Format>) -> cadkit_format {
    match f {
        Some(Format::Dwg) => cadkit_format::Dwg,
        Some(Format::Dxf) => cadkit_format::Dxf,
        Some(Format::DgnV7) => cadkit_format::DgnV7,
        Some(Format::DgnV8) => cadkit_format::DgnV8,
        Some(Format::CityGml) => cadkit_format::CityGml,
        _ => cadkit_format::Unknown,
    }
}

/// Boxes `doc`, records the handle as live and stores it in `*out`.
///
/// # Safety
/// `out` must be non-null and writable.
unsafe fn register(doc: Document, out: *mut *mut cadkit_document) {
    let ptr = Box::into_raw(Box::new(cadkit_document { doc }));
    live_handles().insert(ptr as usize);
    // SAFETY: `out` is non-null and writable per the function contract.
    unsafe { *out = ptr };
}

/// Looks up a live document.
///
/// # Safety
/// `doc` must be NULL, a pointer returned by `cadkit_read_*` and not yet freed, or a stale
/// pointer (which is rejected through the registry without being dereferenced).
unsafe fn doc_ref<'a>(doc: *const cadkit_document) -> Result<&'a Document, cadkit_status> {
    if doc.is_null() {
        return Err(fail(cadkit_status::NullPointer, "document is NULL"));
    }
    if !live_handles().contains(&(doc as usize)) {
        return Err(fail(
            cadkit_status::InvalidHandle,
            "document handle is not live (already freed or never created)",
        ));
    }
    // SAFETY: the registry proves `doc` came from `Box::into_raw` and has not been freed.
    Ok(unsafe { &(*doc).doc })
}

/// Shared body of the string-returning getters.
///
/// # Safety
/// `doc` must satisfy [`doc_ref`]'s contract; `out` must be NULL or writable.
unsafe fn string_getter(
    doc: *const cadkit_document,
    out: *mut *mut c_char,
    f: impl FnOnce(&Document) -> Result<String, Error>,
) -> cadkit_status {
    if !out.is_null() {
        // SAFETY: non-null and writable per the function contract.
        unsafe { *out = std::ptr::null_mut() };
    }
    guarded(|| {
        if out.is_null() {
            return fail(cadkit_status::NullPointer, "out pointer is NULL");
        }
        // SAFETY: handle validity is checked by `doc_ref`.
        let d = match unsafe { doc_ref(doc) } {
            Ok(d) => d,
            Err(s) => return s,
        };
        match f(d) {
            Ok(s) => {
                // SAFETY: `out` is non-null and writable per the function contract.
                unsafe { *out = into_c_string(s).into_raw() };
                cadkit_status::Ok
            }
            Err(e) => map_error(&e),
        }
    })
}

// ---------------------------------------------------------------------------------------------
// public API

/// Returns the library version, e.g. `"0.1.0"`. Static; never NULL; do not free.
#[unsafe(no_mangle)]
pub extern "C" fn cadkit_version() -> *const c_char {
    VERSION.as_ptr()
}

/// Returns a static description of a `cadkit_status` value. Never NULL; do not free.
#[unsafe(no_mangle)]
pub extern "C" fn cadkit_status_string(status: i32) -> *const c_char {
    // `i32` instead of the enum: an out-of-range value from C would be undefined behavior.
    let s: &CStr = match status {
        0 => c"ok",
        1 => c"null pointer argument",
        2 => c"invalid argument",
        3 => c"I/O error",
        4 => c"truncated input",
        5 => c"invalid data",
        6 => c"unsupported feature",
        7 => c"limit exceeded",
        8 => c"unrecognized file format",
        9 => c"internal panic",
        10 => c"internal error",
        11 => c"invalid document handle",
        _ => c"unknown status",
    };
    s.as_ptr()
}

/// Returns the message of the last failed call on this thread, or an empty string.
///
/// The pointer is owned by the library and valid until the next cadkit call on the same
/// thread. Never NULL.
#[unsafe(no_mangle)]
pub extern "C" fn cadkit_last_error_message() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// Fills `options` with defaults.
///
/// # Safety
/// `options` must be NULL or point to writable memory for one `cadkit_options`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_options_init(options: *mut cadkit_options) {
    // SAFETY: NULL is handled by `as_mut`; otherwise writable per the contract.
    if let Some(o) = unsafe { options.as_mut() } {
        *o = cadkit_options {
            max_input_bytes: 0,
            max_decompressed_bytes: 0,
            max_objects: 0,
            max_depth: 0,
            max_string_bytes: 0,
            max_vertices: 0,
            keep_raw: 0,
            fallback_codepage: std::ptr::null(),
        };
    }
}

/// Fills `options` with defaults (model 0, 1600 px wide, white background, text and blocks on).
///
/// # Safety
/// `options` must be NULL or point to writable memory for one `cadkit_svg_options`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_svg_options_init(options: *mut cadkit_svg_options) {
    let d = SvgOptions::default();
    // SAFETY: NULL is handled by `as_mut`; otherwise writable per the contract.
    if let Some(o) = unsafe { options.as_mut() } {
        *o = cadkit_svg_options {
            model_index: 0,
            width_px: d.width_px,
            stroke_px: d.stroke_px,
            background: c"#ffffff".as_ptr(),
            text: 1,
            expand_blocks: 1,
        };
    }
}

/// Detects the format from the header. Returns `CADKIT_FORMAT_UNKNOWN` for NULL data.
///
/// # Safety
/// `data` must be NULL or point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_detect(data: *const u8, len: usize) -> cadkit_format {
    if data.is_null() {
        return cadkit_format::Unknown;
    }
    // SAFETY: non-null and `len` readable bytes per the contract.
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    catch_unwind(|| to_c_format(cadkit::detect(bytes))).unwrap_or(cadkit_format::Unknown)
}

/// Reads a drawing from a UTF-8 file path. On success `*out` owns a document to free with
/// [`cadkit_document_free`]; on failure `*out` is NULL.
///
/// # Safety
/// `utf8_path` must be NULL or NUL-terminated; `options` NULL or valid; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_read_file(
    utf8_path: *const c_char,
    options: *const cadkit_options,
    out: *mut *mut cadkit_document,
) -> cadkit_status {
    if !out.is_null() {
        // SAFETY: non-null and writable per the contract.
        unsafe { *out = std::ptr::null_mut() };
    }
    guarded(|| {
        if out.is_null() || utf8_path.is_null() {
            return fail(cadkit_status::NullPointer, "path or out pointer is NULL");
        }
        // SAFETY: `utf8_path` is non-null and NUL-terminated per the contract.
        let path = match unsafe { opt_str(utf8_path, "path") } {
            Ok(Some(p)) => p,
            Ok(None) => return fail(cadkit_status::NullPointer, "path is NULL"),
            Err(s) => return s,
        };
        // SAFETY: `options` is NULL or valid per the contract.
        let opts = match unsafe { convert_options(options) } {
            Ok(o) => o,
            Err(s) => return s,
        };
        match cadkit::open_with(path, &opts) {
            Ok(doc) => {
                // SAFETY: `out` was checked non-null above and is writable per the contract.
                unsafe { register(doc, out) };
                cadkit_status::Ok
            }
            Err(e) => map_error(&e),
        }
    })
}

/// Reads a drawing from memory. The buffer is not retained.
///
/// # Safety
/// `data` must point to `len` readable bytes (it may be NULL only when `len` is 0);
/// `options` NULL or valid; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_read_bytes(
    data: *const u8,
    len: usize,
    options: *const cadkit_options,
    out: *mut *mut cadkit_document,
) -> cadkit_status {
    if !out.is_null() {
        // SAFETY: non-null and writable per the contract.
        unsafe { *out = std::ptr::null_mut() };
    }
    guarded(|| {
        if out.is_null() {
            return fail(cadkit_status::NullPointer, "out pointer is NULL");
        }
        let bytes: &[u8] = if data.is_null() {
            if len != 0 {
                return fail(cadkit_status::NullPointer, "data is NULL but len is not 0");
            }
            &[]
        } else {
            // SAFETY: non-null and `len` readable bytes per the contract.
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        // SAFETY: `options` is NULL or valid per the contract.
        let opts = match unsafe { convert_options(options) } {
            Ok(o) => o,
            Err(s) => return s,
        };
        match cadkit::read_with(bytes, &opts) {
            Ok(doc) => {
                // SAFETY: `out` was checked non-null above and is writable per the contract.
                unsafe { register(doc, out) };
                cadkit_status::Ok
            }
            Err(e) => map_error(&e),
        }
    })
}

/// Releases a document. NULL is a no-op; a handle that is not live returns
/// `CADKIT_INVALID_HANDLE` and frees nothing. The handle must not be used afterwards.
///
/// # Safety
/// `doc` must be NULL or a handle from `cadkit_read_*`; no other thread may use it concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_free(doc: *mut cadkit_document) -> cadkit_status {
    if doc.is_null() {
        return cadkit_status::Ok;
    }
    guarded(|| {
        if !live_handles().remove(&(doc as usize)) {
            return fail(
                cadkit_status::InvalidHandle,
                "document handle is not live (double free?)",
            );
        }
        // SAFETY: the registry proved `doc` came from `Box::into_raw` and removed it, so this is
        // the single owner releasing it.
        drop(unsafe { Box::from_raw(doc) });
        cadkit_status::Ok
    })
}

/// Fills `out` with a summary of `doc`.
///
/// # Safety
/// `doc` must be a live handle; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_info(
    doc: *const cadkit_document,
    out: *mut cadkit_info,
) -> cadkit_status {
    guarded(|| {
        if out.is_null() {
            return fail(cadkit_status::NullPointer, "out pointer is NULL");
        }
        // SAFETY: handle validity is checked by `doc_ref`.
        let d = match unsafe { doc_ref(doc) } {
            Ok(d) => d,
            Err(s) => return s,
        };
        let info = cadkit_info {
            format: to_c_format(Some(d.source.format)),
            model_count: d.models.len() as u64,
            layer_count: d.layers.len() as u64,
            block_count: d.blocks.len() as u64,
            entity_count: d.models.iter().map(|m| m.entities.len() as u64).sum(),
            warning_count: d.warnings.len() as u64,
        };
        // SAFETY: `out` is non-null and writable per the contract.
        unsafe { *out = info };
        cadkit_status::Ok
    })
}

/// Returns the summary as a JSON object (format, version, application, counts, per-kind entity
/// counts). `*out` is an owned string; free with [`cadkit_string_free`].
///
/// # Safety
/// `doc` must be a live handle; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_info_json(
    doc: *const cadkit_document,
    out: *mut *mut c_char,
) -> cadkit_status {
    // SAFETY: forwarded contract.
    unsafe {
        string_getter(doc, out, |d| {
            let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
            for e in d.models.iter().flat_map(|m| m.entities.iter()) {
                let name = e.kind.type_name().to_owned();
                *kinds.entry(name).or_insert(0) += 1;
            }
            let v = serde_json::json!({
                "format": d.source.format,
                "version": d.source.version,
                "application": d.source.application,
                "models": d.models.len(),
                "layers": d.layers.len(),
                "blocks": d.blocks.len(),
                "entities": d.models.iter().map(|m| m.entities.len()).sum::<usize>(),
                "warnings": d.warnings.len(),
                "entity_kinds": kinds,
            });
            Ok(v.to_string())
        })
    }
}

/// Serializes the document to JSON (UTF-8). `*out` is owned; free with [`cadkit_string_free`].
///
/// # Safety
/// `doc` must be a live handle; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_to_json(
    doc: *const cadkit_document,
    pretty: i32,
    out: *mut *mut c_char,
) -> cadkit_status {
    // SAFETY: forwarded contract.
    unsafe { string_getter(doc, out, |d| cadkit::to_json(d, pretty != 0)) }
}

/// Converts C SVG options to the facade's, validating them.
///
/// # Safety
/// `options` must be NULL or readable with a valid `background` string.
unsafe fn convert_svg_options(
    options: *const cadkit_svg_options,
) -> Result<SvgOptions, cadkit_status> {
    // SAFETY: NULL is handled by `as_ref`; otherwise readable per the function contract.
    let Some(o) = (unsafe { options.as_ref() }) else {
        return Ok(SvgOptions::default());
    };
    // SAFETY: the background pointer is NULL or NUL-terminated per the function contract.
    let bg = unsafe { opt_str(o.background, "background") }?;
    if !(o.width_px.is_finite() && o.width_px > 0.0 && o.stroke_px.is_finite() && o.stroke_px > 0.0)
    {
        return Err(fail(
            cadkit_status::InvalidArgument,
            "width_px and stroke_px must be finite and > 0",
        ));
    }
    Ok(SvgOptions {
        model: o.model_index as usize,
        width_px: o.width_px,
        stroke_px: o.stroke_px,
        background: bg.map(str::to_owned),
        text: o.text != 0,
        expand_blocks: o.expand_blocks != 0,
        ..SvgOptions::default()
    })
}

/// Renders one model as SVG. `options` may be NULL for defaults.
///
/// # Safety
/// `doc` must be a live handle; `options` NULL or valid; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_to_svg(
    doc: *const cadkit_document,
    options: *const cadkit_svg_options,
    out: *mut *mut c_char,
) -> cadkit_status {
    clear_error();
    // SAFETY: `options` is NULL or valid per the contract.
    match unsafe { convert_svg_options(options) } {
        // SAFETY: forwarded contract.
        Ok(opts) => unsafe { string_getter(doc, out, |d| Ok(cadkit::to_svg(d, &opts))) },
        Err(status) => {
            if !out.is_null() {
                // SAFETY: non-null and writable per the contract.
                unsafe { *out = std::ptr::null_mut() };
            }
            status
        }
    }
}

/// Writes the document as ASCII DXF of the given `cadkit_dxf_version`.
///
/// # Safety
/// `doc` must be a live handle; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_to_dxf(
    doc: *const cadkit_document,
    version: i32,
    out: *mut *mut c_char,
) -> cadkit_status {
    // `i32` instead of the enum: an out-of-range value from C would be undefined behavior.
    let v = match version {
        0 => DxfVersion::R12,
        1 => DxfVersion::R2000,
        2 => DxfVersion::R2004,
        3 => DxfVersion::R2007,
        4 => DxfVersion::R2010,
        5 => DxfVersion::R2013,
        6 => DxfVersion::R2018,
        _ => {
            if !out.is_null() {
                // SAFETY: non-null and writable per the contract.
                unsafe { *out = std::ptr::null_mut() };
            }
            return fail(cadkit_status::InvalidArgument, "unknown DXF version");
        }
    };
    // SAFETY: forwarded contract.
    unsafe { string_getter(doc, out, |d| cadkit::to_dxf(d, v)) }
}

/// Frees a string returned by a `cadkit_document_*` getter. NULL is a no-op.
///
/// # Safety
/// `s` must be NULL or a string returned by this library that has not been freed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: per the contract `s` came from `CString::into_raw` in this library and is freed once.
    drop(unsafe { CString::from_raw(s) });
}

fn live_buffers() -> MutexGuard<'static, std::collections::HashMap<usize, Vec<u8>>> {
    static BUFFERS: OnceLock<Mutex<std::collections::HashMap<usize, Vec<u8>>>> = OnceLock::new();
    BUFFERS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// İkili çıktı belleğini serbest bırakır; NULL ve daha önce bırakılmış adresler etkisizdir.
#[unsafe(no_mangle)]
pub extern "C" fn cadkit_bytes_free(data: *mut u8) {
    live_buffers().remove(&(data as usize));
}

unsafe fn options_json<T: serde::de::DeserializeOwned + Default>(
    text: *const c_char,
) -> Result<T, cadkit_status> {
    // SAFETY: Çağıran işlev metnin NULL veya geçerli C dizesi olmasını şart koşar.
    match unsafe { opt_str(text, "options_json") }? {
        None => Ok(T::default()),
        Some(s) => serde_json::from_str(s)
            .map_err(|e| fail(cadkit_status::InvalidArgument, &e.to_string())),
    }
}

unsafe fn binary_getter(
    doc: *const cadkit_document,
    out: *mut *mut u8,
    out_len: *mut usize,
    f: impl FnOnce(&Document) -> Result<Vec<u8>, Error>,
) -> cadkit_status {
    // SAFETY: Çıktı adresleri NULL veya yazılabilir olmalıdır.
    unsafe {
        if !out.is_null() {
            *out = std::ptr::null_mut();
        }
        if !out_len.is_null() {
            *out_len = 0;
        }
    }
    guarded(|| {
        if out.is_null() || out_len.is_null() {
            return fail(cadkit_status::NullPointer, "binary output pointer is NULL");
        }
        // SAFETY: Kayıt tablosu belge adresini dereference öncesinde denetler.
        let d = match unsafe { doc_ref(doc) } {
            Ok(d) => d,
            Err(s) => return s,
        };
        match f(d) {
            Ok(mut bytes) => {
                let len = bytes.len();
                let ptr = bytes.as_mut_ptr();
                live_buffers().insert(ptr as usize, bytes);
                // SAFETY: Her iki çıktı adresi de yukarıda NULL denetiminden geçti.
                unsafe {
                    *out = ptr;
                    *out_len = len;
                }
                cadkit_status::Ok
            }
            Err(e) => map_error(&e),
        }
    })
}

/// Yeni nötr belgeyi JSON'dan oluşturur.
///
/// # Safety
/// `json` geçerli C dizesi, `out` yazılabilir adres olmalıdır.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_from_json(
    json: *const c_char,
    out: *mut *mut cadkit_document,
) -> cadkit_status {
    // SAFETY: NULL olmayan çıktı adresi sözleşme gereği yazılabilirdir.
    unsafe {
        if !out.is_null() {
            *out = std::ptr::null_mut();
        }
    }
    guarded(|| {
        if out.is_null() {
            return fail(cadkit_status::NullPointer, "out is NULL");
        }
        // SAFETY: Girdi NULL veya sonlandırılmış C dizesidir.
        let text = match unsafe { opt_str(json, "json") } {
            Ok(Some(s)) => s,
            Ok(None) => return fail(cadkit_status::NullPointer, "json is NULL"),
            Err(s) => return s,
        };
        match cadkit::export::document_from_json(text, &cadkit::Limits::default()) {
            Ok(doc) => {
                // SAFETY: Çıktı adresi denetlendi; oluşturulan belge kayıt tablosuna eklenir.
                unsafe {
                    register(doc, out);
                }
                cadkit_status::Ok
            }
            Err(e) => map_error(&e),
        }
    })
}

/// Seed ile DGN V8 üretir. Çıktıyı `cadkit_bytes_free` ile bırakın.
/// `options_json`, Rust `dgn::WriteOptions` sözleşmesidir; NULL varsayılanları seçer.
///
/// # Safety
/// Belge canlı olmalı; seed `seed_len` bayt okunabilir olmalı; seçenek dizesi geçerli,
/// çıktı adresleri yazılabilir olmalıdır. Belge eşzamanlı serbest bırakılamaz.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_to_dgn(
    doc: *const cadkit_document,
    seed: *const u8,
    seed_len: usize,
    options_json_text: *const c_char,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> cadkit_status {
    guarded(|| {
        // SAFETY: Çıktı adresleri NULL veya yazılabilir olmalıdır.
        unsafe {
            if !out.is_null() {
                *out = std::ptr::null_mut();
            }
            if !out_len.is_null() {
                *out_len = 0;
            }
        }
        if seed.is_null() || seed_len > isize::MAX as usize {
            return fail(cadkit_status::InvalidArgument, "invalid DGN seed buffer");
        }
        // SAFETY: Seçenek dizesi ve seed aralığı çağıranın sözleşmesiyle korunur.
        let options = match unsafe { options_json::<cadkit::dgn::WriteOptions>(options_json_text) }
        {
            Ok(o) => o,
            Err(s) => return s,
        };
        // SAFETY: Seed aralığı okunabilir ve dilim boyutu isize::MAX altında.
        let bytes = unsafe { std::slice::from_raw_parts(seed, seed_len) };
        // SAFETY: Belge/çıktı sözleşmesi binary_getter'a aktarılır.
        unsafe { binary_getter(doc, out, out_len, |d| cadkit::to_dgn(d, bytes, &options)) }
    })
}

/// Genel geometriyi CityGML UTF-8 dizesine yazar; çıktı `cadkit_string_free` ile bırakılır.
///
/// # Safety
/// Belge canlı, `srs_name` geçerli C dizesi, `out` yazılabilir olmalıdır.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_document_to_citygml(
    doc: *const cadkit_document,
    srs_name: *const c_char,
    lod: u8,
    out: *mut *mut c_char,
) -> cadkit_status {
    guarded(|| {
        // SAFETY: NULL olmayan çıktı yazılabilirdir; CRS sonlandırılmış C dizesidir.
        unsafe {
            if !out.is_null() {
                *out = std::ptr::null_mut();
            }
        }
        // SAFETY: CRS dizesinin geçerliliği çağıranın sözleşmesindedir.
        let srs = match unsafe { opt_str(srs_name, "srs_name") } {
            Ok(Some(s)) => s.to_owned(),
            Ok(None) => return fail(cadkit_status::InvalidArgument, "srs_name is required"),
            Err(s) => return s,
        };
        // SAFETY: Belge ve çıktı sözleşmesi string_getter'a aktarılır.
        unsafe {
            string_getter(doc, out, |d| {
                String::from_utf8(cadkit::to_citygml(
                    d,
                    &cadkit::gml::ExportOptions {
                        srs_name: srs,
                        lod,
                        limits: cadkit::Limits::default(),
                    },
                )?)
                .map_err(|e| Error::invalid(0, e.to_string()))
            })
        }
    })
}

/// CityGML'in özgün nesne ağını JSON olarak okur; çıktı `cadkit_string_free` ile bırakılır.
///
/// # Safety
/// `data` en az `len` bayt okunabilir, seçenekler NULL veya geçerli, çıktı yazılabilir olmalıdır.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_citygml_read_json(
    data: *const u8,
    len: usize,
    options: *const cadkit_options,
    out: *mut *mut c_char,
) -> cadkit_status {
    // SAFETY: NULL olmayan çıktı sözleşme gereği yazılabilirdir.
    unsafe {
        if !out.is_null() {
            *out = std::ptr::null_mut();
        }
    }
    guarded(|| {
        if data.is_null() || out.is_null() || len > isize::MAX as usize {
            return fail(
                cadkit_status::InvalidArgument,
                "invalid CityGML input/output buffer",
            );
        }
        // SAFETY: Seçenek adresi NULL veya geçerli bir yapıdır.
        let opts = match unsafe { convert_options(options) } {
            Ok(o) => o,
            Err(s) => return s,
        };
        // SAFETY: Girdi aralığı geçerli ve boyut isize::MAX altında.
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        let result = cadkit::gml::read_native(bytes, &opts)
            .and_then(|d| serde_json::to_string(&d).map_err(|e| Error::invalid(0, e.to_string())));
        match result {
            Ok(s) => {
                // SAFETY: Çıktı adresi denetlendi; sahiplik çağırana aktarılır.
                unsafe {
                    *out = into_c_string(s).into_raw();
                }
                cadkit_status::Ok
            }
            Err(e) => map_error(&e),
        }
    })
}

/// Özgün model JSON'undan CityGML üretir; seçenekler ValidationOptions JSON sözleşmesidir.
///
/// # Safety
/// JSON ve seçenek dizeleri geçerli, çıktı adresi yazılabilir olmalıdır.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadkit_citygml_write_json(
    json: *const c_char,
    validation_options: *const c_char,
    out: *mut *mut c_char,
) -> cadkit_status {
    // SAFETY: NULL olmayan çıktı adresi sözleşme gereği yazılabilirdir.
    unsafe {
        if !out.is_null() {
            *out = std::ptr::null_mut();
        }
    }
    guarded(|| {
        if out.is_null() {
            return fail(cadkit_status::NullPointer, "out is NULL");
        }
        // SAFETY: Girdi NULL veya sonlandırılmış C dizesidir.
        let text = match unsafe { opt_str(json, "json") } {
            Ok(Some(s)) => s,
            Ok(None) => return fail(cadkit_status::NullPointer, "json is NULL"),
            Err(s) => return s,
        };
        // SAFETY: Seçenekler NULL veya geçerli C dizesidir.
        let options =
            match unsafe { options_json::<cadkit::gml::ValidationOptions>(validation_options) } {
                Ok(o) => o,
                Err(s) => return s,
            };
        let result = cadkit::gml::read_native_json(
            text,
            &ReadOptions {
                limits: options.limits,
                ..Default::default()
            },
        )
        .and_then(|doc| cadkit::gml::write(&doc, &options))
        .and_then(|bytes| String::from_utf8(bytes).map_err(|e| Error::invalid(0, e.to_string())));
        match result {
            Ok(s) => {
                // SAFETY: Çıktı adresi denetlendi; dize sahipliği çağırana aktarılır.
                unsafe {
                    *out = into_c_string(s).into_raw();
                }
                cadkit_status::Ok
            }
            Err(e) => map_error(&e),
        }
    })
}
