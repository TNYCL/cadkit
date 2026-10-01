//! Read DWG, DGN and DXF drawings into one format-neutral model.
//!
//! ```no_run
//! let doc = cadkit::open("plan.dgn")?;
//! for model in &doc.models {
//!     println!("{}: {} entities", model.name, model.entities.len());
//! }
//! # Ok::<(), cadkit::Error>(())
//! ```
//!
//! Native, lossless APIs of each format are available under [`dgn`], [`dwg`] and [`dxf`].

use std::path::Path;

pub use cadkit_core::svg::SvgOptions;
pub use cadkit_core::*;
pub use cadkit_dgn as dgn;
pub use cadkit_dwg as dwg;
pub use cadkit_dxf as dxf;
pub use cadkit_dxf::DxfVersion;

/// Detects the format of `bytes` from its header.
pub fn detect(bytes: &[u8]) -> Option<Format> {
    if cadkit_dwg::sniff(bytes) {
        Some(Format::Dwg)
    } else if cadkit_dgn::sniff(bytes) {
        // The DGN reader tells V7 from V8 itself; report the container family here.
        Some(if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
            Format::DgnV8
        } else {
            Format::DgnV7
        })
    } else if cadkit_dxf::sniff(bytes) {
        Some(Format::Dxf)
    } else {
        None
    }
}

/// Reads a drawing of any supported format with default options.
pub fn read(bytes: &[u8]) -> Result<Document> {
    read_with(bytes, &ReadOptions::default())
}

/// Reads a drawing of any supported format.
pub fn read_with(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    if bytes.len() as u64 > options.limits.max_input_bytes {
        return Err(Error::LimitExceeded(format!(
            "input is {} bytes, limit is {}",
            bytes.len(),
            options.limits.max_input_bytes
        )));
    }
    match detect(bytes) {
        Some(Format::Dwg) => cadkit_dwg::read(bytes, options),
        Some(Format::DgnV7 | Format::DgnV8) => cadkit_dgn::read(bytes, options),
        Some(Format::Dxf) => cadkit_dxf::read(bytes, options),
        _ => Err(Error::UnknownFormat),
    }
}

/// Reads a drawing file from disk with default options.
pub fn open(path: impl AsRef<Path>) -> Result<Document> {
    open_with(path, &ReadOptions::default())
}

/// Reads a drawing file from disk.
pub fn open_with(path: impl AsRef<Path>, options: &ReadOptions) -> Result<Document> {
    use std::io::Read as _;
    let limit = options.limits.max_input_bytes;
    let too_large =
        |len: u64| Error::LimitExceeded(format!("input is {len} bytes, limit is {limit}"));
    let file = std::fs::File::open(path)?;
    // Check the size before reading so a huge file is never loaded into memory; the
    // `take` guards against files that grow (or lie about their size) while being read.
    let len = file.metadata()?.len();
    if len > limit {
        return Err(too_large(len));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large(bytes.len() as u64));
    }
    read_with(&bytes, options)
}

/// Serializes a document to JSON.
pub fn to_json(doc: &Document, pretty: bool) -> Result<String> {
    let out = if pretty {
        serde_json::to_string_pretty(doc)
    } else {
        serde_json::to_string(doc)
    };
    out.map_err(|e| Error::Unsupported(format!("JSON serialization failed: {e}")))
}

/// Renders a document model as SVG.
pub fn to_svg(doc: &Document, options: &SvgOptions) -> String {
    cadkit_core::svg::render(doc, options)
}

/// Writes a document as ASCII DXF.
pub fn to_dxf(doc: &Document, version: DxfVersion) -> Result<String> {
    cadkit_dxf::write(doc, version)
}
