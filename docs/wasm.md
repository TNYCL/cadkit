# cadkit-wasm

WebAssembly bindings for cadkit plus a static, drag-and-drop viewer. Everything runs in the
browser (or Node); drawings are never uploaded.

## Build

```
python crates/cadkit-wasm/build.py
```

The script runs `cargo build --release --target wasm32-unknown-unknown` through
`scripts/buildlock.py` (do not wrap `build.py` in the lock again), then runs the
`wasm-bindgen` CLI twice:

- `crates/cadkit-wasm/pkg/` for `--target web` (ES module, used by the viewer)
- `crates/cadkit-wasm/pkg-node/` for `--target nodejs` (CommonJS)

The CLI version must equal the `wasm-bindgen` version in `Cargo.lock`. `build.py` looks in
`crates/cadkit-wasm/.tools/` (git-ignored; unpack the prebuilt release archive from
`github.com/wasm-bindgen/wasm-bindgen/releases` there), then on `PATH`.

Size: the workspace release profile is shared, so `build.py` applies size settings through
`CARGO_PROFILE_RELEASE_*` environment variables (`opt-level=z`, fat LTO, 1 codegen unit,
`panic=abort`). If `wasm-opt` (binaryen) is on `PATH`, it additionally runs `wasm-opt -Oz`.
Current `.wasm`: about 1.8 MB (DWG and DGN readers now included) without `wasm-opt`.

## JS API

Data crosses the boundary as JSON text parsed with `JSON.parse` (via `js-sys`) rather than
`serde-wasm-bindgen`: the model is already `Serialize`, the JSON shape is identical to the
CLI and Python output, and one fewer dependency keeps the module small.

```js
import init, { detect, read } from "./pkg/cadkit_wasm.js";   // web target
await init();
// Node: const { detect, read } = require("./pkg-node/cadkit_wasm.js");

detect(bytes)                       // "dwg" | "dxf" | "dgn_v7" | "dgn_v8" | undefined
const doc = read(bytes, { keepRaw: false, codepage: "windows-1254",
                     maxInputBytes, maxDecompressedBytes, maxObjects, maxDepth });  // all optional

doc.info()      // {format, version, application, units, metersPerUnit,
                //  models: [{index, name, entities, is3d}], layerCount, blockCount, warningCount}
doc.layers()    // [{name, color, visible, frozen, locked}]
doc.warnings()  // [{code, message, offset}]
doc.toJSON()    // full document as an object; toJsonString(pretty) gives text
doc.toSvg({ model: 0, widthPx: 1600, strokePx: 1, background: "#fff", text: true, images: true, expandBlocks: true })
doc.toDxf("r2018")   // "r12" | "r2000" | "r2004" | "r2007" | "r2010" | "r2013" | "r2018"
doc.free()
```

Errors are thrown as `Error` objects with a `kind` property: `unknown_format`, `unsupported`,
`invalid`, `truncated`, `limit_exceeded` (from the readers), plus `io` and `bad_argument`
(bad `model` index or DXF version). `toSvg({background: null})` gives a transparent SVG.

Limits: wasm32 aborts on allocation failure, so `read` uses conservative defaults instead of
the desktop ones: input 512 MiB, decompressed 1 GiB, 5,000,000 objects, depth 64. Override with
the `max*` options. If a call still traps (`WebAssembly.RuntimeError`) the module instance is
unusable and must be re-instantiated; the viewer does this automatically.

`build.py` also writes `package.json` into `pkg/` (name `cadkit`, ES module) and `pkg-node/`
(name `cadkit-node`, CommonJS), so `npm pack` works in either folder.

## Tests

```
python scripts/buildlock.py node --test crates/cadkit-wasm/tests/node_test.mjs
python scripts/buildlock.py cargo clippy --no-deps -p cadkit-wasm --target wasm32-unknown-unknown -- -D warnings
node --check crates/cadkit-wasm/www/viewer.js
```

The Node test needs `pkg-node/` (run `build.py` first). It covers garbage input, a synthetic
DXF, SVG well-formedness and every file under `corpus/public/`; reader-dependent checks skip
when a reader reports `unsupported` or `unknown_format`.

## Viewer

```
cd crates/cadkit-wasm
python -m http.server 8000      # then open http://localhost:8000/www/
```

Serve the crate folder (not `www/` alone) because the viewer imports `../pkg/`. Opening
`index.html` from `file://` does not work (browsers block module and wasm loading).

Features: drop zone and file picker (.dwg, .dgn, .dxf); pan by dragging, zoom with the wheel,
pinch or +/- buttons, all through the SVG `viewBox`; fit-to-view (button, `0`/`F`, double
click); keyboard pan with arrow keys; layer panel with per-layer visibility from the
`data-layer` groups (layers that render nothing, e.g. frozen, are shown disabled); model
selector and text toggle; information panel with format, version, units, counts and
warnings; SVG, DXF and JSON export; light/dark theme from `prefers-color-scheme`; responsive
layout with a collapsible panel on phones. The canvas is a white sheet:
the renderer paints on a white background (`background: "#ffffff"`).

## DGN V8 and CityGML

`CadDocument.fromJsonString(text)` constructs a neutral model. `toDgn(seed,
optionsJson?)` returns `Uint8Array`; `toCityGml(srsName, lod)` returns UTF-8 bytes.
DGN options use Rust `dgn::WriteOptions` JSON, including `clear_seed_model`.

`readCityGml(bytes, optionsJson?)` returns a native `CityGmlDocument` with
`toGml(optionsJson?)`, `toJsonString()`, static `fromJsonString(text)`,
`toDocument(lod?)` and `validate(optionsJson?)`. Call `free()` on both document
classes when done. Native read options use Rust `ReadOptions`; native write and
validation options use `gml::ValidationOptions`.

New native APIs cap input at 512 MiB, decompressed data at 1 GiB, objects at five
million and depth at 64 even if larger JSON limits are supplied. Runtime schema
fetching and implicit CRS transformation are not performed. The Node test suite
includes native GML preservation, new generic output and public-seed DGN writing.
