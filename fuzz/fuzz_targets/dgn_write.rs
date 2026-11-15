//! Resource-bounded fuzzing for DGN V8 reading, writing, seed stripping and CFB repacking.
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
        let write = cadkit_dgn::WriteOptions::new()
            .with_limits(options.limits)
            .with_clear_seed_model(true);
        let _ = cadkit_dgn::write_v8(&doc, data, &write);
        let _ = cadkit_dgn::repack_v8(data, &options);
        let strip = cadkit_dgn::StripSeedOptions::new()
            .with_keep_controls(data.len().is_multiple_of(2))
            .with_keep_raster_template(data.len().is_multiple_of(3));
        if let Ok(stripped) = cadkit_dgn::strip_seed_v8(data, &strip, &options) {
            // A stripped seed is empty, so the document goes in without clearing it.
            let _ = cadkit_dgn::write_v8(&doc, &stripped.bytes, &write.with_clear_seed_model(false));
        }
    }
});
