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

cadkit reads DWG, DGN (V7 and V8), DXF and CityGML 2.0 drawings into one format-neutral
model and writes SVG, JSON, DXF, seed-based DGN V8 and CityGML 2.0. It is pure Rust, with no
vendor SDK and no C dependencies. The same core ships as a Rust library, a command-line
tool, Python and JavaScript (WebAssembly) packages, and a C and C++ API.

```sh
pip install pycadkit      # Python (imports as `cadkit`)
npm install cadkit-wasm   # JavaScript, browser and Node
cargo add cadkit          # Rust
```

cadkit is developed clean-room and dual-licensed under MIT or Apache-2.0. Every source of
format knowledge is recorded in [docs/PROVENANCE.md](docs/PROVENANCE.md).

> **Versioning:** cadkit is 0.x, so a minor release (for example 0.2 to 0.3) may change the
> API. Every change is listed in [CHANGELOG.md](CHANGELOG.md).

## Format support

| Format | Read | Write | Status |
|---|---|---|---|
| DXF (ASCII and binary) | R12 to 2018 | ASCII DXF, R12 to 2018 | Implemented; entity coverage is still growing |
| DWG | AC1014 to AC1032, including 2007 | no | Implemented; R13 (AC1012) is untested; entity coverage is still growing |
| DGN V7 | yes | no | Implemented, including tags |
| DGN V8 | yes | seed-based V8 subset | Local tests pass; external application acceptance pending |
| CityGML 2.0 | native XML and geometry | native XML and explicit generic geometry | Independent XSD tests; no TKGM business rules |

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
The minimum supported Rust version is 1.85.

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
cadkit info plan.dxf
cadkit layers plan.dxf
cadkit convert plan.dxf plan.svg --width 1200
cadkit convert plan.dxf plan.json --pretty
cadkit convert plan.dxf plan.dgn --seed blank.dgn
cadkit convert surfaces.dxf model.gml --crs "YOUR_EXPLICIT_CRS" --lod 1
cadkit validate-gml model.gml
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
| Rust API | [docs.rs/cadkit](https://docs.rs/cadkit) |
| Python API | [docs/python.md](docs/python.md) |
| JavaScript / WebAssembly API | [docs/wasm.md](docs/wasm.md) |
| C and C++ API | [docs/c-api.md](docs/c-api.md) |
| Format notes | [DWG](docs/dwg/FORMAT_NOTES.md), [DGN](docs/dgn/FORMAT_NOTES.md), [DXF](docs/dxf/NOTES.md), [CityGML](docs/gml/FORMAT_NOTES.md) |
| Architecture | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Release history | [CHANGELOG.md](CHANGELOG.md) |

## Design

Crates: `cadkit-core` (model, limits, errors, SVG), one reader crate per format
(`cadkit-dxf`, `cadkit-dgn`, `cadkit-dwg`, `cadkit-gml`), the `cadkit` facade, and the bindings
(`cadkit-cli`, `cadkit-py`, `cadkit-wasm`, `cadkit-capi`). See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

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
