# cadkit-gpu

`wgpu` renderers for [`cadkit-scene`](https://crates.io/crates/cadkit-scene), part of
[cadkit](https://github.com/TNYCL/cadkit), an open-source CAD engine for building CAD
applications. This is the optional GPU layer; file parsing and scene preparation can be
used independently.

- `PlanRenderer` draws `Plan2d` frames: analytic lines, polylines, arcs and circles with
  mitred corners, one-pixel antialiasing, hairline fading and premultiplied blending.
  Chunk buffers are uploaded once per content generation; panning and zooming rewrite
  32 bytes per chunk.
- `MeshRenderer` draws `MeshScene`s with an `OrbitCamera`, depth testing and flat
  lighting, with incremental mesh and camera updates.
- `render_texture`, `render_offscreen` and `RenderTargetCache` render into reusable
  textures or read RGBA back.

The renderers take a `wgpu::Device`, `Queue`, target format and `RenderPass` and open no
window, so they fit inside the render pass of a UI toolkit: uploads happen in `prepare`,
`paint` only records draw calls. `wgpu` 30 is re-exported as `cadkit_gpu::wgpu`. GPU
validation and out-of-memory errors are captured with error scopes and returned as
`GpuError`.

## Render a drawing to PNG

From a repository checkout, the existing example opens a DWG, DXF or DGN drawing and uses
`cadkit-scene`'s optional `document` bridge to render its plan view:

```sh
cargo run -p cadkit-gpu --example render_png -- plan.dgn plan.png 1600 1000
```

The bridge projects a model onto XY. It reports skipped text, hatches/fills, points, images
and other unsupported kinds, curves flattened to polylines, dashes drawn solid and traversal
truncation. The example reports CPU handoffs but does not draw them. This is a plan-rendering
path; it does not automatically turn drawing entities into 3D meshes. A host supplies its own `MeshScene` for the mesh renderer and draws items
returned through the plan frame's CPU handoff. See the
[scene API](https://docs.rs/cadkit-scene) and
[integration guide](https://github.com/TNYCL/cadkit/blob/main/docs/CADKIT_INTEGRATION.md#scene-and-gpu-integration).

CI runs the rendering tests on Mesa lavapipe (`CADKIT_GPU_TESTS=1 cargo test -p cadkit-gpu`);
that is software-rasterizer evidence, not a substitute for checks on real GPUs.

Licensed under MIT or Apache-2.0, at your option.
