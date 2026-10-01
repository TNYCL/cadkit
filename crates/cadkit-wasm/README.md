# cadkit-wasm

Read DWG, DGN (V7 and V8), DXF and CityGML 2.0 drawings in the browser or in Node with
WebAssembly, and write SVG, JSON, DXF, seed-based DGN V8 and CityGML 2.0. Built from the
Rust library [cadkit](https://github.com/TNYCL/cadkit). Everything runs locally; files are
never uploaded.

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

The API (options, errors, limits, DGN and CityGML export) is documented in
[docs/wasm.md](https://github.com/TNYCL/cadkit/blob/main/docs/wasm.md).

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor.

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of the Apache
License, Version 2.0 or the MIT license at your option (`LICENSE-APACHE`, `LICENSE-MIT`).
