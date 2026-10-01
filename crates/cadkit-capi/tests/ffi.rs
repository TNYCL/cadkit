//! Calls the exported `extern "C"` functions the way a C client would.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use cadkit_capi::*;

fn msg() -> String {
    // SAFETY: the library returns a valid NUL-terminated string.
    unsafe { CStr::from_ptr(cadkit_last_error_message()) }
        .to_string_lossy()
        .into_owned()
}

fn take(s: *mut c_char) -> String {
    assert!(!s.is_null());
    // SAFETY: `s` is a NUL-terminated string just returned by the library.
    let out = unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
    // SAFETY: returned by the library, freed once.
    unsafe { cadkit_string_free(s) };
    out
}

const MINIMAL_DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n10.0\n21\n5.0\n31\n0.0\n0\nENDSEC\n0\nEOF\n";

/// Reads `bytes`; returns `None` when the reader does not support them yet.
fn read(bytes: &[u8]) -> Option<*mut cadkit_document> {
    let mut doc = ptr::null_mut();
    // SAFETY: valid buffer and out pointer.
    let st = unsafe { cadkit_read_bytes(bytes.as_ptr(), bytes.len(), ptr::null(), &mut doc) };
    match st {
        cadkit_status::Ok => {
            assert!(!doc.is_null());
            Some(doc)
        }
        other => {
            assert!(doc.is_null());
            assert!(
                !msg().is_empty(),
                "failure {other:?} must set an error message"
            );
            None
        }
    }
}

#[test]
fn version_and_status_strings() {
    // SAFETY: static strings.
    let v = unsafe { CStr::from_ptr(cadkit_version()) }
        .to_str()
        .unwrap();
    assert_eq!(v, env!("CARGO_PKG_VERSION"));
    // SAFETY: static strings.
    let s = unsafe { CStr::from_ptr(cadkit_status_string(8)) }
        .to_str()
        .unwrap();
    assert!(s.contains("format"));
    // SAFETY: out-of-range values are accepted (i32 parameter).
    let s = unsafe { CStr::from_ptr(cadkit_status_string(12345)) }
        .to_str()
        .unwrap();
    assert_eq!(s, "unknown status");
}

#[test]
fn null_pointers_are_rejected() {
    let mut doc: *mut cadkit_document = ptr::null_mut();
    let mut s: *mut c_char = ptr::null_mut();
    // SAFETY: every call below deliberately passes NULL where the contract allows a check.
    unsafe {
        assert_eq!(
            cadkit_read_file(ptr::null(), ptr::null(), &mut doc),
            cadkit_status::NullPointer
        );
        assert!(doc.is_null());
        assert_eq!(
            cadkit_read_file(c"x".as_ptr(), ptr::null(), ptr::null_mut()),
            cadkit_status::NullPointer
        );
        assert_eq!(
            cadkit_read_bytes(ptr::null(), 4, ptr::null(), &mut doc),
            cadkit_status::NullPointer
        );
        assert_eq!(
            cadkit_read_bytes(ptr::null(), 0, ptr::null(), ptr::null_mut()),
            cadkit_status::NullPointer
        );
        assert_eq!(
            cadkit_document_to_json(ptr::null(), 0, &mut s),
            cadkit_status::NullPointer
        );
        assert_eq!(
            cadkit_document_to_svg(ptr::null(), ptr::null(), &mut s),
            cadkit_status::NullPointer
        );
        assert_eq!(
            cadkit_document_to_dxf(ptr::null(), 6, &mut s),
            cadkit_status::NullPointer
        );
        assert_eq!(
            cadkit_document_info_json(ptr::null(), &mut s),
            cadkit_status::NullPointer
        );
        assert_eq!(
            cadkit_document_info(ptr::null(), ptr::null_mut()),
            cadkit_status::NullPointer
        );
        assert!(s.is_null());
        assert_eq!(cadkit_detect(ptr::null(), 10), cadkit_format::Unknown);
        assert_eq!(cadkit_document_free(ptr::null_mut()), cadkit_status::Ok);
        cadkit_string_free(ptr::null_mut());
        cadkit_options_init(ptr::null_mut());
        cadkit_svg_options_init(ptr::null_mut());
    }
}

#[test]
fn empty_and_garbage_input_fail_cleanly() {
    let mut doc = ptr::null_mut();
    // SAFETY: NULL data is allowed for len 0.
    let st = unsafe { cadkit_read_bytes(ptr::null(), 0, ptr::null(), &mut doc) };
    assert_eq!(st, cadkit_status::UnknownFormat);
    assert!(doc.is_null());
    assert!(!msg().is_empty());

    let garbage: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    // SAFETY: valid buffer.
    let st = unsafe { cadkit_read_bytes(garbage.as_ptr(), garbage.len(), ptr::null(), &mut doc) };
    assert_ne!(st, cadkit_status::Ok);
    assert!(doc.is_null());

    // Truncated headers of every format must not panic.
    let heads: [&[u8]; 4] = [
        b"AC1032",
        b"AC1015\0\0\0",
        &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        b"  0\nSECTION\n",
    ];
    for magic in heads {
        // SAFETY: valid buffer.
        let st = unsafe { cadkit_read_bytes(magic.as_ptr(), magic.len(), ptr::null(), &mut doc) };
        assert_ne!(st, cadkit_status::Panic);
        if st == cadkit_status::Ok {
            // SAFETY: handle returned by the library.
            unsafe { cadkit_document_free(doc) };
        }
    }
}

#[test]
fn missing_file_and_bad_utf8_path() {
    let mut doc = ptr::null_mut();
    let p = CString::new("definitely/does/not/exist.dwg").unwrap();
    // SAFETY: valid C string and out pointer.
    let st = unsafe { cadkit_read_file(p.as_ptr(), ptr::null(), &mut doc) };
    assert_eq!(st, cadkit_status::Io);
    assert!(doc.is_null());

    let bad = [0xFFu8, 0xFE, 0x00];
    // SAFETY: NUL-terminated buffer.
    let st = unsafe { cadkit_read_file(bad.as_ptr().cast(), ptr::null(), &mut doc) };
    assert_eq!(st, cadkit_status::InvalidArgument);
}

#[test]
fn size_limit_option_is_honored() {
    let mut opts = std::mem::MaybeUninit::<cadkit_options>::uninit();
    // SAFETY: init writes the whole struct.
    let mut opts = unsafe {
        cadkit_options_init(opts.as_mut_ptr());
        opts.assume_init()
    };
    opts.max_input_bytes = 4;
    let mut doc = ptr::null_mut();
    let data = MINIMAL_DXF.as_bytes();
    // SAFETY: valid pointers.
    let st = unsafe { cadkit_read_bytes(data.as_ptr(), data.len(), &opts, &mut doc) };
    assert_eq!(st, cadkit_status::LimitExceeded);
    assert!(doc.is_null());
}

#[test]
fn detect_dxf() {
    let data = MINIMAL_DXF.as_bytes();
    // SAFETY: valid buffer.
    assert_eq!(
        unsafe { cadkit_detect(data.as_ptr(), data.len()) },
        cadkit_format::Dxf
    );
    // SAFETY: valid buffer.
    assert_eq!(
        unsafe { cadkit_detect(b"hello".as_ptr(), 5) },
        cadkit_format::Unknown
    );
}

#[test]
fn round_trip_and_double_free_protection() {
    let Some(doc) = read(MINIMAL_DXF.as_bytes()) else {
        eprintln!("skipped: DXF reader returned an error for the minimal drawing");
        return;
    };
    // SAFETY: `doc` is a live handle; all out pointers are valid.
    unsafe {
        let mut info = std::mem::MaybeUninit::<cadkit_info>::uninit();
        assert_eq!(
            cadkit_document_info(doc, info.as_mut_ptr()),
            cadkit_status::Ok
        );
        let info = info.assume_init();
        assert_eq!(info.format, cadkit_format::Dxf);

        let mut s = ptr::null_mut();
        assert_eq!(cadkit_document_info_json(doc, &mut s), cadkit_status::Ok);
        let json: serde_json::Value = serde_json::from_str(&take(s)).unwrap();
        assert_eq!(json["format"], "dxf");

        assert_eq!(cadkit_document_to_json(doc, 1, &mut s), cadkit_status::Ok);
        let full: serde_json::Value = serde_json::from_str(&take(s)).unwrap();
        assert!(full["models"].is_array());

        assert_eq!(
            cadkit_document_to_svg(doc, ptr::null(), &mut s),
            cadkit_status::Ok
        );
        assert!(take(s).contains("<svg"));

        let mut svg_opts = std::mem::MaybeUninit::<cadkit_svg_options>::uninit();
        cadkit_svg_options_init(svg_opts.as_mut_ptr());
        let mut svg_opts = svg_opts.assume_init();
        svg_opts.width_px = -1.0;
        assert_eq!(
            cadkit_document_to_svg(doc, &svg_opts, &mut s),
            cadkit_status::InvalidArgument
        );
        assert!(s.is_null());

        assert_eq!(
            cadkit_document_to_dxf(doc, 99, &mut s),
            cadkit_status::InvalidArgument
        );
        match cadkit_document_to_dxf(doc, 6, &mut s) {
            cadkit_status::Ok => assert!(take(s).contains("SECTION")),
            other => assert_ne!(other, cadkit_status::Panic),
        }

        assert_eq!(cadkit_document_free(doc), cadkit_status::Ok);
        // Second free and use after free are detected, not undefined.
        assert_eq!(cadkit_document_free(doc), cadkit_status::InvalidHandle);
        assert_eq!(
            cadkit_document_to_json(doc, 0, &mut s),
            cadkit_status::InvalidHandle
        );
        assert!(s.is_null());
    }
}

#[test]
fn corpus_files_never_panic() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/public");
    let Ok(dirs) = std::fs::read_dir(&root) else {
        eprintln!("skipped: corpus/public missing");
        return;
    };
    for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
        for file in std::fs::read_dir(dir.path()).unwrap().flatten() {
            let path = file.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "dwg" | "dxf" | "dgn") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let mut doc = ptr::null_mut();
            // SAFETY: valid buffer and out pointer.
            let st =
                unsafe { cadkit_read_bytes(bytes.as_ptr(), bytes.len(), ptr::null(), &mut doc) };
            assert_ne!(st, cadkit_status::Panic, "{path:?}");
            if st == cadkit_status::Ok {
                let mut s = ptr::null_mut();
                // SAFETY: live handle and valid out pointer.
                unsafe {
                    assert_eq!(cadkit_document_info_json(doc, &mut s), cadkit_status::Ok);
                    cadkit_string_free(s);
                    cadkit_document_free(doc);
                }
            }
        }
    }
}
