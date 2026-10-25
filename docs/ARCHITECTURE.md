# Architecture

cadkit is a modular CAD engine for building CAD applications. Its file/document stack,
standalone geometry, scene data and GPU renderers can be adopted independently. The host
supplies the UI, editing workflow and domain semantics.

## File I/O and document stack

```
            ┌──────────── bindings ────────────┐
 cadkit-cli   cadkit-py (PyO3)   cadkit-wasm   cadkit-capi (C ABI + C++ header)
      └───────────────┬──────────────┴──────────────┘
                   cadkit            facade: detect / read / open / to_json / to_svg /
                                             to_dxf / to_dgn / to_citygml
      ┌───────────┬───┴──────┬────────────┬────────────┐
 cadkit-dgn   cadkit-dwg   cadkit-dxf   cadkit-gml     format crates: native I/O ↔ Document
      └───────────┴────┬─────┴────────────┴────────────┘
                  cadkit-core         model, document operations, bounded I/O, JSON, SVG
```

## Analytic geometry

`cadkit-geom` is an independent `f64` analytic geometry crate, alongside the format
stack above:

```text
cadkit-geom       analytic curves, polygons, booleans, calibration, NURBS
    |            no dependencies on any other cadkit crate
    +-- serde, thiserror, i_overlay, robust, optional schemars
```

The facade and format crates do not depend on the scene or GPU crates; reading a
format must not pull in a GPU runtime. Geometry coordinates and tolerances belong to
the caller; see [geometry contracts](geom/NOTES.md).

## Scene and rendering

```text
cadkit-scene      GPU-free: Plan2d chunked 2D line/arc store with an f32 error
    |             budget and instance encoding; Mesh3d triangle meshes
    +-- cadkit-geom, thiserror (WASM-compatible)
    +-- cadkit-core only with the optional document feature

cadkit-gpu        wgpu 30: PlanRenderer (analytic lines/arcs), MeshRenderer
    |             (lit meshes, orbit camera), render targets and readback
    +-- cadkit-scene, wgpu, glam, thiserror
```

`cadkit-gpu` takes a `wgpu::Device`, `Queue`, target format and `RenderPass`; it opens no
window and depends on no UI toolkit. `cadkit_gpu::wgpu` is re-exported, so callers use the
same `wgpu`; a `wgpu` major upgrade is a cadkit minor release. Uploads happen in
`prepare`, `paint` only records draw calls, so a UI toolkit's render pass can host it.
GPU errors are captured with error scopes and returned as `GpuError`.

`cadkit-scene` knows how to draw, not what the geometry means. Callers map their
objects to `ObjectId`s, choose colors and stroke widths in pixels, track which objects
changed and call `upsert` / `remove`; change tracking and domain rules stay with the
caller. Chunks that would draw imprecisely in f32 at the current zoom, curves the GPU
does not represent and objects without bounds are returned to the caller's CPU path
for that frame, never dropped. The instance byte layout is documented in
`cadkit_scene::layout`.

The optional `cadkit-scene/document` feature provides `document::plan_document`: one
neutral document model becomes plan items, with blocks/arrays expanded, placement applied,
XY projection, layer groups and resolved colors/lineweights. Non-circular curves are
flattened. `DocumentPlanReport` records skipped entity kinds, flattened curves, dashes drawn
solid and traversal truncation. The bridge produces plan items, not 3D meshes; a host
constructs its `MeshScene` separately. `PlanItem::force_cpu` explicitly requests the host's
CPU drawing path. See [host integration](CADKIT_INTEGRATION.md#scene-and-gpu-integration).

The curve and NURBS helpers in `cadkit-core` overlap with the standalone geometry layer;
they remain separate APIs with their own contracts.

## Contracts

Each format crate exposes at least:

```rust
pub fn sniff(bytes: &[u8]) -> bool;                                   // header check only
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document>; // full read
```

`cadkit-dxf` exposes `write(&Document, DxfVersion) -> Result<String>`.
`cadkit-dgn` exposes `write_v8(&Document, seed, &WriteOptions)` and `repack_v8`.
`cadkit-gml` exposes bounded native XML read/write (`CityGmlDocument`), a neutral
geometry bridge, and optional local structural TKGM preflight (`cadkit-gml::tkgm`);
the facade offers `to_dgn` and `to_citygml` (plus `gml::write_document_with_report`).
Seed, CRS, and options are explicit inputs.

CityGML semantics live in `cadkit-gml::CityGmlDocument`. `EntityKind::Polygon`
is the format-neutral exception: an exterior and interior rings are necessary to
carry holes honestly through CAD operations and SVG/DXF export. Building/room/LoD
semantics are not added to core. Native CityGML, rather than a reverse-engineered
neutral projection, is the authoritative input for preservation roundtrips.

See [GML contracts](gml/FORMAT_NOTES.md) and [DGN writer limits](dgn/FORMAT_NOTES.md).
`cadkit::gml::tkgm` checks documented public structural subsets on native CityGML
without certifying institutional acceptance; proprietary domain rules and host-application
workflows live outside this repository.

Format crates are layered internally:

1. **container** — file framing (DGN V8: OLE compound file + zlib pages; DWG: file header,
   sections, page maps, LZ77 / Reed-Solomon; DGN V7: flat record stream; CityGML 2.0:
   bounded UTF-8 XML token stream).
2. **records** — native, lossless structures (`DgnElement`, `DwgObject`, `CityGmlDocument`),
   public so advanced users can inspect or preserve native structure.
3. **mapping** — native ↔ `cadkit_core::Document`.

## Model rules (`crates/cadkit-core/src/model.rs`)

- Coordinates in drawing units described by `Document::units`.
  DGN: master units = `(uor - global_origin) / uor_per_master`.
- Angles in radians, CCW about the entity normal.
- References by name (layer, block, linetype, style).
- DGN levels → `Layer`; shared cell definitions → `Block`; shared cell instances → `Insert`;
  normal cells, complex chains/shapes and text nodes → `Group` with children.
- DGN tags / item-type properties and DWG ATTRIBs → `Entity::attributes`.
- DGN palette colors resolve to `Color::Rgb`; the index goes to `props["dgn.color_index"]`.
- Anything decoded without a typed home goes to `props` with a format prefix.
- `Entity::raw` is filled only when `ReadOptions::keep_raw` is set.

## Versioned JSON and Rust API migration

Document JSON written by 0.5 includes `schema_version: 1`. JSON without the field still
reads; other schema versions return `Error::SchemaVersion`. `cadkit-core/schemars` provides
JSON Schema generation, with the current schema committed in
[schema/document-v1.schema.json](schema/document-v1.schema.json).

Model and option types marked `#[non_exhaustive]` are constructed through `new()`,
`Default` and `with_*` setters; non-exhaustive enum matches need a fallback arm.
`Error::LimitExceeded` has named `limit: LimitKind` and `message` fields. These are 0.5
source changes; see the [0.4 migration guide](../CHANGELOG.md#migrating-from-04) for exact
replacements. Later 0.5.x releases follow the documented additive API policy.

## Errors and limits

`Error` carries absolute offsets. Readers return `Err` only when no useful document can be
built; recoverable issues become `Document::warnings`. All file-provided sizes are checked
against `Limits` before allocation.

## Bindings

- **cadkit-py**: `cadkit.read(bytes|path) -> Document`, `cadkit.read_citygml(bytes) -> CityGmlDocument`
  (Python classes and dicts via JSON), `doc.to_svg()`, `doc.to_dxf()`, `doc.to_dgn()`,
  `doc.to_citygml()` / `doc.to_citygml_with_report()`, `doc.to_json()`, and
  `CityGmlDocument.preflight_tkgm()`; built with maturin, abi3 wheels.
- **cadkit-wasm**: `read(Uint8Array) -> CadDocument`, `readCityGml(Uint8Array) -> CityGmlDocument`,
  `toSvg`, `toDxf`, `toDgn`, `toCityGml` / `toCityGmlWithReport`, `preflightTkgm`; plus a static
  drag-and-drop viewer page in `crates/cadkit-wasm/www/`.
- **cadkit-capi**: opaque `cadkit_document*` (live-handle registry: double free returns
  `CADKIT_INVALID_HANDLE`), `cadkit_status` return codes, thread-local last-error message,
  `cadkit_document_info` struct + JSON/SVG/DXF/DGN/CityGML exporters, native CityGML JSON I/O
  and TKGM preflight, `cadkit_status_string`, `cadkit_version`; hand-written `include/cadkit.h`
  (a test fails on missing exports) and a header-only C++17 RAII wrapper `include/cadkit.hpp`.
  No panic crosses the boundary.


Bindings expose header-only inspection and neutral document operations. Python and C/C++
unit conversion/block expansion return new immutable documents; WASM performs the same
operations in-place. C additions use separate entry points without enlarging existing ABI
structs. The CLI accepts bounded stdin, explicit stdout formats, legacy DXF output code
pages, and transform/filter options. SVG hidden-layer inclusion is a separate render/walk
entry point. C ABI layouts and the Python/WASM APIs were retained in 0.5; Rust callers
follow the constructor and non-exhaustive migration described above.

The bindings expose file I/O and their documented document operations. They do not expose
the standalone `cadkit-geom`, `cadkit-scene` or `cadkit-gpu` APIs; the browser viewer uses
SVG output. Rust rendering integration is a separate path.
