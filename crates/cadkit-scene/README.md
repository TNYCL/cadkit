# cadkit-scene

GPU-independent scene data for CAD application viewers and editors, part of the
[cadkit CAD engine](https://github.com/TNYCL/cadkit). It depends on `cadkit-geom` and is
WASM-compatible; it does not create a GPU device or open a window.

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

## From a drawing document

Enable the optional `document` feature for `cadkit_scene::document::plan_document` and
`DocumentPlanOptions`. This bridge adds `cadkit-core` and turns one document model into
plan items: it expands blocks and arrays, projects onto XY, resolves colors/lineweights
and groups items by layer. Curves that do not stay circular are flattened to polylines.

Inspect `DocumentPlanReport`: it counts flattened curves, dashes drawn solid, skipped
entity kinds and traversal truncation. Text, hatches/fills, points, images and viewports
are not drawn by this bridge. It does not produce a 3D mesh from a drawing document.

A frame's CPU handoff must be drawn by the host. `PlanItem::force_cpu` explicitly keeps an
item on that path; selection/preview cut-outs and precision failures also need host handling.
The [render_png example](https://github.com/TNYCL/cadkit/blob/main/crates/cadkit-gpu/examples/render_png.rs)
shows the document bridge with `cadkit-gpu`.

Licensed under MIT or Apache-2.0, at your option.
