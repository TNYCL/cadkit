# cadkit-wasm

`cadkit-wasm` is cadkit compiled to WebAssembly, for browsers, workers and Node. It reads DWG,
DGN V7/V8, DXF and CityGML 2.0 and writes DXF, seed-based DGN V8, CityGML 2.0, SVG and JSON,
locally, without a conversion service (see the [format matrix](../README.md#format-support)
for what each format covers). The standalone Rust geometry, scene and GPU APIs are not part
of the package.

This page is the reference of the JavaScript API. Build notes for maintainers are at the end.

## Install and load

```sh
npm install cadkit-wasm
```

The package holds one `.wasm` module and two glue builds: `web/` (ES module, instantiated by
`init()`) and `node/` (CommonJS, instantiated when loaded). The package root picks the right
one:

| Import | Resolves to |
|---|---|
| `import ... from "cadkit-wasm"` in a browser or bundler | `web/`: call `await init()` once |
| `import ... from "cadkit-wasm"` in Node | `node/cadkit_wasm.mjs`: ready on import; `init()` and `initSync()` exist and do nothing |
| `require("cadkit-wasm")` in Node | `node/`: ready on load |
| `cadkit-wasm/node` | `node/` (CommonJS) |
| `cadkit-wasm/web` | `web/`, also under Node (pass the module bytes to `init`) |
| `cadkit-wasm/cadkit_wasm_bg.wasm` | the module file |

Browser (served over HTTP; `file://` cannot load modules):

```js
import init, { read } from "cadkit-wasm";

await init();                                   // fetches cadkit_wasm_bg.wasm next to the module
const doc = read(await file.arrayBuffer());
console.log(doc.info());
const svg = doc.toSvgBounded({ maxOutputBytes: 16 << 20 });
doc.free();
```

Bundlers (Vite, webpack 5, esbuild) resolve `new URL("cadkit_wasm_bg.wasm", import.meta.url)`
inside the glue; when a bundler does not, give the URL explicitly:

```js
import init from "cadkit-wasm";
import wasmUrl from "cadkit-wasm/cadkit_wasm_bg.wasm?url";   // Vite syntax
await init({ module_or_path: wasmUrl });
```

Web Worker: load the module inside the worker (as above) and keep documents there; post
results (SVG text, JSON values) to the page. A worker can be terminated and recreated to get
a fresh module instance, which is the only way to give memory back (see
[Limits and memory](#limits-and-memory)).

Node:

```js
import { read } from "cadkit-wasm";             // or: const { read } = require("cadkit-wasm");
import { readFileSync } from "node:fs";

const doc = read(readFileSync("plan.dwg"));
console.log(doc.layers());
doc.free();
```

Code written for the browser runs unchanged in Node: `await init()` resolves immediately
there. To use the web build under Node, pass the bytes:
`await init({ module_or_path: readFileSync(require.resolve("cadkit-wasm/cadkit_wasm_bg.wasm")) })`
with `init` imported from `cadkit-wasm/web`.

## Conventions

- **Bytes.** Every byte argument accepts a `Uint8Array` (and Node `Buffer`), any other typed
  array or `DataView` (its bytes, not its elements), an `ArrayBuffer` or a
  `SharedArrayBuffer`. The bytes are copied into wasm memory; the length is checked against
  `maxInputBytes` first.
- **Options** are a plain object or JSON text of one, with the same keys. Keys are camelCase;
  the snake_case spelling of every key is accepted too (`maxObjects` or `max_objects`; giving
  both is an error). `undefined`, `null` and `""` mean "no options"; a key set to
  `undefined` or `null` is not set. A value of the wrong type, `NaN`, an infinite number, or a
  negative or fractional number where an integer is needed throws `bad_argument` naming the
  option. **An unknown key is reported once per call with `console.warn` and otherwise
  ignored in this release; from the next minor release it is a `bad_argument` error.**
  (`stripDgnSeed` options already reject unknown keys.)
- **Arguments** are validated: a wrong type (a number for a string, `NaN` for a model index,
  an array for an object, ...) throws `bad_argument` and never reaches the module's memory.
- **Coordinates** are in drawing units (`info().units`, `metersPerUnit`), angles in radians.
- **Entity paths** say where an entity sits: an index of the model's entity list, or an
  array of indices that steps into a group's `children` or into the block of an `insert` (or
  of a `dimension` with a block). `[3]` is `toJSON().models[m].entities[3]`; `[3, 1]` is its
  second group child, or the second entity of the block it inserts. Every cell of an insert
  array has the same path.
- **JSON shape.** Entities, blocks and other model values have the document schema's shape
  ([docs/schema/document-v1.schema.json](schema/document-v1.schema.json), snake_case keys,
  `kind.type` tags such as `"line"` and `"m_text"`). Summaries and results around them use
  camelCase like `info()`.
- **Mutation.** Only `convertUnits`, `explodeInserts`, `transform`, `retainLayers`,
  `retainKinds` and `purgeUnused` change a document, in place; use `clone()` first to keep
  the original. Every other method only reads.
- **free().** `CadDocument` and `CityGmlDocument` live in wasm memory. Call `free()` (or use
  `using`) when done; the garbage collector does not reclaim them in time.

## Functions

| Function | Result |
|---|---|
| `detect(bytes)` | `"dwg" \| "dxf" \| "dgn_v7" \| "dgn_v8" \| "city_gml" \| undefined` from the header |
| `inspect(bytes, options?)` | `Summary` from the header only: `{format, version, application, codepage, units: {unit, meters_per_unit} \| null, is_3d}` (snake_case keys of the Rust `Summary`) |
| `read(bytes, options?)` | `CadDocument`; `options`: [read options](#read-options-and-limits) |
| `readCityGml(bytes, options?)` | `CityGmlDocument` (lossless native CityGML) |
| `stripDgnSeed(seed, options?, readOptions?)` | `{seed: Uint8Array, report}`, see [DGN seeds](#dgn-v8-and-citygml) |
| `memoryBytes()` | current size of the module's wasm memory in bytes |

### Read options and limits

| Key | Type | Default | Ceiling | `preset: "strict"` |
|---|---|---|---|---|
| `maxInputBytes` | integer | 512 MiB | 512 MiB | 64 MiB |
| `maxDecompressedBytes` | integer | 1 GiB | 1 GiB | 256 MiB |
| `maxObjects` | integer | 1,500,000 | 5,000,000 | 500,000 |
| `maxDepth` | integer | 64 | 64 | 32 |
| `maxStringBytes` | integer | 16 MiB | 64 MiB | 4 MiB |
| `maxVertices` (one entity) | integer | 10,000,000 | 10,000,000 | 1,000,000 |
| `maxTotalVertices` (document) | integer | 50,000,000 | 50,000,000 | 5,000,000 |
| `preset` | `"default"` \| `"strict"` | `"default"` | | |
| `limits` | object with the keys above | | | |
| `keepRaw` | boolean | `false` | | keep record bytes in `entity.raw` |
| `codepage` (alias `fallbackCodepage`) | string \| null | windows-1252 | | 8-bit code page when the file declares none |

A value above its ceiling is lowered to the ceiling. Keys given with `preset` override the
preset; keys in a nested `limits` object override top-level ones. `preset: "strict"` is meant
for untrusted uploads. Every function that takes limits (`read`, `inspect`, `readCityGml`,
`CadDocument.fromJsonString`, `CadDocument.fromCityGml`, `stripDgnSeed`'s `readOptions`,
`toDgn`, `CityGmlDocument.toGml` and `validate`) reads them with the same rules; writers and
validators start from 5,000,000 objects instead of 1,500,000.

## CadDocument

### Construction

| Method | Result |
|---|---|
| `read(bytes, options?)` | from a drawing file |
| `CadDocument.fromJsonString(text, options?)` | from document JSON (`toJsonString()`); `options`: the limits |
| `CadDocument.fromCityGml(bytes, lod?, options?)` | from CityGML, optionally only `lod` 0 to 4 |
| `doc.clone()` | an independent deep copy (as much memory again) |

### Reading without converting the whole document

`toJSON()` copies everything (66,000 entities: about 0.35 s and 70 MB of JS heap). These copy
only what is asked for:

| Method | Result |
|---|---|
| `info()` | `{format, version, application, units, metersPerUnit, models: [{index, name, entities, is3d}], layerCount, blockCount, warningCount}` |
| `layers()` | `[{name, color, visible, frozen, locked}]` |
| `warnings()` | `[{code, message, offset}]` recorded while reading |
| `models()` | `[{index, name, id, kind, is3d, entities, props}]` (`entities`: top-level count) |
| `entityCount(model)` | number of top-level entities |
| `entity(model, path)` | `Entity` or `null`; an insert is returned as the insert |
| `entities(model, options?)` | one page `{total, offset, next, entities: [{index, entity}]}` |
| `blocks()` | `[{name, id, entities, basePoint, description, isXref, xrefPath}]` |
| `block(name)` | `Block` (with its entities) or `null` |
| `stats(model?)` | `{entities, nested, blockEntities, kinds, unknownTypes, layers}` |
| `bbox(model, path?)` | `{min: [x, y, z], max: [x, y, z]}` or `null` |
| `viewports(model)` | `[{path, entity}]` of the viewport entities (paper-space layouts) |
| `toJSON()` | the whole document (`DocumentJson`); `JSON.stringify(doc)` uses it |
| `toJsonString(pretty?)` | the same as text |

`entities(model, options)` options: `offset` (default 0) and `limit` (default 1,000, at most
100,000) count entities that pass the filters `layers` (own layer names; the default layer is
`"0"`) and `kinds` (`type` tags; `"mtext"` is accepted for `"m_text"`, an unknown kind is an
error); `maxOutputBytes` (default 16 MiB, at most 256 MiB) bounds the JSON size of a page. A
page ends early when the next entity would not fit; `next` is the offset to continue from, or
`null` after the last page. An entity larger than `maxOutputBytes` on its own throws
`limit_exceeded`. `index` is the entity's index in the model.

`stats(model)` counts one model's tree: `entities` (top level), `nested` (group children at
any depth), `kinds` per `type` tag (nested ones included), `unknownTypes` (the source type
names of `unknown` entities, such as `"dwg.ACAD_TABLE"`), `layers` (entities per own layer).
Without `model` it counts every model and, in `blockEntities`, the block definitions.

`bbox(model)` is the box of what is drawn on visible layers, as `toSvg` frames it.
`bbox(model, path)` is the box of everything drawn for that entity on any layer: for an insert
all its array cells, for a block part its copies in every cell.

### Picking

`pick(model, x, y, options)` returns the entity closest to the world point `(x, y)`, or
`null`:

| Option | Default | |
|---|---|---|
| `tolerance` | required | largest distance, in drawing units |
| `maxOutputBytes` | 65,536 | complete JSON size of the result, 64 to 65,536 |
| `maxWork` | 5,000,000 | traversal budget, 1 to 5,000,000 |
| `includeHiddenLayers` | `false` | also hidden and frozen layers |
| `excludedLayers`, `excludedKinds` | | at most 20,000 names of at most 2,048 UTF-16 units and 65,536 bytes per array |

The result (`PickHit`):

| Field | |
|---|---|
| `path` | path of the entity that was hit: `entity(model, path)` returns it (for geometry drawn from a block, the block part) |
| `modelPath` | path of the model entity that owns it: the outermost insert for a block part, otherwise equal to `path` (groups belong to the model's tree) |
| `kind`, `layer`, `id` | of the hit (`layer` is the effective one: block parts on layer `"0"` take the insert's layer) |
| `modelKind`, `modelLayer`, `modelId` | of the model entity |
| `block` | block name of the model entity (or of the hit when it is an unexpanded insert), or `null` |
| `attributes` | `[{tag, value}]` of the model entity, values as text previews |
| `color`, `text`, `bbox`, `distance` | display color `#rrggbb`, text of a text entity, model-space box, distance |
| `truncated` | a preview was shortened (layer and block names 200 characters, text 500, tags 100, values 200, at most 20 attributes and 8 KiB of preview text) |

Distances are exact for lines and circular arcs, sampled for other curves and approximated by
the box for text and hatches. Picking follows block transforms and layer visibility; it is a
selection aid, not a snapping API. An exhausted `maxWork` or `maxOutputBytes` throws
`limit_exceeded` (`limit: "maxWork"` or `"maxOutputBytes"`), also when a candidate was
found: a partial search is never returned as a result.

**Deprecated:** `pickEntity(model, wx, wy, tolerance, includeHiddenLayers?, options?)` and
`pickEntityBounded(model, wx, wy, tolerance, options, includeHiddenLayers?)` keep working
unchanged. Their `index` is the hit's position in the walk's visiting order (block contents
and group children included), **not** an index of `toJSON().models[m].entities`: on drawings
with blocks or groups it names a different entity. Use `pick` and its `path`.
`pickEntity` copies attributes and `props` in full; `pickEntityBounded` caps them as `pick`
does and requires `maxOutputBytes` (1 to 65,536).

### Operations that change the document

| Method | Effect, result |
|---|---|
| `convertUnits(unit)` | scales coordinates, blocks, dash lengths and fixed text heights to `unit` (`"mm"`, `"meter"`, `"ft"`, ...); returns the factor |
| `explodeInserts(options?)` | replaces inserts by their block content in model space; `{truncated, work, entities}` |
| `transform(spec)` | affine transform of the design models; returns nothing |
| `retainLayers(names)` | keeps those layers and the model entities on them; `{layers, entities}` removed |
| `retainKinds(kinds)` | keeps model entities of those `type` tags; `{layers: 0, entities}` removed |
| `purgeUnused()` | removes unreferenced blocks, layers (except `"0"`), linetypes and text styles; `{blocks, layers, linetypes, textStyles}` removed |

`explodeInserts` options: `maxDepth` (nesting expanded, default 32, at most 64) and
`maxEntities` (entities the expansion may add, default 1,000,000, at most 3,000,000; an entity
counts one plus one per 16 vertices, so that the budget bounds memory). The expansion is
counted before anything changes: when it would exceed `maxEntities` the call throws
`limit_exceeded` with `limit: "maxEntities"` and `needed`, and the document is unchanged.
`truncated` is `true` when the traversal's own work budget still stopped the expansion (the
inserts not reached stay inserts). Missing blocks, cycles and inserts deeper than `maxDepth`
stay inserts.

`transform(spec)`: a row-major affine matrix (12 numbers or 3 rows of 4; 16 numbers or 4 rows
of 4 whose last row is `0, 0, 0, 1`), `{matrix}`, or parts applied as scale, then rotation,
then translation: `{scale: s | [sx, sy] | [sx, sy, sz], rotateZ: radians, translate: [x, y] |
[x, y, z], origin: [x, y] | [x, y, z]}` (`origin`: the fixed point of scale and rotation).
Block definitions stay in block coordinates; inserts absorb the transform. A singular or
non-finite matrix throws `bad_argument`.

`retainKinds`: an entity whose kind is listed is kept with everything below it; a group whose
kind is not listed keeps the children that pass and disappears when none does. Block
definitions are not changed. `retainLayers` compares the entity's own layer (the default layer
is `"0"`).

### Export

| Method | Result |
|---|---|
| `toSvg(options?)` | SVG text; very large drawings are cut at an internal body size with a marker |
| `toSvgBounded(options)` | complete SVG within `maxOutputBytes`, or `limit_exceeded` |
| `toDxf(version?)` | DXF text; `version` `"r12"`, `"r2000"`, `"r2004"`, `"r2007"`, `"r2010"`, `"r2013"`, `"r2018"` (default) |
| `toDxfWithReport(version?)` | `{text, warnings}` |
| `toDxfBytes(version?, codepage?)` | `Uint8Array`; `codepage` (`"windows-1254"`, `"DOS857"`, ...) for versions before R2007 |
| `toDxfBytesWithReport(version?, codepage?)` | `{bytes, warnings}` |
| `toDgn(seed, options?)` | DGN V8 `Uint8Array` written into a copy of `seed` |
| `toCityGml(srsName, lod)` | CityGML 2.0 XML bytes |
| `toCityGmlReport(srsName, lod, metadataOnly?)` | `{xml, report}` |
| `toCityGmlWithReport(srsName, lod, metadataOnly?)` | deprecated: the same as JSON text |

DXF cannot hold every entity: records with no DXF writer (for example `dwg.MESH`,
`dwg.MULTILEADER` from DWG) are left out. `toDxf` leaves them out silently; the
`WithReport` variants list one warning `{code: "dxf.write.skipped", message, offset}` each.

SVG options: `model` (default 0), `widthPx` (1600), `strokePx` (1), `background` (CSS color,
default `"#ffffff"`; `null`, `""` or `"none"` for transparent), `text`, `images`, `expandBlocks` (all `true`),
`includeHiddenLayers` (`false`). `toSvgBounded` adds `maxOutputBytes` (required, 1 to
2^32 - 1; the complete UTF-8 SVG, markup included) and `maxWork` (traversal budget). Its
temporary buffers are checked against the cap before they grow; exhaustion throws
`limit_exceeded` and never returns a partial drawing. The cap does not bound the parsed
document. Use `toSvgBounded` for untrusted drawings.

## DGN V8 and CityGML

`toDgn(seed, options)` options (camelCase or snake_case): `clearSeedModel` (allow clearing a
non-empty seed model), `codepage` (8-bit code page for text; `null` writes UTF-16LE),
`timestampMs` (0 to 1e15), `planarityTolerance` (drawing units, default 0.01),
`preserveSeedRasters`, `preserveSeedControls` (both `true`) and the limit keys.

`stripDgnSeed(seed, options?, readOptions?)` copies a DGN V8 seed without its model's drawing
content and without the document properties that name people or organisations. Options:
`keepControls`, `keepRasterTemplate` (both `false`), `keepCellDefinitions` (`true`),
`modelName` (1 to 249 UTF-16 units, no NUL), `timestampMs` (0 to 1e15), `application` (at
most 255 units, default `"cadkit"`); an unknown key throws `bad_argument`. `readOptions` sets
the read limits. The report (`DgnStripSeedReport`) counts what was removed and what remains;
names in the kept tables remain, so the result is not anonymous. See
[obtaining a seed](CADKIT_INTEGRATION.md#obtaining-a-seed).

`toCityGml(srsName, lod)` writes generic city objects with the CRS name of the existing
coordinates (no reprojection, no axis swap) and `lod` 0 to 4. With `metadataOnly` `false`
(the default) an entity without CityGML geometry is an `unsupported` error; with `true` it is
kept as JSON metadata and listed in `report.issues` (`{code, model, path, entity_type}`).
`report` also counts `entities`, `geometry_entities` and `metadata_only_entities`.

`CityGmlDocument` (from `readCityGml`): `toGml(options?)` (XML bytes), `toJsonString()`,
`CityGmlDocument.fromJsonString(text)`, `toDocument(lod?)` (a `CadDocument`),
`validate(options?)` (`{elements, ids, references, polygons, interiors, positions,
attributes}`) and `preflightTkgm(profile)` with `"city-model-tender"` or
`"digital-building-registration"` (a local structural preflight; `acceptance_verified` is
always `false`). Validation options: `geometry` (`true`), `tolerance` (1e-6),
`planarityTolerance` (0.01), `maxGeometryWork` and the limit keys. Schemas are never fetched.

## Errors

Every function throws a JS `Error` (`CadkitError` in the typings) with:

| Property | |
|---|---|
| `kind` | one of the kinds below (closed list) |
| `message` | human-readable; do not parse it |
| `offset` | absolute byte offset in the input, when the cause has one (`invalid`, `truncated`) |
| `limit` | for `limit_exceeded`: the option that bounds what was exceeded (`"maxObjects"`, `"maxInputBytes"`, `"maxWork"`, `"maxOutputBytes"`, `"maxEntities"`, ...) |
| `limitValue` | for `limit_exceeded`: that option's value in this call, when known |
| `needed` | bytes still needed (`truncated`), or what the call needed (`limit_exceeded`), when known |

| Kind | Meaning |
|---|---|
| `bad_argument` | an argument or option has the wrong type or value; the message names it |
| `unknown_format` | the bytes are not a supported format |
| `unsupported` | valid input that uses something cadkit does not implement, or output the target format cannot hold |
| `invalid` | the input violates its format (also document JSON that does not match the schema) |
| `truncated` | the input ends inside a structure |
| `limit_exceeded` | a limit or budget was reached |
| `io` | an I/O error |

## Limits and memory

wasm32 has a 4 GiB address space and an allocation failure aborts the module
(`WebAssembly.RuntimeError`, "unreachable" or "memory access out of bounds"): the instance is
then unusable and must be recreated. The limits keep allocations below that:

- A document costs about **900 bytes of wasm memory per entity** at its peak while reading
  (measured on one million points, lines and texts read from DXF), more for entities with
  many vertices. The default `maxObjects` of 1,500,000 therefore keeps a document at about
  1.35 GB, which leaves room for the input bytes, an export and a second document. Raise it
  (up to 5,000,000) only for trusted drawings in a dedicated worker.
- `explodeInserts` refuses expansions larger than `maxEntities` before it allocates;
  `toSvgBounded`, `pick` and `entities` have output and work budgets.
- **The memory never shrinks.** `free()` returns memory to the module's allocator, which
  reuses it, but the wasm memory keeps its largest size until the instance is dropped.
  `memoryBytes()` reports the current size; a host that sees it grow past what it wants to
  hold (for example 2 GiB) should terminate the worker and start a new one.

Traps are not caught inside the module. Keep untrusted work in a worker and recreate the
worker after a `WebAssembly.RuntimeError`.

## TypeScript

The package ships declarations for every function, option and result. The document model
(`DocumentJson`, `Entity`, the discriminated union `EntityKind` with members such as
`EntityKindLine` and `EntityKindInsert`, `Block`, `Layer`, `Color`, `Value`, ...) is generated
from the JSON schema; narrowing on `entity.kind.type` gives the kind's fields. Errors are
`CadkitError` with `kind: CadkitErrorKind`.

## Build notes

```
python crates/cadkit-wasm/build.py [--check-size] [--debug-names]
```

The script runs `cargo build --release --target wasm32-unknown-unknown` through
`scripts/buildlock.py` (do not wrap `build.py` in the lock again) with size settings applied by
`CARGO_PROFILE_RELEASE_*` variables (`opt-level=z`, fat LTO, one codegen unit,
`panic=abort`), then runs the `wasm-bindgen` CLI for `--target web` (`pkg/`, used by the
viewer) and `--target nodejs` (`pkg-node/`, used by the tests). The CLI version must equal the
`wasm-bindgen` version in `Cargo.lock`; `build.py` looks in `crates/cadkit-wasm/.tools/`
(git-ignored), then on `PATH`.

- **Names.** The "name" custom section (function names, about 2 MB, nearly half of the
  module) is removed from the shipped modules. `--debug-names` keeps it, for stack traces with
  Rust function names.
- **wasm-opt** (binaryen) runs with `-Oz` when it is on `PATH`; it is optional. Its
  `--enable-*` flags are derived from the target features `rustc --print cfg --target
  wasm32-unknown-unknown` reports, so that wasm-opt accepts every instruction the compiler
  emits.
- **Size budget.** `--check-size` fails when the web module exceeds `SIZE_BUDGET` in
  `build.py` (2,700,000 bytes; 2,438,423 when it was set). CI runs it.
- **TypeScript.** `ts/api.d.ts` (options, results, errors) and `ts/document.d.ts` are added to
  the generated declarations. `ts/document.d.ts` is generated:
  `node crates/cadkit-wasm/scripts/schema-to-ts.mjs` (`--check` fails when it is stale).
- **npm package.** `build.py` assembles `crates/cadkit-wasm/npm/` (git-ignored): `web/` with
  the one `.wasm`, `node/` with the CommonJS glue patched to load `../web/cadkit_wasm_bg.wasm`
  and an ESM entry `node/cadkit_wasm.mjs`, and `package.json` with the `node` condition on
  the root export. `build.py` stops when the web and Node modules differ or the glue changed
  shape. The package README is `crates/cadkit-wasm/README.md`.

Tests:

```
node --test --test-concurrency=1 "crates/cadkit-wasm/tests/*_test.mjs"
node crates/cadkit-wasm/tests/package_smoke.mjs --pack crates/cadkit-wasm/npm
python scripts/buildlock.py cargo clippy -p cadkit-wasm --all-targets -- -D warnings
python scripts/buildlock.py cargo clippy -p cadkit-wasm --target wasm32-unknown-unknown --all-targets -- -D warnings
node --check crates/cadkit-wasm/www/viewer.js
```

The Node tests need `pkg/` and `pkg-node/` (run `build.py` first). `node_test` covers
readers and exports, `contract_test` the option parser, argument validation and error
properties, `document_test` the access API against `toJSON()`, pick paths and document
operations, `typings_test` the declarations against the runtime and the samples,
`memory_test` inputs that used to exhaust the address space (it grows the module to about
1.5 GB), `mutation_test` a deterministic run of truncated and bit-flipped samples. They use
the committed site samples (`site/public/samples`), and the third-party samples and
`corpus/public/` (or the directory in `CADKIT_CORPUS`) when present. `package_smoke.mjs` packs
`npm/`, installs the tarball into a temporary project and imports it every supported way;
the release workflow runs it from the install directory.

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
`data-layer` groups (layers that render nothing are shown disabled; Include hidden layers
renders hidden/frozen layers too); model selector, text/image toggles and fallback code-page
selection; world-coordinate HUD and click selection; information panel with format, version,
units, counts and warnings; SVG, DXF, GML and JSON export; light/dark theme from
`prefers-color-scheme`; responsive layout with a collapsible panel on phones. GML export asks
for the CRS identifier of the existing coordinates; unsupported geometry is rejected unless
the metadata option is checked. Browser appearance and pointer interaction need interactive
QA; the Node tests exercise the module contract only.
