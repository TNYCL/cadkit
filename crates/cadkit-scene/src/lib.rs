//! GPU-free scene data for drawing CAD geometry.
//!
//! - [`plan`]: [`Plan2d`], a persistent store of 2D lines, polylines, arcs and circles split
//!   into fixed-size chunks. Each chunk encodes its curves once into GPU instances
//!   ([`layout`]) in f32 coordinates relative to the chunk center; every frame checks the
//!   f32 representation error against a screen-space budget and hands chunks that exceed it
//!   back to the caller's CPU path.
//! - [`mesh`]: [`Mesh3d`] triangle meshes in f64 world coordinates and
//!   [`relative_positions`], the error-checked f64 to f32 conversion for upload.
//!
//! The crate knows how to draw, not what the geometry means: callers map their own objects to
//! [`ObjectId`]s, decide colors and widths, track which objects changed and call
//! [`Plan2d::upsert`] / [`Plan2d::remove`]. `cadkit-gpu` renders the result with `wgpu`.
//! Coordinates are in the caller's model units; screen quantities are in logical pixels
//! unless a name says `physical`.
//!
//! ```
//! use cadkit_geom::{Curve, LineSegment, Point2};
//! use cadkit_scene::{GroupId, ObjectId, Plan2d, Plan2dOptions, PlanItem, PlanView};
//! use std::collections::BTreeSet;
//!
//! let mut plan = Plan2d::new(Plan2dOptions::new());
//! let wall = Curve::Line(LineSegment { start: Point2::new(0., 0.), end: Point2::new(5., 0.) });
//! plan.upsert(ObjectId(1), PlanItem::new(GroupId(0), vec![wall], [40, 40, 40, 255], 1.5));
//!
//! let view = PlanView::new(Point2::new(2.5, 0.), 100., [800., 600.], 800.);
//! plan.prepare(&view, |_| true);
//! let frame = plan.frame(&view, |_| true, &BTreeSet::new(), true);
//! assert_eq!(frame.draws.len(), 1);
//! assert!(frame.cpu.is_empty());
//! ```

pub mod layout;
pub mod mesh;
pub mod plan;

pub use mesh::{Mesh3d, MeshBudget, MeshScene, relative_positions};
pub use plan::{ChunkDraw, Plan2d, Plan2dOptions, PlanFrame, PlanItem, PlanView, curve_bounds};

/// Identifier of one caller object. The caller assigns it; [`Plan2d`] draws objects of a
/// chunk in ascending id order, so later ids are drawn on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

/// A group the caller can show or hide per frame without rebuilding, such as a storey or a
/// sheet. Chunks never mix groups; groups are drawn in ascending id order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct GroupId(pub u32);

/// Why scene data could not be produced.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum SceneError {
    /// A caller-supplied budget is not a positive finite number.
    #[error("invalid {name}: must be positive and finite")]
    InvalidBudget {
        /// Name of the parameter.
        name: &'static str,
    },
    /// A coordinate cannot be represented relative to the origin within the error budget.
    #[error("vertex {vertex} exceeds the f32 precision budget relative to the origin")]
    Precision {
        /// Index of the first vertex that failed.
        vertex: usize,
    },
}
