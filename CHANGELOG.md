# Changelog

All notable changes are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) once 1.0 is reached (0.x releases may break).

## [Unreleased]

## [0.3.5] - 2026-10-19

DGN dictionary pages keep the seed's layout, so a level table is no longer split across
pages.

### Fixed

- `write_v8` no longer joins the seed's dictionary records (`Dgn^Nm`) and re-splits them
  into new 256 KiB pages. A level table split over two pages is not loaded as one table
  by MicroStation: every element showed on "Default". Every seed page now keeps its
  number and record order; a page whose records did not change keeps its stream byte for
  byte (empty pages included), a changed page is rewritten in place, and new tables and
  tag sets share one new page after the seed's last page. No table or complex element
  crosses a page; only `max_decompressed_bytes` bounds a page (exceeding it is a
  `LimitExceeded` error, nothing is split silently).
- Dictionary and XAttribute page headers write page number and population 0, as on every
  surveyed file. XAttribute records move with their elements.
- A new level entry is appended to the level table's page with the seed entries' flags
  and a fresh level code (`+0x5C`), instead of the first level's code. Seeds written by
  earlier cadkit versions (split tables) are repaired on write.
- The workspace builds with its declared MSRV (Rust 1.85) again: the DGN writer no
  longer uses `let` chains (since 0.3.2).

## [0.3.4] - 2026-10-19

DGN raster attachments can name a file beside the DGN instead of a machine path.

### Added

- A new raster attachment whose `path` is relative and names a file beside the DGN
  stores no machine-specific location: a bare file name writes the name and an empty
  full path, `.\name` (or `./name`) is stored as given. Absolute paths still write the
  `file://` URL. Relative paths into other folders are rejected.

### Fixed

- An empty raster full path or file name reads as absent: the image `path` falls back to
  the file name and no empty `dgn.raster_path` is reported.

## [0.3.3] - 2026-10-16

DGN raster attachments can be written into seeds that hold none.

### Added

- New raster attachments no longer need a seed attachment to clone: a seed without one
  gets a built-in attachment group made of the bytes every surveyed attachment shares,
  with a bitonal display (white foreground, black background) and 200 DPI unless
  `dgn.raster_dpi` is set. `seed_has_raster_template` still tells whether the seed's own
  group is used.

## [0.3.2] - 2026-10-16

DGN seeds with a second level table no longer move new elements to the wrong level.

### Fixed

- Some seeds (most 3D files a CityMax-style post-processor wrote) hold a second level
  table whose level codes repeat those of the first table. Elements store the code only,
  so elements written on a first-table level read back, and displayed, on the second
  table's level of the same code. The writer now keeps the first definition of each code,
  moves later definitions to free codes above the highest one and keeps a single level
  table header with the total count.

## [0.3.1] - 2026-10-16

DGN raster attachments: frames are placed the way MicroStation stores them, and new
attachments can be written by cloning one from the seed.

### Fixed

- DGN raster frames read `0x108`/`0x110` as the edge lengths along the image axes
  (width·|a|, height·|b|), not as an XY extent: `u_vector = â·ext_x`,
  `v_vector = b̂·ext_y`. Rotated and sheared frames no longer fall back to their range;
  `dgn.raster_outside_range` and `dgn.raster_range_placement` no longer occur on any
  frame of the private corpus.

### Added

- DGN raster frames report `size_px` (edge lengths over the pixel axis lengths) and the
  props `dgn.raster_dpi` (`dgn.raster_dpi_y` when it differs), `dgn.raster_rotation` and
  `dgn.raster_skew` (radians).
- `write_v8` writes an `Image` without an `id` (with `path` and `size_px`) as a new
  raster attachment cloned from a complete attachment of the seed, rewriting only ids,
  cross links, the file name and `file://` path, placement, resolution and level. Optional
  props: `dgn.raster_file`, `dgn.raster_dpi`, `dgn.raster_dpi_y`.
- `seed_has_raster_template(seed, &ReadOptions)` tells whether a seed can receive new
  raster attachments.

## [0.3.0] - 2026-10-09

DGN V8 output that MicroStation draws: element ranges, string lengths and displayed tags
now follow the layout of MicroStation and ODA files.

### Changed

- `Attribute` gains `layer`, `display` (`AttributeDisplay`: height, width, style,
  alignment, rotation) and `props`; struct literals need the new fields or
  `..Default::default()`. Serialized documents omit them when empty.
- DGN reading returns each tag's own level, the presentation of displayed tags and the
  tag symbology that differs from its target.
- Raster frames are placed from their stored extent relative to the transform origin;
  the `dgn.raster_extent_shared` warning, an artifact of reading that extent as a
  corner, is gone.

### Fixed

- DGN V8 element ranges store the low corner and the extent for every graphic type, not
  an absolute high corner; MicroStation skipped elements below zero, which read as
  negative extents. The reader resolves ranges the same way.
- DGN text payloads and string linkages no longer count a terminating NUL, which
  MicroStation drew as an extra glyph; tag values keep their counted NUL.
- The DGN code page linkage on text matches MicroStation byte for byte; tags no longer
  carry it.
- DGN text without a style or font number uses font 0 instead of the alphabetically
  first seed font.
- DGN cell headers write `1` at `0x6C` and absolute body bounds, as ODA and MicroStation
  cells do.

### Added

- DGN displayed tags in MicroStation's layout: own level, origin at the owner's first
  vertex with an offset to the display point, size, font, justification, text-box range
  and the 56-byte tag target dependency.
- `WriteOptions::preserve_seed_controls` (default on) keeps seed control records such as
  the coordinate system and model settings, with their auxiliary data.
- DGN writing reproduces graphic groups and MicroStation's undecoded `0x0600` property
  and `0x00C0` type-word flags carried in entity props.

## [0.2.1] - 2026-10-05

### Fixed

- DGN V8 measured text range spans and absolute model/cell bounds, including
  rotated text and nonzero seed origins.
- Seed font validation rejects foreign indexes and name/index mismatches while
  retaining external resource fonts actually referenced by seed graphics.
- ByLayer lineweight resolves explicit native layer indexes; millimetre-only,
  unresolved ByBlock and out-of-range native weights require explicit conversion.

## [0.2.0] - 2026-10-02

CityGML export reports, GML circles and an optional local TKGM preflight. Closed CAD
polylines now stay curves in generic GML export (see Changed).

### Added

- Reported generic CityGML export with explicit metadata-only handling for
  unsupported geometry, available in Rust, CLI, Python, WASM and C/C++.
- Exact GML Circle export/import, synthetic new/edit/reopen checks, and bounded
  local TKGM preflight with required city-model tender / Digital Building
  registration profile selection and explicit unchecked rules. Neither establishes
  official acceptance; contract-specific tender-year conditions remain unchecked.

### Changed

- Closed CAD polylines remain curves in generic GML export instead of being
  implicitly reclassified as polygons; explicit surfaces remain strictly checked.

## [0.1.0] - 2026-10-01

First release. Packages: `cadkit` and its format crates on crates.io, `pycadkit` on PyPI
(imports as `cadkit`), `cadkit-wasm` on npm, and CLI and C API archives on GitHub
Releases.

### Added

- Readers for DXF (ASCII and binary, R12 to 2018), DWG (AC1014 to AC1032, including
  2007), DGN V7 and DGN V8 (including tags) and CityGML 2.0, all into one format-neutral
  document model. Readers treat input as untrusted: no panics, configurable limits,
  recoverable problems become warnings.
- Writers: JSON, SVG and ASCII DXF (R12 to 2018); seed-based DGN V8 geometry and tag
  export with stream-preserving CFB repacking; CityGML 2.0 native read/write with
  explicit generic geometry export, bounded XML and reference handling, geometry
  diagnostics and independent official-schema tests.
- DGN V8 writer coverage: all 15 documented text justification codes, exact zero-width
  bulged-polyline decomposition and retention of unchanged seed raster attachments
  (`preserve_seed_rasters`). See `docs/dgn/FORMAT_NOTES.md` for the supported subset;
  external application acceptance is pending.
- Format-neutral polygons with interior rings, SVG even-odd fill and DXF hatch
  boundaries; a separate polygon planarity tolerance (`planarity_tolerance`, CLI
  `--planarity-tolerance`, default 0.01 coordinate units) for CityGML validation and
  DGN output, while intersection checks keep 1e-6.
- Bindings: the `cadkit` CLI (`info`, `layers`, `convert`, `dump`, `validate-gml`,
  `repack-dgn`), Python (`pycadkit`), JavaScript/WebAssembly (`cadkit-wasm`, browser
  and Node entry points) and a stable C ABI with a header-only C++17 wrapper.
- Fuzz targets for every reader and for the DXF, DGN and CityGML writers, run in CI.
- CI on Linux, Windows and macOS (fmt, clippy, tests, MSRV 1.85, cargo-deny, wasm,
  Python, C API, official CityGML XSD validation, fuzz smoke) and a release workflow
  that publishes all packages from one tag after approval (`docs/RELEASING.md`).

### Fixed

Fixed during development, before this first release:

- DGN writer on Windows: seed streams were matched by a `\`-separated path, so seed
  graphics were kept and colliding new pages were dropped. Undecodable seed pages are
  now an error instead of being dropped silently.
- DGN V8 reader: a final CFB sector cut short by the end of the file no longer drops
  the stream when the bytes it needs are present.
- DGN writer: integer tags beyond the signed 32-bit range are written as doubles
  instead of failing; stored 3D quaternions are conjugated so rotated text, ellipses
  and tilted geometry keep their orientation; unnamed tag sets stay unnamed and
  standalone type-37 tags are written without an invented owner.
- DGN reader and writer share deterministic level names, preserving membership and
  IDs for duplicate or unnamed seed levels.

### Known limitations

- No DWG or DGN V7 writer. Full DGN V8 coverage and acceptance by external
  applications remain open.
- No automatic TKGM/CityMax building inference or receiving-system acceptance.
- DWG R13 (AC1012) is untested; entity coverage of all readers is still growing.

[Unreleased]: https://github.com/TNYCL/cadkit/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/TNYCL/cadkit/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/TNYCL/cadkit/releases/tag/v0.1.0
