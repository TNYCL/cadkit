# cadkit

cadkit reads DWG, DGN (V7 and V8), DXF and CityGML 2.0 into one format-neutral model,
with native CityGML preservation, in pure Rust with no vendor SDK or C dependencies. The same core is available as a Rust
library, a command-line tool, Python and JavaScript (WebAssembly) bindings, and a C and C++
API. It is dual-licensed under MIT or Apache-2.0 and is developed clean-room: every source of
format knowledge is recorded in [docs/PROVENANCE.md](docs/PROVENANCE.md).

> Status: early development (0.1.0, unreleased). Readers do not cover every entity type yet; see the support
> matrix below. Nothing has been published to crates.io, PyPI or npm yet.

## Format support

| Format | Read | Write | Status |
|---|---|---|---|
| DXF (ASCII and binary) | R12 to 2018 | ASCII DXF, R12 to 2018 | Implemented; entity coverage is still growing |
| DWG | AC1014 to AC1032, including 2007 | no | Implemented; R13 (AC1012) is untested; entity coverage is still growing |
| DGN V7 | yes | no | Implemented, including tags |
| DGN V8 | yes | seed-based V8 subset | Local tests pass; external application acceptance pending |
| CityGML 2.0 | native XML and geometry | native XML and explicit generic geometry | Independent XSD tests; no TKGM business rules |

Output formats of the model: JSON, SVG, DXF, seed-based DGN V8 and CityGML 2.0
(Rust, CLI, Python, WASM and C; C++ wraps the C ABI).
DGN V8 and CityGML writers are available with [explicit limits](docs/gml/FORMAT_NOTES.md).
Local test evidence and remaining external acceptance are recorded in
[docs/INTERCHANGE_STATUS.md](docs/INTERCHANGE_STATUS.md).
There is no DWG writer yet. Readers degrade instead of failing: records they cannot
model become `Unknown` entities and warnings on the document. Readers may still return
`Unsupported` for individual files that use features cadkit does not handle; see
[CHANGELOG.md](CHANGELOG.md) for the current state.

"DWG", "DGN" and "DXF" are used only to name file formats. cadkit is not affiliated with or
certified by any CAD vendor.

## Install

Nothing is published yet. From the first release on, the packages are:

| Target | Install |
|---|---|
| Rust library | `cargo add cadkit` |
| CLI | prebuilt binaries on [GitHub Releases](https://github.com/TNYCL/cadkit/releases), `cargo binstall cadkit-cli` or `cargo install cadkit-cli` |
| Python | `pip install pycadkit` (imports as `cadkit`) |
| JavaScript | `npm install cadkit-wasm` |
| C / C++ | headers and libraries in the `cadkit-capi-<target>` archives on GitHub Releases |

The release process is described in [docs/RELEASING.md](docs/RELEASING.md). Until the first
release, build from a checkout:

```sh
git clone https://github.com/TNYCL/cadkit
cd cadkit
```

| Target | Command (from a checkout) |
|---|---|
| Rust library | `cadkit = { git = "https://github.com/TNYCL/cadkit" }` in `Cargo.toml` (crates.io after the first release) |
| CLI | `cargo install --path crates/cadkit-cli` |
| Python | `pip install maturin && cd crates/cadkit-py && maturin develop --release` ([docs/python.md](docs/python.md)) |
| JavaScript | `python crates/cadkit-wasm/build.py`, then import `crates/cadkit-wasm/pkg` ([docs/wasm.md](docs/wasm.md)) |
| C / C++ | `cargo build -p cadkit-capi --release`, then use `crates/cadkit-capi/include/` ([docs/c-api.md](docs/c-api.md)) |

Minimum supported Rust version: 1.85.

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

JavaScript:

```js
import init, { read } from "./pkg/cadkit_wasm.js";
await init();
const doc = read(new Uint8Array(await file.arrayBuffer()));
const svg = doc.toSvg({ widthPx: 1200 });
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

## Design

Crates: `cadkit-core` (model, limits, errors, SVG), one reader crate per format
(`cadkit-dxf`, `cadkit-dgn`, `cadkit-dwg`, `cadkit-gml`), the `cadkit` facade, and the bindings
(`cadkit-cli`, `cadkit-py`, `cadkit-wasm`, `cadkit-capi`). See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

Readers treat every file as untrusted: no panics, all sizes checked against configurable
limits before allocation, and recoverable problems become warnings on the document instead
of errors. See [SECURITY.md](SECURITY.md).

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

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

Copyright © 2026 TNYCL ([tnycl.com](https://tnycl.com)).

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Unless you state otherwise, any contribution
intentionally submitted for inclusion in cadkit is dual licensed as above, without additional
terms or conditions.
