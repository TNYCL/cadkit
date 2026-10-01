//! Shared helpers for the cadkit fuzz targets.

use cadkit_core::{Limits, ReadOptions};

/// Options with limits tight enough to keep each fuzz iteration fast and memory-bounded
/// (libFuzzer's default RSS limit is 2 GB), while still exercising every code path.
pub fn options() -> ReadOptions {
    ReadOptions {
        limits: Limits {
            max_input_bytes: 8 << 20,
            max_decompressed_bytes: 64 << 20,
            max_objects: 500_000,
            max_depth: 32,
            max_string_bytes: 1 << 20,
            max_vertices: 500_000,
            max_total_vertices: 2_000_000,
        },
        ..ReadOptions::default()
    }
}
