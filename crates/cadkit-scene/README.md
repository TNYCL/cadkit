# cadkit-scene

GPU-free scene data for drawing CAD geometry, part of [cadkit](https://github.com/TNYCL/cadkit).

- `Plan2d`: a persistent store of 2D lines, polylines, arcs and circles (`cadkit-geom` curves)
  split into fixed-size chunks. Each chunk is encoded once into compact GPU instances in f32
  coordinates relative to its center; every frame checks the f32 representation error
  against a screen-space budget and returns chunks that exceed it, curves the GPU does not
  represent and objects without bounds to the caller's CPU path. Selection and preview
  overlays are cut out of the GPU ranges.
- `Mesh3d` / `MeshScene`: triangle meshes in f64 world coordinates, with
  `relative_positions` for an error-checked f64 to f32 conversion against a scene origin.

The crate decides how to draw, not what the geometry means: the caller maps its objects to
`ObjectId`s, chooses colors and stroke widths, tracks changes and calls `upsert` / `remove`.
[`cadkit-gpu`](https://crates.io/crates/cadkit-gpu) draws the result with `wgpu`.

```rust
use cadkit_geom::{Curve, LineSegment, Point2};
use cadkit_scene::{GroupId, ObjectId, Plan2d, Plan2dOptions, PlanItem, PlanView};
use std::collections::BTreeSet;

let mut plan = Plan2d::new(Plan2dOptions::new());
let wall = Curve::Line(LineSegment { start: Point2::new(0., 0.), end: Point2::new(5., 0.) });
plan.upsert(ObjectId(1), PlanItem::new(GroupId(0), vec![wall], [40, 40, 40, 255], 1.5));

let view = PlanView::new(Point2::new(2.5, 0.), 100., [800., 600.], 800.);
plan.prepare(&view, |_| true);
let frame = plan.frame(&view, |_| true, &BTreeSet::new(), true);
assert_eq!(frame.draws.len(), 1);
```

Licensed under MIT or Apache-2.0, at your option.
