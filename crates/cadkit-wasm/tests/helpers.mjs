// Shared fixtures of the cadkit-wasm Node tests. Not a test file itself.
import { createRequire } from "node:module";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

export const here = path.dirname(fileURLToPath(import.meta.url));
export const crateDir = path.join(here, "..");
export const repoDir = path.join(crateDir, "..", "..");
const pkgDir = path.join(crateDir, "pkg-node");
if (!existsSync(path.join(pkgDir, "cadkit_wasm.js"))) {
  throw new Error("pkg-node/ missing: run `python crates/cadkit-wasm/build.py` first");
}
export const pkgNodeDir = pkgDir;
export const wasm = createRequire(import.meta.url)(path.join(pkgDir, "cadkit_wasm.js"));

/** The closed list of error kinds (docs/wasm.md, CadkitErrorKind). */
export const KINDS = ["bad_argument", "unknown_format", "unsupported", "invalid", "truncated", "limit_exceeded", "io"];
export const READER_KINDS = ["unknown_format", "unsupported", "invalid", "truncated", "limit_exceeded"];

/** Returns the thrown error, or undefined. A trap (WebAssembly.RuntimeError) is rethrown. */
export function thrown(fn) {
  try {
    fn();
  } catch (e) {
    if (e instanceof WebAssembly.RuntimeError) throw e;
    return e;
  }
  return undefined;
}

export const encode = (text) => new TextEncoder().encode(text);

export const DXF = [
  "0", "SECTION", "2", "HEADER", "9", "$ACADVER", "1", "AC1015", "9", "$INSUNITS", "70", "4", "0", "ENDSEC",
  "0", "SECTION", "2", "TABLES", "0", "TABLE", "2", "LAYER", "70", "1",
  "0", "LAYER", "2", "WALLS", "70", "0", "62", "1", "6", "CONTINUOUS", "0", "ENDTAB", "0", "ENDSEC",
  "0", "SECTION", "2", "ENTITIES",
  "0", "LINE", "8", "WALLS", "10", "0.0", "20", "0.0", "30", "0.0", "11", "10.0", "21", "5.0", "31", "0.0",
  "0", "CIRCLE", "8", "WALLS", "10", "5.0", "20", "5.0", "30", "0.0", "40", "2.0",
  "0", "ENDSEC", "0", "EOF", "",
].join("\n");

/** A DXF with one block (a line and a circle) inserted `inserts` times, and a free line. */
export function blockDxf({ columns = 1, rows = 1, inserts = 1 } = {}) {
  const ins = [];
  for (let i = 0; i < inserts; i++) {
    ins.push("0", "INSERT", "8", "PARTS", "2", "B", "10", String(i * 20), "20", "0", "30", "0",
      "70", String(columns), "71", String(rows), "44", "5", "45", "5");
  }
  return encode([
    "0", "SECTION", "2", "BLOCKS",
    "0", "BLOCK", "8", "0", "2", "B", "70", "0", "10", "0", "20", "0", "30", "0",
    "0", "LINE", "8", "0", "10", "0", "20", "0", "30", "0", "11", "4", "21", "0", "31", "0",
    "0", "CIRCLE", "8", "0", "10", "2", "20", "2", "30", "0", "40", "1",
    "0", "ENDBLK", "0", "ENDSEC",
    "0", "SECTION", "2", "ENTITIES",
    "0", "LINE", "8", "FREE", "10", "0", "20", "-10", "30", "0", "11", "10", "21", "-10", "31", "0",
    ...ins,
    "0", "ENDSEC", "0", "EOF", "",
  ].join("\n"));
}

/** A DXF in map coordinates (around x 500,000, y 4,400,000): two lines and a closed polyline. */
export function geoDxf({ x = 500_000.125, y = 4_400_000.25 } = {}) {
  const p = (dx, dy) => [String(x + dx), String(y + dy)];
  const [ax, ay] = p(0, 0), [bx, by] = p(12.5, 3.75), [cx, cy] = p(-4.5, 8.25);
  return encode([
    "0", "SECTION", "2", "ENTITIES",
    "0", "LINE", "8", "ROADS", "10", ax, "20", ay, "30", "0", "11", bx, "21", by, "31", "0",
    "0", "LINE", "8", "ROADS", "10", bx, "20", by, "30", "0", "11", cx, "21", cy, "31", "0",
    "0", "LWPOLYLINE", "8", "PLOTS", "90", "4", "70", "1",
    ...[[1, 1], [6, 1], [6, 5.5], [1, 5.5]].flatMap(([dx, dy]) => {
      const [vx, vy] = p(dx, dy);
      return ["10", vx, "20", vy];
    }),
    "0", "ENDSEC", "0", "EOF", "",
  ].join("\n"));
}

/** Document JSON of one 3D model whose entities are closed polylines through `rings` (arrays of [x, y, z]). */
export function polylineDocument(rings, { layer = "0" } = {}) {
  const vertex = ([x, y, z]) => ({ position: { x, y, z }, bulge: 0, start_width: 0, end_width: 0 });
  return JSON.stringify({
    schema_version: 1,
    source: { format: "dxf", version: "", application: null, codepage: null },
    units: { unit: "meter", meters_per_unit: 1 },
    layers: [], linetypes: [], text_styles: [], blocks: [],
    models: [{
      name: "Model", id: null, kind: "model", is_3d: true, props: {},
      entities: rings.map((ring) => ({
        id: null, layer, color: { type: "by_layer" }, linetype: null, lineweight: { type: "by_layer" },
        visible: true, attributes: [], props: {},
        kind: { type: "polyline", vertices: ring.map(vertex), closed: true, normal: { x: 0, y: 0, z: 1 } },
      })),
    }],
    props: {},
    warnings: [],
  });
}

/** The six faces of a box `[x0, x0 + w] x [y0, y0 + d] x [z0, z0 + h]` as rings. */
export function boxRings([x0, y0, z0] = [0, 0, 0], [w, d, h] = [10, 8, 6]) {
  const [x1, y1, z1] = [x0 + w, y0 + d, z0 + h];
  return [
    [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
    [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
    [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
    [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
    [[x1, y1, z0], [x0, y1, z0], [x0, y1, z1], [x1, y1, z1]],
    [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
  ];
}

/** A DGN V8 seed for the writer tests: the public GDAL sample stripped of its drawing, or undefined. */
export function dgnSeed() {
  for (const file of [
    path.join(samplesDir, "third-party", "gdal", "test_dgnv8.dgn"),
    corpusDir() && path.join(corpusDir(), "gdal", "test_dgnv8.dgn"),
  ]) {
    if (file && existsSync(file)) return wasm.stripDgnSeed(readFileSync(file)).seed;
  }
  return undefined;
}

/** Directory of the site's demo samples (third-party ones live in third-party/, may be missing). */
export const samplesDir = path.join(repoDir, "site", "public", "samples");

/** corpus/public of this checkout, or CADKIT_CORPUS; undefined when neither exists. */
export function corpusDir() {
  for (const dir of [process.env.CADKIT_CORPUS, path.join(repoDir, "corpus", "public")]) {
    if (dir && existsSync(dir)) return dir;
  }
  return undefined;
}

export function* walkFiles(dir) {
  if (!dir || !existsSync(dir)) return;
  for (const n of readdirSync(dir).sort()) {
    const p = path.join(dir, n);
    if (statSync(p).isDirectory()) yield* walkFiles(p);
    else yield p;
  }
}

/** Drawing samples available here: the site samples (own and third-party) and the public corpus. */
export function sampleFiles({ maxBytes = 4 << 20 } = {}) {
  const out = [];
  for (const dir of [samplesDir, corpusDir()]) {
    for (const f of walkFiles(dir)) {
      if (/\.(dwg|dgn|dxf|gml)$/i.test(f) && statSync(f).size <= maxBytes) out.push(f);
    }
  }
  return out;
}

export const readBytes = (file) => new Uint8Array(readFileSync(file));

/** Calls fn with console.warn captured; returns [result, warnings]. */
export function captureWarnings(fn) {
  const warnings = [];
  const original = console.warn;
  console.warn = (...args) => warnings.push(args.join(" "));
  try {
    return [fn(), warnings];
  } finally {
    console.warn = original;
  }
}
