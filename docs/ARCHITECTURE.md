# Architecture

```
            ┌──────────── bindings ────────────┐
 cadkit-cli   cadkit-py (PyO3)   cadkit-wasm   cadkit-capi (C ABI + C++ header)
      └───────────────┬──────────────┴──────────────┘
                   cadkit            facade: detect / read / open / to_json / to_svg / to_dxf
      ┌───────────┬───┴──────┬────────────┐
 cadkit-dgn   cadkit-dwg   cadkit-dxf      format crates: native decode → Document
      └───────────┴────┬─────┴────────────┘
                  cadkit-core         model, ByteReader, Error, Limits, ReadOptions, SVG
```

## Contracts

Each format crate exposes at least:

```rust
pub fn sniff(bytes: &[u8]) -> bool;                                   // header check only
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document>; // full read
```

`cadkit-dxf` exposes `write(&Document, DxfVersion) -> Result<String>`.
`cadkit-dgn` exposes `write_v8(&Document, seed, &WriteOptions)` and `repack_v8`.
`cadkit-gml` exposes bounded native XML read/write and a neutral geometry bridge;
the facade offers `to_dgn` and `to_citygml`. Seed/CRS/options are explicit inputs.

CityGML semantics live in `cadkit-gml::CityGmlDocument`. `EntityKind::Polygon`
is the format-neutral exception: an exterior and interior rings are necessary to
carry holes honestly through CAD operations and SVG/DXF export. Building/room/LoD
semantics are not added to core. Native CityGML, rather than a reverse-engineered
neutral projection, is the authoritative input for preservation roundtrips.

See [GML contracts](gml/FORMAT_NOTES.md) and [DGN writer limits](dgn/FORMAT_NOTES.md).
Company/TKGM rules and Cady integration are outside this implementation.

Format crates are layered internally:

1. **container** — file framing (DGN V8: OLE compound file + zlib pages; DWG: file header,
   sections, page maps, LZ77 / Reed-Solomon; DGN V7: flat record stream).
2. **records** — native, lossless structures (`DgnElement`, `DwgObject`), public under a
   `native` module so advanced users can inspect everything.
3. **mapping** — native → `cadkit_core::Document`.

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

## Errors and limits

`Error` carries absolute offsets. Readers return `Err` only when no useful document can be
built; recoverable issues become `Document::warnings`. All file-provided sizes are checked
against `Limits` before allocation.

## Bindings

- **cadkit-py**: `cadkit.read(bytes|path) -> Document` (Python classes or dicts via JSON),
  `doc.to_svg()`, `doc.to_dxf()`, `doc.to_json()`; built with maturin, abi3 wheels.
- **cadkit-wasm**: `read(Uint8Array) -> object`, `toSvg`, `toDxf`; plus a static
  drag-and-drop viewer page in `crates/cadkit-wasm/www/`.
- **cadkit-capi**: opaque `cadkit_document*` (live-handle registry: double free returns
  `CADKIT_INVALID_HANDLE`), `cadkit_status` return codes, thread-local last-error message,
  `cadkit_document_info` struct + JSON/SVG/DXF getters, `cadkit_status_string`, `cadkit_version`;
  hand-written `include/cadkit.h` (a test fails on missing exports) and a header-only C++17
  RAII wrapper `include/cadkit.hpp`. No panic crosses the boundary.
