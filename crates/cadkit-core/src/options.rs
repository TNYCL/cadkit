//! Reader options and resource limits.

/// Hard resource limits that protect against malicious or corrupt input.
/// Every reader must check sizes taken from the file against these before allocating or looping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted input file.
    pub max_input_bytes: u64,
    /// Largest total of decompressed data per document.
    pub max_decompressed_bytes: u64,
    /// Most records/objects/elements per document.
    pub max_objects: u64,
    /// Deepest nesting (complex elements, cells, block references).
    pub max_depth: u32,
    /// Longest single string in bytes.
    pub max_string_bytes: u32,
    /// Most vertices / control points / knots in a single entity.
    pub max_vertices: u32,
    /// Most vertex-like items (vertices, control points, hatch edges, attributes) in the whole
    /// document. Guards against inputs that reference the same data many times.
    pub max_total_vertices: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 2 << 30,
            max_decompressed_bytes: 4 << 30,
            max_objects: 20_000_000,
            max_depth: 64,
            max_string_bytes: 16 << 20,
            max_vertices: 10_000_000,
            max_total_vertices: 100_000_000,
        }
    }
}

/// Options accepted by every reader.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReadOptions {
    /// Resource limits.
    pub limits: Limits,
    /// Keep original record bytes in [`crate::Entity::raw`].
    pub keep_raw: bool,
    /// Code page (WHATWG label, e.g. `"windows-1254"`) for 8-bit text when the file does not
    /// declare one. `None` uses the reader's default (windows-1252).
    pub fallback_codepage: Option<String>,
}
