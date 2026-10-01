#![no_main]
//! read -> write -> read must never panic, for every DXF output version.
use cadkit_dxf::DxfVersion;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let options = cadkit_fuzz::options();
    let Ok(doc) = cadkit_dxf::read(data, &options) else {
        return;
    };
    let version = match data.first().copied().unwrap_or(0) % 3 {
        0 => DxfVersion::R12,
        1 => DxfVersion::R2000,
        _ => DxfVersion::R2018,
    };
    if let Ok(text) = cadkit_dxf::write(&doc, version) {
        let _ = cadkit_dxf::read(text.as_bytes(), &options);
    }
});
