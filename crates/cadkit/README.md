# cadkit

Read DWG, DGN (V7 and V8), DXF and CityGML 2.0 drawings into one format-neutral model, and
write DXF, seed-based DGN V8 and CityGML 2.0. Pure Rust, no vendor SDK or C dependencies.
Readers treat every file as untrusted: no panics, configurable size limits, and recoverable
problems become warnings on the document.

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

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
