# Changelog

All notable changes are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) once 1.0 is reached (0.x releases may break).

## [Unreleased]

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

[Unreleased]: https://github.com/TNYCL/cadkit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/TNYCL/cadkit/releases/tag/v0.1.0
