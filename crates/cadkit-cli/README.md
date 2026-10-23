# cadkit-cli

The `cadkit` command-line tool: inspect and convert DWG, DGN, DXF and CityGML files to JSON,
SVG, DXF, seed-based DGN V8 and CityGML 2.0.

## Install

- Prebuilt binaries for Windows, macOS and Linux: [GitHub Releases](https://github.com/TNYCL/cadkit/releases)
- `cargo binstall cadkit-cli` (downloads the prebuilt binary)
- `cargo install cadkit-cli` (builds from source)

## Usage

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

`cadkit --help` lists every command and option. `info`, `layers`, `blocks`, and `inspect`
support `--json`; `dump --json` writes the neutral document. Use `-` for stdin/stdout;
stdout conversion requires `--to json|svg|dxf|dgn|gml`. Redirect binary output. Input
limits also apply to stdin. CLI diagnostics sanitize file-controlled terminal characters;
converted bytes written to a pipe retain their exact encoding.

`dump` and `convert` support `--units`, `--explode-inserts`, `--layer`, and `--kind`.
Units convert first, then block references expand, then filters apply. Layer names and
entity kind names match exactly. Groups containing matching children remain as containers;
use `--explode-inserts` before filtering block contents. Native GML-to-GML preservation
rejects these geometry transforms to avoid silently discarding semantic XML.
`--codepage` controls input decoding; `--output-codepage` is restricted to DXF output.
Completions are available for `bash`, `zsh`, `fish`, and `powershell` without extra Cargo
dependencies; source/register the generated script using your shell's normal mechanism.

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
