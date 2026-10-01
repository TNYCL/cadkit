#![no_main]
//! Format reader `dwg` fed with arbitrary bytes (sniff and full read).
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadkit_dwg::sniff(data);
    let _ = cadkit_dwg::read(data, &cadkit_fuzz::options());
});
