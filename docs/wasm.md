# cadkit-wasm

`cadkit-wasm` is cadkit compiled to WebAssembly, for browsers, workers and Node. It reads DWG,
DGN V7/V8, DXF and CityGML 2.0 and writes DXF, seed-based DGN V8, CityGML 2.0, SVG and JSON,
locally, without a conversion service (see the [format matrix](../README.md#format-support)
for what each format covers). It also builds flat renderer buffers (triangle meshes and line
segments as typed arrays, see [Renderer buffers](#renderer-buffers)); the standalone Rust
geometry, plan and GPU APIs are not part of the package.

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
  (`stripDgnSeed` options and the methods added in this release, `surfaceMesh`, `lineBuffer`,
  `toCityGmlBuildings`, `toCityGmlBuildingsDocument`, `toDgnWithReport` and
  `prepareForDgn`, already reject unknown keys.)
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
- **free().** `CadDocument`, `CityGmlDocument`, `SurfaceMesh` and `LineBuffer` live in wasm
  memory. Call `free()` (or use `using`) when done; the garbage collector does not reclaim
  them in time. A method called after `free()` throws a plain `Error`.

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
`toDgn`, `toDgnWithReport`, `prepareForDgn`, the options of `toCityGml`, `toCityGmlReport`
and `toCityGmlBuildings`, `CityGmlDocument.toGml`, `validate` and `geometryIssues`) reads them
with the same rules; writers and validators start from 5,000,000 objects instead of
1,500,000.

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
The call does not explode part of a document to fit the budget: an expansion that fits is
done completely, one that does not is refused, so that `maxEntities` bounds memory and an
error never leaves a half-exploded document. `truncated` is `true` only when the traversal's
own work budget still stopped the expansion (unusual nesting the count underestimates): the
inserts not reached stay inserts, and an insert array that does not fit is kept whole as one
insert (its cells are never partly expanded). Missing blocks, cycles and inserts deeper than
`maxDepth` stay inserts.

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
| `toDgnWithReport(seed, options?)` | `{bytes, modelIndex, warnings}` |
| `prepareForDgn(seed, options?)` | a new `CadDocument`: one model prepared for `seed` |
| `toCityGml(srsName, lod, options?)` | CityGML 2.0 XML bytes |
| `toCityGmlReport(srsName, lod, metadataOnly?, options?)` | `{xml, report}` |
| `toCityGmlBuildings(options)` | `{xml: Uint8Array, report}`: buildings with wall, roof and ground surfaces |
| `toCityGmlBuildingsDocument(options)` | `{document: CityGmlDocument, report}` |
| `surfaceMesh(options?)` | a `SurfaceMesh` (see [Renderer buffers](#renderer-buffers)) |
| `lineBuffer(options?)` | a `LineBuffer` |
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

Options for interactive viewers (both methods; all off by default, and the output is then
unchanged):

| Option | |
|---|---|
| `origin` | `"auto"`, `[x, y]` or `{x, y}` (drawing units); `"none"` or `null` for none (default). Every coordinate (paths, text positions, rotation centres, the background, the `viewBox`, image frames) is written relative to the origin, and the root element carries `data-origin="ox oy"`. An SVG user-space point `(sx, sy)` is the drawing point `(ox + sx, oy - sy)`: X is shifted, Y shifted and flipped. Hatch pattern tiles are pattern-local and unaffected. `"auto"` is a round number near the centre of the drawing extents (the centre rounded to a multiple of the power of ten at or above the larger extent; `0 0` for a drawing around the origin or without geometry). Use it for drawings in map coordinates: browsers hold SVG coordinates in 32-bit floats, which cannot keep `x ≈ 500,000` to the precision of a drawing a few units across. |
| `entityRefs` | `true` tags every drawn element with `data-e="i.j.k"`, the path of the model entity it belongs to joined with dots: `entity(model, ref.split(".").map(Number))` returns it. Geometry drawn from a block carries the path of the outermost insert. Elements whose colour was replaced for contrast also carry the original colour as `data-c="#rrggbb"`; image frames carry `data-image-path` (the stored path) and `data-image-px="w h"` (pixel size) when known. |
| `pointPx` | draw points as round dots of this diameter in CSS pixels (greater than 0, at most 1,000) instead of crosses sized from the drawing extents; `null` for crosses. |
| `capHeightRatio` | cap height / font size of the display font, 0.05 to 20 (about 0.7 for common sans-serif fonts). CAD text height is the cap height, so `font-size` becomes `height / ratio`; it applies to text, fit text and attribute text, aligned text keeps the size that spans its baseline. `null`: `font-size` is the CAD height. |
| `fontFamily` | CSS `font-family` written on the root element and inherited by all text (at most 1,024 UTF-16 units); `""` or `null` writes none. |
| `hatchNativeSpacing` | space non-solid hatch pattern lines by the pattern's own spacing (3.175 drawing units times the pattern scale) instead of clamping it to 0.8 % to 8 % of the drawing extents; the spacing is raised where needed so that at most 500 lines cross the larger side of the hatch's extents. |
| `layoutViewports` | when the rendered model is a paper layout, draw each viewport entity as a window onto the first design model: the model is rendered once into `<defs>` and shown clipped in each viewport's sheet rectangle, scaled by `height / view_height` and centred on `view_center`, with a frame. The first viewport is skipped when it shows the sheet itself (as in DWG and DXF). |

Values out of range are `bad_argument` errors here (the Rust renderer ignores them).

## Renderer buffers

`doc.surfaceMesh(options?)` and `doc.lineBuffer(options?)` turn one model into flat buffers
for WebGL, WebGPU or canvas renderers, with blocks and arrays expanded and their placement
applied, colours resolved as in the SVG export, and every triangle and segment leading back
to its entity. They return a `SurfaceMesh` or `LineBuffer` that owns the buffers in wasm
memory; call `free()` on it when done.

`surfaceMesh` draws polygons (with holes), faces, every face of a mesh, solid hatches with
straight edges and, as `polylines` says, closed planar polylines as triangles; face outlines
and wires (lines, open polylines, curves flattened within `tolerance`) as segments. Two faces
with the same corners are drawn once (CityGML lists the faces of a solid and again each
wall, roof and ground surface; the nested copy wins and carries the surface type as its
layer). `lineBuffer` draws every curve as segments, polygon and face outlines included. What
neither draws (text, points, images, unexpanded inserts, ...) is counted in the report.

| `SurfaceMesh` method | Result (a copy each call) |
|---|---|
| `positions()` | `Float32Array`, x, y, z per vertex relative to the origin, drawing units |
| `normals()` | `Float32Array`, unit face normal per vertex (vertices are not shared between faces) |
| `colors()` | `Uint8Array`, sRGB per vertex |
| `indices()` | `Uint32Array`, three vertex indices per triangle, counter-clockwise about the normal |
| `triangleItems()` | `Uint32Array`, the item of each triangle |
| `segments()` | `Float32Array`, two end points per segment, x, y, z each |
| `segmentColors()` | `Uint8Array`, sRGB of both end points (6 bytes per segment) |
| `segmentItems()` | `Uint32Array`, the item of each segment |
| `bounds()` | `{origin, min, max, maxError}`: the f64 origin in drawing units, the box around all positions relative to it, the largest error of one stored coordinate |
| `layers()` | `[{name, firstTriangle, triangleCount, firstSegment, segmentCount}]`: the triangles and segments of one layer are contiguous, so a layer is hidden by skipping its range |
| `itemCount()`, `items()` | the item table as columns (below) |
| `item(i)` | `{index, path, modelPath, layer, layerIndex, kind, surface}` or `null` past the end |
| `report()` | `{faces, triangles, segments, wires, duplicates, degenerate, flattened, skipped, truncated}` |
| `pick(origin, direction, hiddenLayers?)` | the nearest triangle hit by the ray, `{triangle, item, distance}` or `null` |
| `byteLength()` | bytes of wasm memory the buffers hold |

`LineBuffer` has `components()` (2, or 3 with `threeD`), `segments()` (`components()` f32 per end
point), `segmentColors()`, `segmentItems()`, `bounds()`, `layers()` (empty triangle ranges),
`itemCount()`, `items()`, `item(i)`, `report()` (`{drawn, segments, flattened, degenerate,
skipped, truncated}`) and `byteLength()`.

Positions are f32 offsets from `bounds().origin` (the per-axis median of the model's points),
so map coordinates keep their precision; `origin + offset` differs from the drawing coordinate
by at most `maxError`. An item one of whose coordinates does not fit f32 relative to the
origin is left out and counted as degenerate, unless `maxError` is given, which turns any
larger error into `limit_exceeded` (`limit: "maxError"`).

**Items.** An item is one face (a polygon, a mesh face, a hatch region) or the wire geometry
of one entity. `items()` returns the table without one JS object per item:
`{count, pathStart, pathLength, modelPathLength, layer, kind, kindNames, surface, paths}`, where
item `i`'s path is `paths.subarray(pathStart[i], pathStart[i] + pathLength[i])`, its model path
the first `modelPathLength[i]` steps (the outermost insert for geometry drawn from a block),
`layer[i]` an index into `layers()`, `kindNames[kind[i]]` its JSON `type` tag and `surface[i]` 1
for a face. Paths are [entity paths](#conventions): `entity(model, path)` returns the entity
drawn. `item(i)` returns one item as an object.

**Picking.** `pick(origin, direction, hiddenLayers?)` tests the ray `origin + t * direction`
(`t > 0`) against every triangle, both faces counting, in f64 on the stored positions;
`origin` is relative to `bounds().origin` like the positions, both are arrays (or typed
arrays) of three finite numbers, and `hiddenLayers` lists indices of `layers()` to ignore.
`distance` is the ray parameter (drawing units when `direction` has unit length).

Options (unknown keys are errors):

| Option | Default | |
|---|---|---|
| `model` | 0 | model index |
| `maxDepth` | 32 | block and group nesting expanded, at most 64 |
| `expandInserts` | `true` | expand block references |
| `maxWork` | 20,000,000 | walk budget |
| `tolerance` | 0.1 mm in drawing units (1e-4 when unitless) | chordal error of flattened curves |
| `maxError` | none | largest error of a stored coordinate, drawing units |
| `maxSegments` | 2,000,000 | most segments, at most 4,000,000 |
| `polylines` (mesh) | `"auto"` | closed polylines as faces: `"auto"` (in models flagged 3D: DGN shapes and 3D building models), `"always"`, `"never"` |
| `planarity` (mesh) | 1e-3 | largest distance of a polyline face vertex from its plane, as a fraction of its bounding-box diagonal |
| `wires`, `outlines` (mesh) | `true` | draw wires, draw face outlines |
| `neutralColor` (mesh) | `[201, 204, 210]` | colour of faces that resolve to black (ACI 7, no colour); `null` keeps black |
| `lineBackground` (mesh) | `null` | background segment colours are contrasted against |
| `maxTriangles` (mesh) | 2,000,000 | most triangles, at most 4,000,000 |
| `maxGeometryWork` (mesh) | 200,000,000 | budget of duplicate detection, hatch nesting and triangulation |
| `threeD` (lines) | `false` | store z |
| `background` (lines) | `"#ffffff"` | background colours are contrasted against; `null` keeps them |

Colours are `[r, g, b]` (integers 0 to 255) or `"#rrggbb"`. Larger budgets than the ceilings
are lowered to them. When a budget runs out, the buffers hold a prefix of the model in walk
order and `report().truncated` is `true`.

**Memory.** The buffers stay in wasm memory until `free()`; every accessor copies into a new
typed array in the JS heap (it does not grow the wasm memory), so read each buffer once and
keep the copy. In wasm memory a vertex costs 27 bytes (position, normal, colour), a triangle
16 (indices and item), a 3D segment 34 (a 2D one 26), an item about 28 plus 4 per path step;
`byteLength()` reports the total. Measured: 500,000 quadrilateral faces with outlines
(1,000,000 triangles, 2,000,000 segments) hold 154 MB and grew the module by 185 MB at the
peak, on top of the 580 MB of the document; building them took about one second in Node.
At the ceilings the finished buffers hold at most about 0.65 GB (4,000,000 isolated
triangles and 4,000,000 segments), more while they are built.

## DGN V8 and CityGML

`toDgn(seed, options)` writes one model into a copy of `seed`, a single-model DGN V8 file.
Options (camelCase or snake_case):

| Option | Default | |
|---|---|---|
| `clearSeedModel` | `false` | allow clearing a non-empty seed model |
| `codepage` | `null` | 8-bit code page for text; `null` writes UTF-16LE |
| `timestampMs` | 0 | modification time of new records, 0 to 1e15 |
| `planarityTolerance` | 0.01 | largest distance of polygon ring points from their plane, drawing units |
| `preserveSeedRasters`, `preserveSeedControls` | `true` | keep the seed's raster attachments and control records |
| `modelIndex` | | the model to write; by default the only model, or the only design model beside paper layouts (every DXF and DWG). It must be a design model. |
| `dimensionPolicy` | `"strict"` | a model of the other dimension than the seed: `"strict"` fails, `"promote_2d_to_3d"` writes a 2D model into a 3D seed (and a 3D one into a 2D seed only when every Z is 0), `"project_to_seed"` also projects 3D geometry onto `z = 0` for a 2D seed |
| `estimateTextLength` | `false` | write centred and right-aligned text without a stored length with the estimate `characters × height × width factor` instead of failing; other applications may draw such text shifted along its baseline |
| `prepare` | `false` | `true` or `{dimensionPolicy, explodeInserts, convertUnits, normalizeForDgn}`: write a prepared copy (see below) |
| limit keys | | as for `read` |

A document that did not come from the seed's own file usually needs preparation: all 40
public DXF and DWG samples of `corpus/public` write into the seed that `stripDgnSeed` makes
of `gdal/test_dgnv8.dgn` and read back with it, none without it. Preparing copies the
selected model (`modelIndex`, by default the first design model), explodes block references
(`explodeInserts`), converts to the seed's master unit when both scales are known
(`convertUnits`), reconciles 2D and 3D (`dimensionPolicy`, default `"promote_2d_to_3d"`, or the
writer's `dimensionPolicy` when given) and normalises colours, lineweights, linetypes, fonts,
widths, text alignment and similar to what DGN V8 can hold (`normalizeForDgn`); each step is
on by default. The copy costs as much memory as the model again during the call.
`prepareForDgn(seed, options?)` returns that copy as a new `CadDocument` (options:
`modelIndex`, the four steps, `codepage` of the seed and the limit keys), to inspect or edit it
before `toDgn(seed)`; free it.

`toDgnWithReport(seed, options?)` takes the same options and returns `{bytes, modelIndex,
warnings}`: the index of the source model written and one warning `{code, message, offset,
object}` for each model left out (`dgn.model_not_written`, `object` is its index), for a
dimension change (`dgn.model_dimension_changed`) and for the texts written with an estimated
length (`dgn.text_length_estimated`, with their count).

`stripDgnSeed(seed, options?, readOptions?)` copies a DGN V8 seed without its model's drawing
content and without the document properties that name people or organisations. Options:
`keepControls`, `keepRasterTemplate` (both `false`), `keepCellDefinitions` (`true`),
`modelName` (1 to 249 UTF-16 units, no NUL), `timestampMs` (0 to 1e15), `application` (at
most 255 units, default `"cadkit"`); an unknown key throws `bad_argument`. `readOptions` sets
the read limits. The report (`DgnStripSeedReport`) counts what was removed and what remains;
names in the kept tables remain, so the result is not anonymous. See
[obtaining a seed](CADKIT_INTEGRATION.md#obtaining-a-seed).

`toCityGml(srsName, lod, options?)` writes generic city objects with the CRS name of the
existing coordinates (no reprojection, no axis swap) and `lod` 0 to 4. With `metadataOnly`
`false` (the default) an entity without CityGML geometry is an `unsupported` error; with `true`
it is kept as JSON metadata and listed in `report.issues` (`{code, model, path, entity_type}`).
`report` also counts `entities`, `geometry_entities` and `metadata_only_entities`. Options of
both (`CityGmlExportOptions`): `closedPolylinesAsSurfaces` (default `false`: write closed,
straight polylines with at least three distinct vertices as `gml:Polygon` surfaces when they
are planar and simple; the others stay line strings and are listed in `report.issues` with
`gml.face_not_planar`, `gml.face_degenerate`, `gml.face_self_intersecting` or
`gml.face_invalid`), `planarityTolerance` (largest distance of a ring point from the face
plane as a fraction of the face's bounding-box diagonal, default 1e-3; note that `toDgn`'s
option of that name is in drawing units), `maxPlanarityDeviation` (ceiling of that distance
in coordinate units, default 0.01, the default validation tolerance) and the limit keys.

`toCityGmlBuildings(options)` builds `bldg:Building` objects from the faces of one model:
polygons (holes kept), faces, mesh faces and, with `closedPolylinesAsSurfaces`, closed
straight polylines, including geometry drawn through blocks and groups (hidden layers are
skipped). It returns `{xml, report}`, `xml` as UTF-8 bytes that passed validation with
geometry checks. Faces that are degenerate, not planar within the tolerances or
self-intersecting are skipped and reported. A face is classified by its tilt, the angle
between its normal and the vertical: at least `wallMinTiltDegrees` (80) is a wall; at most
`horizontalMaxTiltDegrees` (10) is a ground surface when its lowest point lies within
`groundTolerance` (0.01 coordinate units) of the building's lowest point, a roof otherwise;
a sloped face above that band is a roof, one reaching the base stays unclassified
(`gml.face_sloped_at_base`). The rule knows geometry, not use: interior floors and overhang
undersides become roofs. No TKGM attribute, name, storey or class is invented.

| Option | |
|---|---|
| `srsName` | required: the CRS of the existing coordinates (no reprojection) |
| `lod` | required: 2, 3 or 4 (CityGML 2.0 has boundary surfaces from LoD 2) |
| `model` | source model, default 0 |
| `grouping` | `"single"` (default: one building), `"layer"` (per effective layer), `"top_level"` (per top-level entity: a cell, group or block reference with all it draws), `"connected"` (faces that share a vertex within `weldTolerance`, default 0.001) |
| `wallMinTiltDegrees`, `horizontalMaxTiltDegrees`, `groundTolerance`, `weldTolerance` | as above; `0 <= horizontal < wall <= 90` |
| `maxWork` | work budget of the walk, the face checks and the welding (default 100,000,000) |
| `closedPolylinesAsSurfaces`, `planarityTolerance`, `maxPlanarityDeviation`, limit keys | as for `toCityGml` |

The report: `{model, buildings: [{id, layer, topLevel, wallSurfaces, roofSurfaces,
groundSurfaces, unclassifiedFaces}], wallSurfaces, roofSurfaces, groundSurfaces,
unclassifiedFaces, skippedFaces, skippedByCode, ignoredEntities, closedPolylinesIgnored,
issues, issuesTruncated}`, each issue `{code, path, modelPath, meshFace, entityId, layer,
entityType, buildingId, polygonId}` with the [entity path](#conventions) of the source face
(`entity(report.model, issue.path)`). At most min(`maxObjects`, 100,000) issues are listed.
`toCityGmlBuildingsDocument(options)` returns `{document, report}` with the buildings as a
`CityGmlDocument`, before validation.

`CityGmlDocument` (from `readCityGml`): `toGml(options?)` (XML bytes), `toJsonString()`,
`CityGmlDocument.fromJsonString(text)`, `toDocument(lod?)` (a `CadDocument`),
`validate(options?)` (`{elements, ids, references, polygons, interiors, positions,
attributes}`), `geometryIssues(options?)` and `preflightTkgm(profile)` with
`"city-model-tender"` or `"digital-building-registration"`. Validation options: `geometry`
(`true`), `tolerance` (1e-6), `planarityTolerance` (0.01), `maxGeometryWork` and the limit
keys. Schemas are never fetched.

`preflightTkgm` is a local structural preflight of a documented subset of the TKGM rules
(required fields and types, geometry property presence, storey references, the 3D envelope);
it runs no official validation and never establishes acceptance: `acceptance_verified` is
always `false`, an empty `issues` list means only that these local checks found nothing, and
`unchecked_rules` names what it does not check. Its issues carry no source identifiers.
`preflightTkgmLocated(profile)` is the same with `id` and `building_id` (the `gml:id`s of the
element and its building) on each issue, and `geometryIssues(options?)` lists polygon and
solid-shell problems as `[{offset, code, message, element, id, objectId, buildingId}]`. Those
ids are chosen by whoever wrote the file and may encode client data (parcel or registry
numbers): keep such reports with the file's owner.

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
  `toSvgBounded`, `pick` and `entities` have output and work budgets, `surfaceMesh` and
  `lineBuffer` triangle, segment and work budgets (see [Renderer buffers](#renderer-buffers)).
- `toDgn` with `prepare` and `prepareForDgn` hold a copy of the written model during the call.
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
  `build.py` (2,700,000 bytes; 2,438,423 when it was set, 2,626,808 with the renderer buffers
  of cadkit-scene and without wasm-opt). CI runs it.
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
operations, `scene_test` the renderer buffers, `export_test` the SVG viewer options, CityGML
buildings and DGN writing with preparation, `typings_test` the declarations against the
runtime and the samples,
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
