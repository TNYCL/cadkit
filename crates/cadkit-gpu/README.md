# cadkit-gpu

`wgpu` renderers for [`cadkit-scene`](https://crates.io/crates/cadkit-scene), part of
[cadkit](https://github.com/TNYCL/cadkit).

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

CI runs the rendering tests on Mesa lavapipe (`CADKIT_GPU_TESTS=1 cargo test -p cadkit-gpu`);
that is software-rasterizer evidence, not a substitute for checks on real GPUs.

Licensed under MIT or Apache-2.0, at your option.
