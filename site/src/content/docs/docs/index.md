---
title: "Overview"
description: "cadkit documentation. Read DWG, DGN, DXF and CityGML and write DXF, DGN V8, CityGML, SVG and JSON from Rust, Python, JavaScript, C or the CLI."
next:
  link: /docs/getting-started/rust/
  label: 'Getting started: Rust'
---

cadkit is an open-source CAD engine written in pure Rust. It reads DWG, DGN (V7 and V8), DXF
and CityGML 2.0 into one format-neutral document and writes DXF, seed-based DGN V8,
CityGML 2.0, SVG and JSON. It uses no vendor SDK, and reading a drawing needs no GPU.

File I/O and document operations ship as a Rust crate, a command-line tool, a Python package,
a JavaScript (WebAssembly) package and a C/C++ API. Standalone analytic geometry
(`cadkit-geom`), GPU-free scene data (`cadkit-scene`) and `wgpu` rendering (`cadkit-gpu`)
are separate Rust crates; the other interfaces do not expose them.

## Start with your language

| Interface | Package | Start here |
|---|---|---|
| Rust | [`cadkit`](https://crates.io/crates/cadkit) on crates.io | [Getting started: Rust](/docs/getting-started/rust/) |
| Python 3.9+ | [`pycadkit`](https://pypi.org/project/pycadkit/) on PyPI, imported as `cadkit` | [Getting started: Python](/docs/getting-started/python/) |
| JavaScript, browser and Node | [`cadkit-wasm`](https://www.npmjs.com/package/cadkit-wasm) on npm | [Getting started: JavaScript](/docs/getting-started/javascript/) |
| C and C++ | `cadkit-capi-<target>` archives on `release.cadkit.org` and [GitHub Releases](https://github.com/TNYCL/cadkit/releases/latest) | [Getting started: C and C++](/docs/getting-started/c/) |
| Shell | `cadkit` binary (`cadkit-cli`), installed with [one command](/docs/getting-started/cli/#install) | [Command line](/docs/getting-started/cli/) |

The [demo](/demo/) runs the JavaScript package in your browser; files you open there stay on
your machine.

## What each format supports

| Format | Read | Write |
|---|---|---|
| [DWG](/formats/dwg/) | AC1014 (R14) to AC1032 (2018); R13 untested | no writer |
| [DGN V7](/formats/dgn/) | yes, including tags | no writer |
| [DGN V8](/formats/dgn/) | yes | seed-based subset; external application acceptance pending |
| [DXF](/formats/dxf/) | ASCII and binary, R12 to 2018 | ASCII, R12 to 2018 |
| [CityGML 2.0](/formats/citygml/) | native model and geometry | native model and generic geometry; new geometry needs an explicit CRS |

Readers degrade instead of failing: records a reader cannot model become `unknown` entities,
and recoverable problems become warnings on the document. Local test evidence and what is
still open are recorded in [Interchange status](/docs/reference/interchange-status/).

## Guides

- [Convert DXF to SVG](/docs/guides/dxf-to-svg/), the same task in every interface.
- [Inspect a drawing and its layers](/docs/guides/inspect-drawing/): header-only inspection, layers, entity counts and warnings.
- [Write DGN V8 from a seed](/docs/guides/write-dgn-v8/): stripping a seed, preparing a document and the writer's limits.
- [CityGML 2.0 and the TKGM preflight](/docs/guides/citygml-tkgm/): explicit CRS, validation and a local structural preflight.
- [Build a browser viewer](/docs/guides/browser-viewer/) with cadkit-wasm in a Web Worker.
- [Handling untrusted files](/docs/guides/untrusted-files/): resource limits, error kinds and process isolation.
- [Geometry with cadkit-geom](/docs/guides/geometry/) and [scene and GPU rendering](/docs/guides/scene-gpu/) in Rust.

## Reference

- Rust API: [docs.rs/cadkit](https://docs.rs/cadkit), [docs.rs/cadkit-core](https://docs.rs/cadkit-core), [docs.rs/cadkit-geom](https://docs.rs/cadkit-geom), [docs.rs/cadkit-scene](https://docs.rs/cadkit-scene), [docs.rs/cadkit-gpu](https://docs.rs/cadkit-gpu)
- [Python API](/docs/reference/python/), [JavaScript API](/docs/reference/javascript/), [C and C++ API](/docs/reference/c-api/)
- [Document JSON schema](/docs/reference/document-schema/), the shape every interface exports
- [Architecture](/docs/reference/architecture/) and [host integration](/docs/reference/integration/)
- Format notes: [DWG](/docs/reference/formats/dwg/), [DGN](/docs/reference/formats/dgn/), [DXF](/docs/reference/formats/dxf/), [CityGML](/docs/reference/formats/citygml/)

The reference pages are the repository's own documents, published unchanged apart from links;
each has an edit link to its source.

## Versions

cadkit is 0.x: a minor release (for example 0.4 to 0.5) may change the API. From 0.5 on, patch
releases within a minor series only add: model and option types are `#[non_exhaustive]` and
built through constructors. Every change is listed in the [changelog](/changelog/). The
minimum supported Rust version is 1.88.
