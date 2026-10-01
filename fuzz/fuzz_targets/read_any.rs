#![no_main]
//! Facade entry point: detection plus whichever reader matches.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadkit::detect(data);
    let _ = cadkit::read_with(data, &cadkit_fuzz::options());
});
