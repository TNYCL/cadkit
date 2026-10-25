<p align="center">
  <img src="docs/assets/cadkit-icon-256.png" width="128" height="128" alt="cadkit logo">
</p>

# cadkit

[![crates.io](https://img.shields.io/crates/v/cadkit.svg)](https://crates.io/crates/cadkit)
[![PyPI](https://img.shields.io/pypi/v/pycadkit.svg)](https://pypi.org/project/pycadkit/)
[![npm](https://img.shields.io/npm/v/cadkit-wasm.svg)](https://www.npmjs.com/package/cadkit-wasm)
[![docs.rs](https://img.shields.io/docsrs/cadkit)](https://docs.rs/cadkit)
[![CI](https://github.com/TNYCL/cadkit/actions/workflows/ci.yml/badge.svg)](https://github.com/TNYCL/cadkit/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**An open-source CAD engine for building CAD applications.**

Read and write CAD files, manipulate geometry, and render with GPU acceleration using
modules you can adopt independently.

Use cadkit to build drawing viewers and editors, CAD import/export workflows, geometry
tools, and drawing inspection or conversion services. Read DWG, DGN (V7 and V8), DXF and
CityGML 2.0 into one format-neutral document; write DXF, seed-based DGN V8, CityGML 2.0,
SVG and JSON. DWG is currently read-only; DGN output is a seed-based V8 subset.

The Rust modules cover file I/O, document operations, standalone analytic geometry,
GPU-independent scene data and `wgpu` rendering. File I/O and document operations also
ship as a CLI, Python and JavaScript (WebAssembly) packages, and a C/C++ API. The parsers
are pure Rust and use no vendor SDK; reading a drawing does not require a GPU runtime.

```sh
pip install pycadkit      # Python (imports as `cadkit`)
npm install cadkit-wasm   # JavaScript, browser and Node
cargo add cadkit          # Rust
```

cadkit is developed clean-room and dual-licensed under MIT or Apache-2.0. Every source of
format knowledge is recorded in [docs/PROVENANCE.md](docs/PROVENANCE.md).

> **Versioning:** cadkit is 0.x, so a minor release (for example 0.4 to 0.5) may change the
> API. From 0.5 on, patch releases within a minor series (0.5.x) only add: model and option
> types are `#[non_exhaustive]` and built through constructors, so new fields and variants do
> not break callers. Every change is listed in [CHANGELOG.md](CHANGELOG.md).

## Modules

| Need | Module | Scope |
|---|---|---|
| Open and export drawings | [`cadkit`](https://crates.io/crates/cadkit) | Format detection, inspection, I/O and access to the neutral document model |
| Work with drawing data | [`cadkit-core`](https://crates.io/crates/cadkit-core) | Documents, layers, blocks, entities, transforms, units, traversal, limits, JSON and SVG |
| Use a format directly | [`cadkit-dwg`](https://crates.io/crates/cadkit-dwg), [`cadkit-dgn`](https://crates.io/crates/cadkit-dgn), [`cadkit-dxf`](https://crates.io/crates/cadkit-dxf), [`cadkit-gml`](https://crates.io/crates/cadkit-gml) | Readers and supported writers; see the matrix below |
| Compute geometry independently | [`cadkit-geom`](https://crates.io/crates/cadkit-geom) | Analytic curves, polygon booleans, trim/offset, calibration and supported NURBS operations |
| Prepare scene data without a GPU | [`cadkit-scene`](https://crates.io/crates/cadkit-scene) | Chunked 2D plans, precision budgets, CPU handoff and f64 triangle meshes |
| Draw with a GPU | [`cadkit-gpu`](https://crates.io/crates/cadkit-gpu) | `wgpu` 30 plan/mesh renderers, cameras, render targets and readback |
| Integrate through another interface | CLI, Python, WebAssembly, C/C++ | File I/O and documented document operations; see [Install](#install) |

The `cadkit` facade and format crates do not depend on `cadkit-scene` or `cadkit-gpu`.
Choose the geometry and rendering crates separately in Rust. Python, JavaScript and C/C++
expose the APIs in their own guides; they do not expose the standalone `cadkit-geom`,
`cadkit-scene` or `cadkit-gpu` APIs. The host application supplies its UI, editing tools
and domain rules.

## Format support

| Format | Read | Write | Status |
|---|---|---|---|
| DWG | AC1014 (R14) to AC1032 (2018), including 2007 | no | Implemented; R13 (AC1012) is untested; entity coverage is still growing |
| DGN V7 | yes | no | Implemented, including tags |
| DGN V8 | yes | seed-based V8 subset | Local tests pass; external application acceptance pending |
| DXF (ASCII and binary) | R12 to 2018 | ASCII DXF, R12 to 2018 | Implemented; entity coverage is still growing |
| CityGML 2.0 | native XML and geometry | native XML and explicit generic geometry | Independent XSD tests; generic structural/geometric I/O plus optional local structural TKGM preflight (`cadkit::gml::tkgm`), without external or proprietary acceptance rules |

The model can be written as JSON, SVG, DXF, seed-based DGN V8 and CityGML 2.0 from Rust,
the CLI, Python, WebAssembly and C (C++ wraps the C ABI). The DGN V8 writer needs a seed
file and covers a [documented subset](docs/dgn/FORMAT_NOTES.md#seed-based-v8-writer). New
CityGML geometry needs an explicit CRS, and the writer has
[explicit limits](docs/gml/FORMAT_NOTES.md).
Local test evidence and the remaining external acceptance are recorded in
[docs/INTERCHANGE_STATUS.md](docs/INTERCHANGE_STATUS.md). There is no DWG writer yet.

Readers degrade instead of failing: records they cannot model become `Unknown` entities and
warnings on the document. A reader may still return `Unsupported` for a file that uses
features cadkit does not handle yet.

"DWG", "DGN" and "DXF" are used only to name file formats. cadkit is not affiliated with or
certified by any CAD vendor.

## Install

| Target | Package | Install |
|---|---|---|
| Python | [`pycadkit`](https://pypi.org/project/pycadkit/) | `pip install pycadkit` (imports as `cadkit`) |
| JavaScript (browser, Node) | [`cadkit-wasm`](https://www.npmjs.com/package/cadkit-wasm) | `npm install cadkit-wasm` |
| Rust library | [`cadkit`](https://crates.io/crates/cadkit) | `cargo add cadkit` |
| CLI | [`cadkit-cli`](https://crates.io/crates/cadkit-cli) | binary from [GitHub Releases](https://github.com/TNYCL/cadkit/releases/latest), `cargo binstall cadkit-cli` or `cargo install cadkit-cli` |
| C / C++ | `cadkit-capi-<target>` | archive from [GitHub Releases](https://github.com/TNYCL/cadkit/releases/latest): headers, shared and static library |

Python wheels, CLI binaries and C API archives are built for Linux x86_64 and aarch64, macOS
arm64 and x86_64, and Windows x64. One Python wheel per platform serves Python 3.9 and
newer; the Linux CLI is a static binary. Each release lists checksums in `SHA256SUMS`.
The minimum supported Rust version is 1.88.

## Document operations

Work with models, layers, blocks and entities after reading a drawing or constructing a
`Document` in your application. Rust document operations include affine transforms, unit
conversion, block expansion, group flattening, layer filtering and unused-definition
purging. Header-only inspection, bounded stream I/O and Cargo feature selection let a host
choose the file handling it needs. For DGN reading without its writer, disable default
features and select `dgn-read`; add other format/output features as needed.

The bindings expose documented subsets: Python and C/C++ unit conversion and block
expansion return new documents; the corresponding WASM methods mutate the document.
The CLI provides transform/filter flags and bounded stdin/stdout conversion. See the
[API guides](#documentation) for signatures and limits.

Document JSON includes `schema_version: 1`; `cadkit-core` has an optional `schemars`
feature and a [committed schema](docs/schema/document-v1.schema.json). Rust model and
option constructors, non-exhaustive types and typed limit errors are covered by the
[0.5 migration guide](CHANGELOG.md#migrating-from-04).

## Geometry (`cadkit-geom`)

Independent `f64` analytic curves, polygon booleans, trim/offset, calibration and
supported NURBS evaluation, tessellation and nearest-point search. Coordinates
use caller-defined units and explicit tolerances. It depends on no other cadkit
crate. Serde support is always available; enable `schemars` for JSON Schema.

```rust
use cadkit_geom::{GeometryTolerance, LineSegment, Point2};

let line = LineSegment {
    start: Point2::new(0.0, 0.0),
    end: Point2::new(4.0, 0.0),
};
let shifted = line.offset(1.0, &GeometryTolerance::default())?;
assert_eq!(shifted.start, Point2::new(0.0, 1.0)); // Positive offset is left.
# Ok::<(), cadkit_geom::GeometryError>(())
```

NURBS support is limited to the documented degree 1-3, positive-weight, open clamped
profile. Polygon booleans use linear rings; curved and mesh booleans are unsupported.
See [supported profiles, error budgets and limits](docs/geom/NOTES.md).

## Rendering (`cadkit-scene`, `cadkit-gpu`)

`cadkit-scene` is GPU-free scene data: `Plan2d`, a chunked store of 2D lines, polylines,
arcs and circles that encodes each chunk once for the GPU and checks every frame whether
f32 chunk coordinates still meet a screen error budget, plus `Mesh3d` triangle meshes in
f64. `cadkit-gpu` draws them with `wgpu` 30 (re-exported): analytic antialiased strokes
with mitred corners, lit depth-tested meshes with an orbit camera, and offscreen readback.
The scene model needs no GPU. The renderers take a device, queue and render pass and open
no window, so the host supplies its UI toolkit. Chunks that exceed the precision budget,
curves the GPU does not draw and selection overlays are handed back for the caller to draw
through its CPU path. `PlanItem::force_cpu` lets the host request that handoff explicitly.

The `document` feature of `cadkit-scene` bridges a read drawing to the plan renderer and
projects a document model onto XY, expands blocks and arrays, and reports skipped entities,
flattened curves, dashes drawn solid and exhausted traversal budgets. Text, hatches/fills,
points and images are among the skipped kinds. From a checkout, the existing example renders
a DWG, DXF or DGN plan to PNG. The example reports CPU handoffs but does not draw them:

```sh
cargo run -p cadkit-gpu --example render_png -- plan.dxf plan.png 1600 1000
```

## Quick start

Rust:

```rust
let doc = cadkit::open("plan.dxf")?;
for model in &doc.models {
    println!("{}: {} entities", model.name, model.entities.len());
}
std::fs::write("plan.svg", cadkit::to_svg(&doc, &cadkit::SvgOptions::default()))?;
```

CLI:

```sh
cadkit inspect plan.dxf --json
cadkit info plan.dxf --json
cadkit blocks plan.dxf --json
cadkit layers plan.dxf
cadkit convert plan.dxf plan.svg --width 1200
cadkit convert plan.dxf plan.json --pretty
cadkit convert plan.dxf plan.dgn --seed blank.dgn
cadkit convert surfaces.dxf model.gml --crs "YOUR_EXPLICIT_CRS" --lod 1
cadkit validate-gml model.gml
cadkit convert plan.dxf - --to dxf --dxf-version r2000 --output-codepage DOS857 > legacy.dxf
cadkit convert - plan.svg --explode-inserts --units meter --layer WALLS < plan.dxf
cadkit completions bash > cadkit.bash
```

Python:

```python
import cadkit

doc = cadkit.read("plan.dxf")
open("plan.svg", "w").write(doc.to_svg(width=1200))
```

JavaScript in the browser or a bundler:

```js
import init, { read } from "cadkit-wasm";

await init();
const doc = read(new Uint8Array(await file.arrayBuffer()));
const svg = doc.toSvg({ widthPx: 1200 });
doc.free();
```

JavaScript in Node:

```js
const { read } = require("cadkit-wasm/node");
const fs = require("node:fs");

const doc = read(fs.readFileSync("plan.dwg"));
console.log(doc.layers());
doc.free();
```

C++:

```cpp
#include "cadkit.hpp"

cadkit::Document doc = cadkit::Document::read_file("plan.dxf");
std::string svg = doc.to_svg();
```

C:

```c
cadkit_document *doc = NULL;
if (cadkit_read_file("plan.dxf", NULL, &doc) == CADKIT_OK) {
    char *svg = NULL;
    if (cadkit_document_to_svg(doc, NULL, &svg) == CADKIT_OK) { /* use svg */ cadkit_string_free(svg); }
    cadkit_document_free(doc);
}
```

## Documentation

| Topic | Where |
|---|---|
| Rust file I/O and document API | [docs.rs/cadkit](https://docs.rs/cadkit), [docs.rs/cadkit-core](https://docs.rs/cadkit-core) |
| Geometry API and contracts | [docs.rs/cadkit-geom](https://docs.rs/cadkit-geom), [geometry notes](docs/geom/NOTES.md) |
| Scene and GPU APIs | [docs.rs/cadkit-scene](https://docs.rs/cadkit-scene), [docs.rs/cadkit-gpu](https://docs.rs/cadkit-gpu) |
| Host application integration | [docs/CADKIT_INTEGRATION.md](docs/CADKIT_INTEGRATION.md) |
| Migrating from 0.4 to 0.5 | [constructors, non-exhaustive types and JSON](CHANGELOG.md#migrating-from-04) |
| Python API | [docs/python.md](docs/python.md) |
| JavaScript / WebAssembly API | [docs/wasm.md](docs/wasm.md) |
| C and C++ API | [docs/c-api.md](docs/c-api.md) |
| Format notes | [DWG](docs/dwg/FORMAT_NOTES.md), [DGN](docs/dgn/FORMAT_NOTES.md), [DXF](docs/dxf/NOTES.md), [CityGML](docs/gml/FORMAT_NOTES.md) |
| Architecture | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Release history | [CHANGELOG.md](CHANGELOG.md) |

## Design

The [module map](#modules) separates the document and file I/O stack from standalone
geometry, scene preparation and GPU rendering. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
for dependency boundaries, coordinate conventions and API contracts.

Readers treat every file as untrusted: no panics, all sizes checked against configurable
limits before allocation, and recoverable problems become warnings on the document instead
of errors. Every reader has a fuzz target that runs in CI. See [SECURITY.md](SECURITY.md).

## Clean-room development and test data

cadkit is built only from allowed references: the public Open Design Specification for
DWG, MIT-licensed projects (ACadSharp, ezdgn, GDAL's DGN driver), public sample files and
our own analysis of sample files. Code or headers from GPL/LGPL/AGPL CAD projects and from
vendor SDKs are never read or ported. The full rules are in [AGENTS.md](AGENTS.md) and the
ledger of sources is [docs/PROVENANCE.md](docs/PROVENANCE.md).

Test data comes from two places:

- `corpus/public/`: sample files of open-source projects, fetched by
  `python scripts/fetch-corpus.py`, never committed.
- `corpus/private/`: real client drawings used only on the maintainer's machine. The
  directory is git-ignored; tests that use it skip when it is missing and assert only
  structural facts. Its contents never appear in code, docs, logs or reports.

## Building from source

```sh
git clone https://github.com/TNYCL/cadkit
cd cadkit
```

| Target | Command (from a checkout) |
|---|---|
| Rust library | `cadkit = { git = "https://github.com/TNYCL/cadkit" }` in `Cargo.toml` |
| CLI | `cargo install --path crates/cadkit-cli` |
| Python | `pip install maturin && cd crates/cadkit-py && maturin develop --release` ([docs/python.md](docs/python.md)) |
| JavaScript | `python crates/cadkit-wasm/build.py`, then import `crates/cadkit-wasm/pkg` ([docs/wasm.md](docs/wasm.md)) |
| C / C++ | `cargo build -p cadkit-capi --release`, then use `crates/cadkit-capi/include/` ([docs/c-api.md](docs/c-api.md)) |

How releases are made: [docs/RELEASING.md](docs/RELEASING.md).

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

Copyright © 2026 TNYCL ([tnycl.com](https://tnycl.com)).

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Unless you state otherwise, any contribution
intentionally submitted for inclusion in cadkit is dual licensed as above, without additional
terms or conditions.
