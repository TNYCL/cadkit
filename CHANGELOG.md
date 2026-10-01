# Changelog

All notable changes are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) once 1.0 is reached (0.x releases may break).

## [Unreleased]

### Added

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

- No DWG or DGN writer; DXF is the only writable format.
- DWG R13 (AC1012) is untested; entity coverage of all readers is still growing.
- Nothing is published to crates.io, PyPI or npm.
