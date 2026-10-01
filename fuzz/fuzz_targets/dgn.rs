#![no_main]
//! Format reader `dgn` fed with arbitrary bytes (sniff and full read).
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadkit_dgn::sniff(data);
    let _ = cadkit_dgn::read(data, &cadkit_fuzz::options());
});
