//! Ergonomic prelude re-exporting common model, geometry, option, and format types.
//!
//! ```
//! use cadkit::prelude::*;
//!
//! let mut doc = Document::new();
//! doc.model_space_mut()
//!     .entities
//!     .push(Entity::line(Point3::ZERO, Point3::new(100.0, 50.0, 0.0)).on_layer("WALLS"));
//! assert_eq!(doc.entity_count(), 1);
//! ```

#[cfg(feature = "svg")]
pub use cadkit_core::svg::SvgOptions;
pub use cadkit_core::{
    Attribute, AttributeDisplay, BBox, Block, Color, Document, Entity, EntityKind, Error, Format,
    GroupKind, HAlign, HatchEdge, HatchLoop, Layer, LengthUnit, Limits, Linetype, Lineweight,
    Model, ModelKind, Point3, Props, PurgeStats, ReadOptions, Result, SourceInfo, Summary,
    TextStyle, Transform, Units, VAlign, Value, Vec3, Vertex, Warning,
};

#[cfg(feature = "dgn")]
pub use cadkit_dgn::{
    DimensionPolicy as DgnDimensionPolicy, SeedPrepareOptions as DgnSeedPrepareOptions,
    V8Seed as DgnV8Seed, WriteOptions as DgnWriteOptions,
};

#[cfg(feature = "dxf")]
pub use cadkit_dxf::{DxfVersion, WriteOptions as DxfWriteOptions};

#[cfg(feature = "gml")]
pub use cadkit_gml::{
    ExportOptions as GmlExportOptions, ImportOptions as GmlImportOptions,
    ValidationOptions as GmlValidationOptions,
};
