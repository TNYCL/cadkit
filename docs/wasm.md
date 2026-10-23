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
`build.py` prints the measured size of each generated module; archive size depends on the
selected features, toolchain, and optional optimizer.

## JS API

Data crosses the boundary as JSON text parsed with `JSON.parse` (via `js-sys`) rather than
`serde-wasm-bindgen`: the model is already `Serialize`, the JSON shape is identical to the
CLI and Python output, and one fewer dependency keeps the module small.

```js
import init, { detect, inspect, read, readCityGml } from "./pkg/cadkit_wasm.js";   // web target
await init();
// Node: const { detect, read, readCityGml } = require("./pkg-node/cadkit_wasm.js");

detect(bytes)                       // "dwg" | "dxf" | "dgn_v7" | "dgn_v8" | "city_gml" | undefined
const summary = inspect(bytes);     // header-only format/version/units metadata
const doc = read(bytes, { keepRaw: false, codepage: "windows-1254",
                     maxInputBytes, maxDecompressedBytes, maxObjects, maxDepth });  // all optional

doc.info()      // {format, version, application, units, metersPerUnit,
                //  models: [{index, name, entities, is3d}], layerCount, blockCount, warningCount}
doc.layers()    // [{name, color, visible, frozen, locked}]
doc.warnings()  // [{code, message, offset}]
doc.toJSON()    // full document as an object; toJsonString(pretty) gives text
doc.toSvg({ model: 0, widthPx: 1600, strokePx: 1, background: "#fff", text: true, images: true, expandBlocks: true, includeHiddenLayers: false })
doc.toDxf("r2018")   // "r12" | "r2000" | "r2004" | "r2007" | "r2010" | "r2013" | "r2018"
doc.toDgn(seedBytes, { clearSeedModel: true })   // Uint8Array
doc.toCityGml("urn:ogc:def:crs:EPSG::4979", 1)                     // Uint8Array (UTF-8 XML)
doc.toCityGmlWithReport("urn:ogc:def:crs:EPSG::4979", 1, false)    // JSON string: {xml, report}
doc.pickEntity(0, 10, 5, 0.5) // closest visible entity summary or null; world XY coordinates
// Optional fifth argument includes hidden/frozen layers; sixth accepts
// {excludedLayers: ["WALLS"], excludedKinds: ["text"]} for viewer display filters.
doc.convertUnits("meter")    // mutates this document; returns the scale factor
doc.explodeInserts()         // mutates; returns {truncated, work}
doc.toDxfBytes("r2000", "DOS857") // Uint8Array with optional legacy output encoding
doc.free()
```

Errors are thrown as `Error` objects with a `kind` property: `unknown_format`, `unsupported`,
`invalid`, `truncated`, `limit_exceeded` (from the readers), plus `io` and `bad_argument`
(bad `model` index or DXF version). `toSvg({background: null})` gives a transparent SVG.

Limits: wasm32 aborts on allocation failure, so `read` uses conservative defaults instead of
the desktop ones: input 512 MiB, decompressed 1 GiB, 5,000,000 objects, depth 64. Override with
the `max*` options. If a call still traps (`WebAssembly.RuntimeError`) the module instance is
unusable and must be re-instantiated; the viewer does this automatically.

`build.py` writes a private development `package.json` into `pkg/` and `pkg-node/` and then
assembles the npm package `cadkit-wasm` in `crates/cadkit-wasm/npm/` (git-ignored):

| Import | Folder | Build |
|---|---|---|
| `cadkit-wasm` | `web/` | `--target web`, ES module; call `await init()` first |
| `cadkit-wasm/node` | `node/` | `--target nodejs`, CommonJS, ready on load |
| `cadkit-wasm/cadkit_wasm_bg.wasm` | `web/` | the module bytes, for `init({ module_or_path })` in Node |

The package README is `crates/cadkit-wasm/README.md`. `npm pack` in `npm/` produces the
tarball that the release workflow publishes; `tests/package_smoke.mjs` installs it and
exercises both entry points.

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
`data-layer` groups (layers that render nothing are shown disabled; Include hidden layers renders hidden/frozen layers too); model
selector, text/image toggles and fallback code-page selection; world-coordinate HUD and click selection; information panel with format, version, units, counts and
warnings; SVG, DXF, GML and JSON export; light/dark theme from `prefers-color-scheme`; responsive
layout with a collapsible panel on phones. The sheet background can be switched between light and dark. GML export requires the user to enter the CRS identifier of existing coordinates. Unsupported
geometry is rejected unless the user explicitly checks the metadata option; a diagnostic
message reports metadata-only exports.

## DGN V8 and CityGML

`CadDocument.fromJsonString(text)` constructs a neutral model. `toDgn(seed,
optionsJson?)` returns `Uint8Array`; `toCityGml(srsName, lod)` returns UTF-8 XML
bytes as `Uint8Array`; `toCityGmlWithReport(srsName, lod, metadataOnly)` returns a
JSON string `{"xml": ..., "report": ...}` (`metadataOnly: false` rejects
unsupported geometry; `true` retains unsupported entities as JSON metadata). DGN
options use Rust `dgn::WriteOptions` JSON, including `clear_seed_model`.

`readCityGml(bytes, optionsJson?)` returns a native `CityGmlDocument` with
`toGml(optionsJson?)`, `toJsonString()`, static `fromJsonString(text)`,
`toDocument(lod?)`, `validate(optionsJson?)`, and `preflightTkgm(profile)`.
`preflightTkgm(profile)` requires an explicit profile (`"city-model-tender"` or
`"digital-building-registration"`) and returns a local structural preflight report
object (`acceptance_verified` is always `false`). Call `free()` on both document
classes when done. Native read options use Rust `ReadOptions`; native write and
validation options use `gml::ValidationOptions`.

New native APIs cap input at 512 MiB, decompressed data at 1 GiB, objects at five
million and depth at 64 even if larger JSON limits are supplied. Runtime schema
fetching and implicit CRS transformation are not performed. The Node test suite
includes native GML preservation, new generic output and public-seed DGN writing.


The wasm-bindgen output includes TypeScript option/result interfaces. Read options and
DGN write options accept JS objects; native CityGML/DGN options also accept their existing
Rust JSON strings. `CadDocument.fromCityGml(bytes, lod?, options?)` converts native GML
geometry directly into a neutral document. All returned document objects need `free()`.

Picking uses exact XY distance for lines and circular arcs, bounded curve sampling for
other curves, and bounding-box approximations for annotations and hatches. It follows
nested block transforms and layer visibility; it is a viewer selection aid, not a CAD
snapping or geometric intersection API. Browser appearance and pointer interactions need
separate interactive QA; Node tests exercise the WASM contract only.
