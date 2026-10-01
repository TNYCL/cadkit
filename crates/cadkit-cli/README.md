# cadkit-cli

The `cadkit` command-line tool: inspect and convert DWG, DGN, DXF and CityGML files to JSON,
SVG, DXF, seed-based DGN V8 and CityGML 2.0.

## Install

- Prebuilt binaries for Windows, macOS and Linux: [GitHub Releases](https://github.com/TNYCL/cadkit/releases)
- `cargo binstall cadkit-cli` (downloads the prebuilt binary)
- `cargo install cadkit-cli` (builds from source)

## Usage

```sh
cadkit info plan.dxf
cadkit layers plan.dxf
cadkit convert plan.dxf plan.svg --width 1200
cadkit convert plan.dxf plan.json --pretty
cadkit convert plan.dxf plan.dgn --seed blank.dgn
cadkit convert surfaces.dxf model.gml --crs "YOUR_EXPLICIT_CRS" --lod 1
cadkit validate-gml model.gml
```

`cadkit --help` lists every command and option.

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
