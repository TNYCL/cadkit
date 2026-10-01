# Changelog

All notable changes are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) once 1.0 is reached (0.x releases may break).

## [Unreleased]

### Added

- Seed-based DGN V8 geometry/tag export, stream-preserving CFB repacking and
  reproducible public/synthetic acceptance samples. External application acceptance
  is pending; see `docs/dgn/FORMAT_NOTES.md` for the supported subset.
- CityGML 2.0 native read/write, explicit generic geometry export, bounded XML and
  reference handling, geometry diagnostics and independent official-schema tests.
- Format-neutral polygons with interior rings, SVG even-odd fill and DXF hatch
  boundaries; Rust, CLI, Python, WASM, C and C++ export APIs.
- `validate-gml` and `repack-dgn` commands, GML/DGN writer fuzz targets.
- Separate polygon planarity tolerance (`planarity_tolerance`, CLI
  `--planarity-tolerance`, default 0.01 coordinate units) for CityGML validation and
  DGN output; intersection checks keep 1e-6.

### Fixed

- DGN writer on Windows: seed streams were matched by a `\`-separated path, so seed
  graphics were kept and colliding new pages were dropped. Undecodable seed pages are
  now an error instead of being dropped silently.
- DGN V8 reader: a final CFB sector cut short by the end of the file no longer drops
  the stream when the bytes it needs are present.
- DGN writer: integer tags beyond the signed 32-bit range are written as doubles
  instead of failing.

- Workspace of `cadkit-core`, `cadkit-dxf`, `cadkit-dgn`, `cadkit-dwg`, the `cadkit` facade
  and the bindings `cadkit-cli`, `cadkit-py`, `cadkit-wasm` and `cadkit-capi`.
- Format-neutral document model with JSON, SVG and DXF output.
- DXF reader (ASCII and binary) and ASCII DXF writer (R12 to 2018).
- DWG reader (AC1014 to AC1032, including 2007) and DGN V7 / V8 readers, including tags.
- C API (`cadkit-capi`): opaque document handle, status codes with a thread-local error
  message, JSON / SVG / DXF getters, panic containment, `include/cadkit.h`, header-only C++17
  wrapper `include/cadkit.hpp`, C and C++ examples with CMake.
- Fuzz targets `read_any`, `dgn`, `dwg`, `dxf`, `dxf_roundtrip` and a corpus seeding script.
- GitHub Actions: CI (fmt, clippy, tests on Linux/Windows/macOS, MSRV 1.85, wasm, Python
  wheel, C API, fuzz smoke, cargo-deny) and a manual/tag-triggered release workflow.
- `deny.toml` permitting only permissive licenses.
- README, CONTRIBUTING, SECURITY and `docs/c-api.md`.

### Not yet implemented

- No DWG or DGN V7 writer. Full DGN V8 coverage and external acceptance remain open.
- No automatic TKGM/CityMax building inference or receiving-system acceptance.
- DWG R13 (AC1012) is untested; entity coverage of all readers is still growing.
- Nothing is published to crates.io, PyPI or npm.
