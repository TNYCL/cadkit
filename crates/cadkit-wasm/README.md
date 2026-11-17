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
const doc = read(await file.arrayBuffer());     // Uint8Array, ArrayBuffer, typed array or DataView
console.log(doc.info());                        // { format, version, units, models, ... }
const svg = doc.toSvgBounded({ maxOutputBytes: 16 << 20 });
doc.free();
```

## Node

The package root resolves to the Node build under Node, ready on load; `init()` resolves at
once, so code written for the browser runs unchanged.

```js
import { read } from "cadkit-wasm";             // or: const { read } = require("cadkit-wasm");
import { readFileSync } from "node:fs";

const doc = read(readFileSync("plan.dwg"));
console.log(doc.stats());
doc.free();
```

## What it does

- `read`, `inspect` (header only), `detect`, `readCityGml`, `stripDgnSeed`.
- Reading parts of a document without converting all of it: `models`, `entity(model, path)`,
  paged `entities`, `blocks`, `block`, `stats`, `bbox`, `viewports`; `toJSON` for everything.
- Selection: `pick(model, x, y, { tolerance })` returns the hit's entity path and the model
  entity that owns it (the insert for a block part), with capped metadata. `pickEntity` and
  `pickEntityBounded` are deprecated: their `index` is a visiting order, not an entity index.
- In-place operations: `convertUnits`, `explodeInserts`, `transform`, `retainLayers`,
  `retainKinds`, `purgeUnused`; `clone` copies a document.
- Export: `toSvg` / `toSvgBounded` (with a local origin for map coordinates and `data-e`
  entity references for viewers), `toDxf` / `toDxfWithReport` (lists what DXF left out),
  `toDgn` / `toDgnWithReport` (with `prepare` for documents from other formats) and
  `prepareForDgn`, `toCityGml` / `toCityGmlReport`, `toCityGmlBuildings` (buildings with
  wall, roof and ground surfaces from faces and closed polylines).
- Renderer buffers: `surfaceMesh` (triangles, normals, colours and outlines) and `lineBuffer`
  (segments) as typed arrays relative to an f64 origin, grouped by layer, with an item table
  that leads every triangle and segment back to its entity, and a ray pick.
- CityGML checks: `validate`, `geometryIssues`, and `preflightTkgm` / `preflightTkgmLocated`,
  local structural checks of a TKGM subset that never establish acceptance.

Options are plain objects or JSON text; a wrong value throws `bad_argument` naming the option,
and an unknown key is reported with `console.warn` (it becomes an error in the next minor
release). Errors are `Error` objects with `kind` and, when known, `offset`, `limit`,
`limitValue` and `needed`. Read limits default to values a 4 GiB wasm memory can hold
(1,500,000 objects); `{ preset: "strict" }` tightens them for untrusted uploads. The wasm
memory never shrinks: `memoryBytes()` tells a host when to recycle its worker. The TypeScript
declarations cover every option and result, and the document model as a discriminated union.

This npm package does not export the standalone Rust `cadkit-geom`, `cadkit-gpu` or
`cadkit-scene` plan APIs; it builds the `cadkit-scene` buffers. See the [format matrix](https://github.com/TNYCL/cadkit#format-support) and
the API reference in [docs/wasm.md](https://github.com/TNYCL/cadkit/blob/main/docs/wasm.md).

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor.

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of the Apache
License, Version 2.0 or the MIT license at your option (`LICENSE-APACHE`, `LICENSE-MIT`).
