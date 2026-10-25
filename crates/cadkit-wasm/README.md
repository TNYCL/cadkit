# cadkit-wasm

WebAssembly file I/O and document operations for [cadkit](https://github.com/TNYCL/cadkit),
an open-source CAD engine for building CAD applications. Read DWG, DGN (V7 and V8), DXF
and CityGML 2.0 drawings in the browser or Node, and write SVG, JSON, DXF, seed-based DGN V8
and CityGML 2.0. DWG is read-only; DGN output requires a compatible seed and covers a V8
subset. Parsing and export run locally in your browser or Node process.

```sh
npm install cadkit-wasm
```

## Browser and bundlers

```js
import init, { read } from "cadkit-wasm";

await init();                                   // loads cadkit_wasm_bg.wasm next to the module
const doc = read(new Uint8Array(await file.arrayBuffer()));
console.log(doc.info());                        // { format, version, units, models, ... }
const svg = doc.toSvg({ widthPx: 1200 });
doc.free();
```

## Node

`cadkit-wasm/node` is a CommonJS build that is ready as soon as it is loaded:

```js
const { read } = require("cadkit-wasm/node");   // or: import { read } from "cadkit-wasm/node"
const fs = require("node:fs");

const doc = read(fs.readFileSync("plan.dwg"));
console.log(doc.layers());
doc.free();
```

The default entry also works in Node when it is given the module bytes:
`await init({ module_or_path: fs.readFileSync(require.resolve("cadkit-wasm/cadkit_wasm_bg.wasm")) })`.

Use `inspect` for header-only metadata, `convertUnits` and `explodeInserts` for in-place
document operations, and `pickEntity` for geometric viewer selection. SVG output can be
used in a web UI; the repository includes an SVG-based drag-and-drop viewer.

This npm package exposes the file/document APIs in the WASM guide. It does not export the
standalone Rust `cadkit-geom`, `cadkit-scene` or `cadkit-gpu` APIs. DGN and CityGML output
have explicit limits; CityGML generic export requires a CRS. See the
[format matrix](https://github.com/TNYCL/cadkit#format-support).

The API (options, errors, limits, DGN and CityGML export) is documented in
[docs/wasm.md](https://github.com/TNYCL/cadkit/blob/main/docs/wasm.md).

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor.

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of the Apache
License, Version 2.0 or the MIT license at your option (`LICENSE-APACHE`, `LICENSE-MIT`).
