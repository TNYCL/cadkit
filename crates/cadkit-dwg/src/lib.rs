//! Clean-room reader for DWG drawings, AC1014 (R14) through AC1032 (2018), including 2007.
//! The AC1012 (R13) path is implemented but untested; see the repository's DWG format notes.
//! Entity coverage is still growing. There is no DWG writer.
//!
//! Contract used by the `cadkit` facade: [`sniff`], [`inspect`] and [`read`]. The [`native`]
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

use cadkit_core::{Document, Format, ReadOptions, Result, Summary, Units};

pub use version::DwgVersion;

/// Returns true when `bytes` start with the magic of a supported DWG version.
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.get(..6).and_then(DwgVersion::from_magic).is_some()
}

/// Inspects the DWG file header and `AcDb:Header` variables without decoding object streams.
pub fn inspect(bytes: &[u8], options: &ReadOptions) -> Result<Summary> {
    let c = container::read_header_only(bytes, &options.limits)?;
    let (encoding, _) = codepage::resolve(c.codepage, options.fallback_codepage.as_deref());
    let ctx = object::DecodeContext {
        version: c.version,
        encoding,
        max_string: options.limits.max_string_bytes,
        max_vertices: options.limits.max_vertices,
    };
    let (units, is_3d) = match &c.header {
        Some(section) => match header::parse(section, c.maintenance, ctx) {
            Ok((vars, _)) => {
                let units = vars
                    .int("INSUNITS")
                    .map_or_else(Units::default, |u| Units::from_insunits(u as i32));
                let is_3d = match (vars.point3("EXTMIN"), vars.point3("EXTMAX")) {
                    (Some([_, _, z0]), Some([_, _, z1]))
                        if z0.is_finite() && z1.is_finite() && z0 <= z1 =>
                    {
                        (z0.abs() > 1e-12 || z1.abs() > 1e-12).then_some(true)
                    }
                    _ => None,
                };
                (Some(units), is_3d)
            }
            Err(_) => (None, None),
        },
        None => (None, None),
    };
    Ok(Summary::new(Format::Dwg, c.version.magic())
        .with_is_3d(is_3d)
        .with_units(units)
        .with_codepage(Some(encoding.name().to_owned())))
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
        assert!(inspect(b"not a drawing", &options).is_err());
        assert!(inspect(b"AC1015", &options).is_err());
    }
}
