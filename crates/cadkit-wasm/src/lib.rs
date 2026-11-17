//! WebAssembly bindings for cadkit (the npm package `cadkit-wasm`).
//!
//! One module per topic: [`io`] (detect, inspect, read, seed stripping, memory), [`document`]
//! (the `CadDocument` class and its JSON), [`access`] (reading parts of a document),
//! [`pick`] (selection), [`edit`] (operations that change a document), [`export`] (writers),
//! [`dgn`] (DGN V8 writing and preparation), [`citygml`] (the native CityGML class),
//! [`buildings`] (CityGML buildings from surfaces), [`scene`] (renderer buffers: surface meshes
//! and line buffers). [`args`] and [`options`] validate what JS passes in
//! and [`error`] builds what is thrown. The TypeScript declarations of options and results are
//! in `ts/`.
//!
//! Errors are JS `Error` objects with `kind` (`bad_argument`, `unknown_format`,
//! `unsupported`, `invalid`, `truncated`, `limit_exceeded`, `io`) and, when known,
//! `offset`, `limit`, `limitValue` and `needed`.

use wasm_bindgen::prelude::*;

mod access;
mod args;
mod bounded_pick;
mod buildings;
mod citygml;
mod dgn;
mod document;
mod edit;
mod error;
mod export;
mod io;
mod options;
mod pick;
mod scene;
mod value;

pub use citygml::{CityGmlDocument, read_citygml};
pub use document::CadDocument;
pub use io::{detect, inspect, memory_bytes, read, strip_dgn_seed};
pub use scene::{LineBuffer, SurfaceMesh};

#[wasm_bindgen(typescript_custom_section)]
const TS_API: &'static str = include_str!("../ts/api.d.ts");

#[wasm_bindgen(typescript_custom_section)]
const TS_DOCUMENT: &'static str = include_str!("../ts/document.d.ts");
