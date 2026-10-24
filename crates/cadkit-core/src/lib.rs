//! Format-neutral CAD drawing model shared by the cadkit readers and writers.
//!
//! - [`model`]: the [`Document`] every reader produces.
//! - [`bytes`]: bounds-checked little-endian reader for untrusted input.
//! - [`svg`]: 2D SVG rendering of a document.

pub mod aci;
pub mod bytes;
pub mod doc_ops;
pub mod error;
pub mod export;
pub mod geom;
pub mod geom_ops;
pub mod model;
mod model_builders;
pub mod ops;
pub mod options;
pub mod polygon;
pub mod svg;
pub mod text;
pub mod units;

pub use doc_ops::PurgeStats;
pub use error::{Error, LimitKind, Result};
pub use geom::{BBox, Point3, Vec3};
pub use geom_ops::Transform;
pub use model::*;
pub use options::{Limits, ReadOptions};
