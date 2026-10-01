//! Sınırlı kaynaklarla rastgele girdi ve yeniden yazma denemeleri.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let mut options = cadkit_core::ReadOptions::default();
    options.limits.max_input_bytes = 1 << 20;
    options.limits.max_decompressed_bytes = 4 << 20;
    options.limits.max_objects = 10_000;
    options.limits.max_vertices = 10_000;
    options.limits.max_total_vertices = 20_000;
    options.limits.max_depth = 32;
    if let Ok(doc) = cadkit_dgn::read(data, &options) {
        let write = cadkit_dgn::WriteOptions {
            limits: options.limits,
            clear_seed_model: true,
            ..Default::default()
        };
        let _ = cadkit_dgn::write_v8(&doc, data, &write);
        let _ = cadkit_dgn::repack_v8(data, &options);
    }
});
