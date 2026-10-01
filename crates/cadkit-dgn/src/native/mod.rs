//! Lossless native layer.
//!
//! - [`cfb`]: the compound file (OLE2) container used by V8.
//! - [`v8`]: V8 streams, pages and raw elements; [`decode_v8::decode`] decodes one element.
//! - [`v7`]: V7 records; [`v7::decode`] decodes one record.
//! - [`element`]: decoded element data shared by both versions (coordinates in UOR).
//! - [`linkage`]: attribute linkages.
//!
//! Use [`v8::read`] / [`v7::read`] to inspect everything a file contains, including
//! records the document mapping does not model.

pub mod decode_v8;
pub mod element;
pub mod linkage;
pub mod v7;
pub mod v8;

pub use crate::cfb;
pub use crate::text::StringEncoding;
pub use element::*;
