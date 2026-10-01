//! Clean-room reader for DWG drawings, format versions R13 through 2018
//! (`AC1012`, `AC1014`, `AC1015`, `AC1018`, `AC1021`, `AC1024`, `AC1027`, `AC1032`).
//!
//! Contract used by the `cadkit` facade: [`sniff`] and [`read`]. The [`native`]
//! module exposes the decoded file losslessly (header variables, classes, object
//! map, every object with its common data, type-specific data and raw bytes).
//!
//! Layers, following the ODA *Open Design Specification for .dwg files* v5.4.1:
//! - `container`: file header, section maps, page decompression (LZ77 variants,
//!   R2007 Reed-Solomon de-interleaving);
//! - `header`, `classes`, `handles`: the header variables, class list and object map;
//! - `object`: the per-object bit streams, common data and type decoders
//!   (extension point: `object::registry`);
//! - `mapping`: native objects → [`cadkit_core::Document`].

pub mod bits;
mod classes;
mod codepage;
mod container;
mod crc;
mod handles;
mod header;
mod mapping;
pub mod native;
mod object;
mod reader;
mod version;

use cadkit_core::{Document, ReadOptions, Result};

pub use version::DwgVersion;

/// Returns true when `bytes` start with the magic of a supported DWG version.
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.get(..6).and_then(DwgVersion::from_magic).is_some()
}

/// Reads a whole file into the format-neutral model.
///
/// Fails only when no useful document can be built (unknown format, broken file
/// header or object map, a limit exceeded); recoverable problems become
/// [`Document::warnings`].
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    let file = reader::read_file(bytes, options, options.keep_raw)?;
    mapping::to_document(&file, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_accepts_supported_magics_only() {
        assert!(sniff(b"AC1015\0\0\0\0\0"));
        assert!(sniff(b"AC1032"));
        assert!(!sniff(b"AC1009"));
        assert!(!sniff(b"AC10"));
        assert!(!sniff(b""));
    }

    #[test]
    fn read_rejects_garbage_without_panicking() {
        let options = ReadOptions::default();
        assert!(read(b"not a drawing", &options).is_err());
        assert!(read(b"AC1015", &options).is_err());
        assert!(read(b"AC1018", &options).is_err());
        assert!(read(b"AC1021", &options).is_err());
    }
}
