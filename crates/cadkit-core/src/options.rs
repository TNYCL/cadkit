//! Reader options and resource limits.

/// Hard resource limits that protect against malicious or corrupt input.
/// Every reader must check sizes taken from the file against these before allocating or looping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[non_exhaustive]
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
        Self::new()
    }
}

impl Limits {
    /// The default limits (2 GiB input, 4 GiB decompressed, 20M objects, nesting depth 64).
    pub const fn new() -> Self {
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

    /// Tighter resource limits suitable for untrusted multi-tenant web or API inputs
    /// (64 MiB input, 256 MiB decompressed, 500k objects, max nesting depth 32).
    pub const fn strict() -> Self {
        Self {
            max_input_bytes: 64 << 20,
            max_decompressed_bytes: 256 << 20,
            max_objects: 500_000,
            max_depth: 32,
            max_string_bytes: 4 << 20,
            max_vertices: 1_000_000,
            max_total_vertices: 5_000_000,
        }
    }

    /// Sets [`Limits::max_input_bytes`].
    pub const fn with_max_input_bytes(mut self, bytes: u64) -> Self {
        self.max_input_bytes = bytes;
        self
    }

    /// Sets [`Limits::max_decompressed_bytes`].
    pub const fn with_max_decompressed_bytes(mut self, bytes: u64) -> Self {
        self.max_decompressed_bytes = bytes;
        self
    }

    /// Sets [`Limits::max_objects`].
    pub const fn with_max_objects(mut self, count: u64) -> Self {
        self.max_objects = count;
        self
    }

    /// Sets [`Limits::max_depth`].
    pub const fn with_max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth;
        self
    }

    /// Sets [`Limits::max_string_bytes`].
    pub const fn with_max_string_bytes(mut self, bytes: u32) -> Self {
        self.max_string_bytes = bytes;
        self
    }

    /// Sets [`Limits::max_vertices`].
    pub const fn with_max_vertices(mut self, count: u32) -> Self {
        self.max_vertices = count;
        self
    }

    /// Sets [`Limits::max_total_vertices`].
    pub const fn with_max_total_vertices(mut self, count: u64) -> Self {
        self.max_total_vertices = count;
        self
    }
}

/// Options accepted by every reader.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ReadOptions {
    /// Resource limits.
    pub limits: Limits,
    /// Keep original record bytes in [`crate::Entity::raw`].
    pub keep_raw: bool,
    /// Code page (WHATWG label, e.g. `"windows-1254"`) for 8-bit text when the file does not
    /// declare one. `None` uses the reader's default (windows-1252).
    pub fallback_codepage: Option<String>,
}

impl ReadOptions {
    /// Creates default reader options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides the resource [`Limits`].
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Configures whether original native record bytes are retained in [`crate::Entity::raw`].
    pub fn with_keep_raw(mut self, keep_raw: bool) -> Self {
        self.keep_raw = keep_raw;
        self
    }

    /// Sets the fallback 8-bit code page label (for example `"windows-1254"` or `"cp857"`).
    pub fn with_fallback_codepage(mut self, codepage: impl Into<String>) -> Self {
        self.fallback_codepage = Some(codepage.into());
        self
    }
}
