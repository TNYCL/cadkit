#![no_main]
//! Format reader `dxf` fed with arbitrary bytes (sniff and full read).
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadkit_dxf::sniff(data);
    let _ = cadkit_dxf::read(data, &cadkit_fuzz::options());
});
