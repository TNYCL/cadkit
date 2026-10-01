//! Clean-room reader for MicroStation DGN V7 (ISFF) and V8 (OLE compound file) drawings.
//!
//! Contract used by the `cadkit` facade: [`sniff`] and [`read`]. The [`native`] module
//! exposes the lossless layers (container streams, pages, raw elements, decoded element
//! data) for advanced users.
//!
//! Layering: container ([`cfb`], zlib pages / V7 record stream) -> records
//! ([`native`]) -> mapping to [`cadkit_core::Document`].

pub mod cfb;
pub mod native;
pub mod writer;
pub use writer::{WriteOptions, repack_v8, write_v8};
pub mod palette;

mod budget;
#[cfg(test)]
mod fuzz_tests;
mod le;
mod level_names;
mod map;
mod text;
mod zlib;

use cadkit_core::{Document, Error, ReadOptions, Result};

/// Returns true when `bytes` look like a DGN file: a V7 design file header, or a compound
/// file whose root storage holds the `Dgn~H` stream (other OLE files such as `.doc` or
/// `.xls` are rejected). Reads only headers and the compound file directory.
pub fn sniff(bytes: &[u8]) -> bool {
    native::v7::sniff(bytes) || native::v8::sniff(bytes)
}

/// Reads a whole DGN V7 or V8 file into the format-neutral model.
///
/// Coordinates are converted from storage units (UOR) to the master units of the first
/// model: `(uor - global_origin) / uor_per_master`. Recoverable problems are reported in
/// [`Document::warnings`]; `Err` is returned only when nothing useful can be read.
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    if native::v7::sniff(bytes) {
        return map::v7::read(bytes, options);
    }
    if bytes.starts_with(&cfb::SIGNATURE) {
        return map::v8::read(bytes, options);
    }
    Err(Error::UnknownFormat)
}
