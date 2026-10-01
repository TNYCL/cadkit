//! Format-neutral CAD drawing model shared by the cadkit readers and writers.
//!
//! - [`model`]: the [`Document`] every reader produces.
//! - [`bytes`]: bounds-checked little-endian reader for untrusted input.
//! - [`svg`]: 2D SVG rendering of a document.

pub mod aci;
pub mod bytes;
pub mod error;
pub mod export;
pub mod geom;
pub mod geom_ops;
pub mod model;
pub mod ops;
pub mod options;
pub mod polygon;
pub mod svg;
pub mod text;
pub mod units;

pub use error::{Error, Result};
pub use geom::{BBox, Point3, Vec3};
pub use model::*;
pub use options::{Limits, ReadOptions};
