# cadkit

The file I/O and document facade of [cadkit](https://github.com/TNYCL/cadkit), an open-source
CAD engine for building CAD applications. Read DWG, DGN (V7 and V8), DXF and CityGML 2.0
into one format-neutral model; write DXF, seed-based DGN V8, CityGML 2.0, SVG and JSON.
DWG is read-only; DGN writing covers a documented V8 subset and requires a compatible seed.
The parsers are pure Rust and use no vendor SDK.

Use header-only inspection, bounded stream I/O and Cargo features to select the file
handling your application needs. Documents support transforms, unit conversion, block
expansion, layer filtering and unused-definition purging through `cadkit-core`, re-exported
by this facade. Recoverable reader problems become document warnings; configurable limits
bound input handling.

```rust
let doc = cadkit::open("plan.dxf")?;
for model in &doc.models {
    println!("{}: {} entities", model.name, model.entities.len());
}
std::fs::write("plan.svg", cadkit::to_svg(&doc, &cadkit::SvgOptions::default()))?;
```

Format support, limits and test evidence are in the [repository README](https://github.com/TNYCL/cadkit#format-support).
The same core is available as a CLI ([`cadkit-cli`](https://crates.io/crates/cadkit-cli)),
for Python ([`pycadkit`](https://pypi.org/project/pycadkit/)), for JavaScript
([`cadkit-wasm`](https://www.npmjs.com/package/cadkit-wasm)) and as a C/C++ library
([GitHub Releases](https://github.com/TNYCL/cadkit/releases)).

## Modules and API migration

Adopt [`cadkit-geom`](https://crates.io/crates/cadkit-geom) for standalone analytic geometry,
[`cadkit-scene`](https://crates.io/crates/cadkit-scene) for GPU-independent plan/mesh data,
and [`cadkit-gpu`](https://crates.io/crates/cadkit-gpu) for `wgpu` rendering separately.
Reading a file through this facade does not bring in a GPU runtime.

Version 0.5 introduces model/option constructors, non-exhaustive types, typed limit errors
and versioned document JSON. See the
[0.4 to 0.5 migration guide](https://github.com/TNYCL/cadkit/blob/main/CHANGELOG.md#migrating-from-04).

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
