//! DXF reader and writer (ASCII and binary) for the cadkit model.
//!
//! Contract used by the `cadkit` facade: [`sniff()`], [`read()`], [`write()`].
//!
//! * [`read`] accepts ASCII and binary DXF from R12 (AC1009) to 2018 (AC1032) and maps it to
//!   a [`cadkit_core::Document`]. Geometry of planar entities is converted to world
//!   coordinates; see `docs/dxf/NOTES.md` for every mapping decision.
//! * [`write()`] produces ASCII DXF in one of the [`DxfVersion`]s.
//! * [`native`] exposes the group-code tokenizer for scanners and oracles.

pub mod native;
mod ocs;
mod reader;
mod writer;

use cadkit_core::{Document, ReadOptions, Result, Summary, Warning};

/// DXF output version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DxfVersion {
    /// AutoCAD R12 (AC1009).
    R12,
    /// AutoCAD 2000 (AC1015).
    R2000,
    /// AutoCAD 2004 (AC1018).
    R2004,
    /// AutoCAD 2007 (AC1021).
    R2007,
    /// AutoCAD 2010 (AC1024).
    R2010,
    /// AutoCAD 2013 (AC1027).
    R2013,
    /// AutoCAD 2018 (AC1032).
    #[default]
    R2018,
}

impl DxfVersion {
    /// The `$ACADVER` string of this version (for example `"AC1015"`).
    pub fn acadver(self) -> &'static str {
        match self {
            DxfVersion::R12 => "AC1009",
            DxfVersion::R2000 => "AC1015",
            DxfVersion::R2004 => "AC1018",
            DxfVersion::R2007 => "AC1021",
            DxfVersion::R2010 => "AC1024",
            DxfVersion::R2013 => "AC1027",
            DxfVersion::R2018 => "AC1032",
        }
    }
}

/// Returns true when `bytes` look like DXF (ASCII or binary).
///
/// Only the first few groups are inspected: binary files by their sentinel, ASCII files by
/// `0` / `SECTION` (after optional `999` comments).
pub fn sniff(bytes: &[u8]) -> bool {
    native::sniff(bytes)
}

/// Inspects the DXF `HEADER` section without decoding tables, blocks, or entities.
pub fn inspect(bytes: &[u8], options: &ReadOptions) -> Result<Summary> {
    reader::inspect(bytes, options)
}

/// Reads a DXF file into the format-neutral model.
///
/// Fails only when no useful document can be produced (not DXF, limits exceeded, or no
/// section could be read); damaged or truncated files yield a partial document with
/// `dxf.truncated` warnings.
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    reader::read(bytes, options)
}

/// Options for DXF output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct WriteOptions {
    /// Target DXF release (`R2018` by default).
    pub version: DxfVersion,
    /// Optional legacy `$DWGCODEPAGE`, WHATWG, or OEM encoding label (for example
    /// `"ANSI_1254"`, `"windows-1254"`, `"DOS857"`, or `"cp857"`) for pre-R2007 output.
    /// Characters representable in that 8-bit encoding are emitted directly as raw bytes;
    /// unrepresentable characters remain `\U+XXXX` escapes.
    pub codepage: Option<String>,
}

impl WriteOptions {
    /// Creates DXF output options for `version`.
    #[must_use]
    pub fn new(version: DxfVersion) -> Self {
        Self {
            version,
            codepage: None,
        }
    }

    /// Sets the target DXF release.
    #[must_use]
    pub fn with_version(mut self, version: DxfVersion) -> Self {
        self.version = version;
        self
    }

    /// Sets an explicit legacy `$DWGCODEPAGE` / WHATWG / OEM output encoding for pre-R2007 DXF.
    #[must_use]
    pub fn with_codepage(mut self, codepage: Option<String>) -> Self {
        self.codepage = codepage;
        self
    }
}

/// Writes `doc` as ASCII/UTF-8 DXF text.
pub fn write(doc: &Document, version: DxfVersion) -> Result<String> {
    writer::write(doc, version).map(|(text, _)| text)
}

/// Like [`write()`], but also returns the warnings for everything that could not be
/// represented (skipped unknown entities, undefined block references, ...).
pub fn write_with_report(doc: &Document, version: DxfVersion) -> Result<(String, Vec<Warning>)> {
    writer::write(doc, version)
}

/// Writes `doc` as DXF bytes using `options` (honoring legacy 8-bit `codepage` on pre-R2007 versions).
pub fn write_bytes(doc: &Document, options: &WriteOptions) -> Result<Vec<u8>> {
    writer::write_bytes(doc, options).map(|(bytes, _)| bytes)
}

/// Like [`write_bytes()`], returning both the encoded DXF bytes and conversion warnings.
pub fn write_bytes_with_report(
    doc: &Document,
    options: &WriteOptions,
) -> Result<(Vec<u8>, Vec<Warning>)> {
    writer::write_bytes(doc, options)
}
