//! Shared helpers for the cadkit fuzz targets.

use cadkit_core::{Limits, ReadOptions};

/// Options with limits tight enough to keep each fuzz iteration fast and memory-bounded
/// (libFuzzer's default RSS limit is 2 GB), while still exercising every code path.
pub fn options() -> ReadOptions {
    ReadOptions::new().with_limits(
        Limits::new()
            .with_max_input_bytes(8 << 20)
            .with_max_decompressed_bytes(64 << 20)
            .with_max_objects(500_000)
            .with_max_depth(32)
            .with_max_string_bytes(1 << 20)
            .with_max_vertices(500_000)
            .with_max_total_vertices(2_000_000),
    )
}
