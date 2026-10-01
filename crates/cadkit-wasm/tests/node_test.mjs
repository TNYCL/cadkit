// Run: node --test crates/cadkit-wasm/tests/node_test.mjs   (after `python build.py`)
import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const pkgDir = path.join(here, "..", "pkg-node");
if (!existsSync(path.join(pkgDir, "cadkit_wasm.js"))) {
  throw new Error("pkg-node/ missing: run `python crates/cadkit-wasm/build.py` first");
}
const wasm = createRequire(import.meta.url)(path.join(pkgDir, "cadkit_wasm.js"));
const KINDS = ["unknown_format", "unsupported", "invalid", "truncated", "limit_exceeded"];

const DXF = [
  "0", "SECTION", "2", "HEADER", "9", "$ACADVER", "1", "AC1015", "9", "$INSUNITS", "70", "4", "0", "ENDSEC",
  "0", "SECTION", "2", "TABLES", "0", "TABLE", "2", "LAYER", "70", "1",
  "0", "LAYER", "2", "WALLS", "70", "0", "62", "1", "6", "CONTINUOUS", "0", "ENDTAB", "0", "ENDSEC",
  "0", "SECTION", "2", "ENTITIES",
  "0", "LINE", "8", "WALLS", "10", "0.0", "20", "0.0", "30", "0.0", "11", "10.0", "21", "5.0", "31", "0.0",
  "0", "CIRCLE", "8", "WALLS", "10", "5.0", "20", "5.0", "30", "0.0", "40", "2.0",
  "0", "ENDSEC", "0", "EOF", "",
].join("\n");

/** Returns the thrown error, or undefined. */
function thrown(fn) {
  try { fn(); } catch (e) { return e; }
  return undefined;
}

test("detect returns undefined for garbage", () => {
  assert.equal(wasm.detect(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8])), undefined);
  assert.equal(wasm.detect(new Uint8Array(0)), undefined);
});

test("read garbage throws Error with kind", () => {
  const e = thrown(() => wasm.read(new Uint8Array([9, 9, 9, 9, 9, 9, 9, 9])));
  assert.ok(e instanceof Error, "is an Error");
  assert.equal(e.kind, "unknown_format");
  const e2 = thrown(() => wasm.read(new Uint8Array(0)));
  assert.ok(KINDS.includes(e2?.kind));
});

test("read with bad option types does not crash", () => {
  const e = thrown(() => wasm.read(new Uint8Array([1, 2, 3]), { keepRaw: true, codepage: "windows-1254" }));
  assert.ok(KINDS.includes(e?.kind));
});

test("synthetic DXF round trip", (t) => {
  const bytes = new TextEncoder().encode(DXF);
  const fmt = wasm.detect(bytes);
  if (fmt === undefined) return t.skip("DXF reader not available yet (detect)");
  assert.equal(fmt, "dxf");
  let doc;
  try {
    doc = wasm.read(bytes);
  } catch (e) {
    if (e.kind === "unsupported") return t.skip("DXF reader unsupported");
    throw e;
  }
  try {
    const info = doc.info();
    assert.equal(info.format, "dxf");
    assert.ok(Array.isArray(info.models));
    assert.equal(typeof info.layerCount, "number");
    assert.ok(Array.isArray(doc.layers()));
    const obj = doc.toJSON();
    assert.equal(typeof obj, "object");
    assert.equal(JSON.parse(doc.toJsonString(false)).source.format, "dxf");
    const svg = doc.toSvg({ widthPx: 800, strokePx: 1.5, background: "#fff" });
    assertSvg(svg);
    assertSvg(doc.toSvg());
    assert.equal(thrown(() => doc.toSvg({ model: 999 }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.toDxf("nope"))?.kind, "bad_argument");
    const dxf = thrown(() => doc.toDxf("r2000"));
    if (dxf !== undefined) assert.ok(KINDS.includes(dxf.kind));
  } finally {
    doc.free();
  }
});

/** Cheap well-formedness check for the SVG string. */
function assertSvg(svg) {
  assert.match(svg, /^\s*<svg[\s>]/);
  assert.match(svg, /<\/svg>\s*$/);
  assert.match(svg, /viewBox="[^"]+"/);
  const stack = [];
  for (const m of svg.matchAll(/<(\/?)([A-Za-z][\w:-]*)((?:"[^"]*"|'[^']*'|[^>"'])*?)(\/?)>/g)) {
    const [, close, name, , self] = m;
    if (self) continue;
    if (close) assert.equal(stack.pop(), name, `unbalanced </${name}>`);
    else stack.push(name);
  }
  assert.deepEqual(stack, [], "unclosed elements");
}

function* walk(dir) {
  for (const n of readdirSync(dir)) {
    const p = path.join(dir, n);
    if (statSync(p).isDirectory()) yield* walk(p);
    else yield p;
  }
}

const corpus = path.join(here, "..", "..", "..", "corpus", "public");
if (existsSync(corpus)) {
  for (const file of walk(corpus)) {
    if (!/\.(dwg|dgn|dxf)$/i.test(file)) continue;
    test(`corpus ${path.relative(corpus, file)}`, (t) => {
      const bytes = new Uint8Array(readFileSync(file));
      const fmt = wasm.detect(bytes);
      assert.ok(fmt === undefined || ["dwg", "dxf", "dgn_v7", "dgn_v8"].includes(fmt));
      let doc;
      try {
        doc = wasm.read(bytes);
      } catch (e) {
        assert.ok(e instanceof Error);
        assert.ok(KINDS.includes(e.kind), `kind ${e.kind}`);
        return t.skip(`reader returned ${e.kind}`);
      }
      try {
        const info = doc.info();
        assert.ok(info.models.length >= 0);
        if (info.models.length > 0) assertSvg(doc.toSvg({ model: 0 }));
      } finally {
        doc.free();
      }
    });
  }
}


test("CityGML native and new geometry exports", () => {
  const bytes = new TextEncoder().encode("<CityModel xmlns='http://www.opengis.net/citygml/2.0'/>");
  const native = wasm.readCityGml(bytes);
  try {
    assert.equal(native.validate().elements, 1);
    const profile = native.preflightTkgm("city-model-tender");
    assert.equal(profile.workflow, "city_model_tender_architectural");
    assert.equal(profile.acceptance_verified, false);
    assert.ok(profile.issues.some(i => i.code === "tkgm.missing_building"));
    assert.equal(thrown(() => native.preflightTkgm("unknown"))?.kind, "invalid");
    assert.equal(wasm.detect(native.toGml()), "city_gml");
    const copy = wasm.CityGmlDocument.fromJsonString(native.toJsonString());
    try {
      assert.deepEqual(copy.toGml(), native.toGml());
    } finally {
      copy.free();
    }
  } finally {
    native.free();
  }
  const original = wasm.read(new TextEncoder().encode(DXF));
  const object = original.toJSON();
  original.free();
  object.models[0].entities = object.models[0].entities.filter(e => e.kind.type === "line");
  assert.equal(object.models[0].entities.length, 1);
  const doc = wasm.CadDocument.fromJsonString(JSON.stringify(object));
  try {
    const gml = doc.toCityGml("urn:ogc:def:crs:EPSG::4979", 1);
    assert.match(new TextDecoder().decode(gml), /GenericCityObject/);
    const checked = wasm.readCityGml(gml);
    try {
      assert.equal(checked.validate().ids, 1);
    } finally {
      checked.free();
    }
  } finally {
    doc.free();
  }
});

test("CityGML export diagnostics require explicit metadata handling", () => {
  const original = wasm.read(new TextEncoder().encode(DXF));
  const source = original.toJSON();
  original.free();
  source.models[0].entities = source.models[0].entities.slice(0, 1);
  source.models[0].entities[0].kind = { type: "unknown", type_name: "synthetic" };
  const doc = wasm.CadDocument.fromJsonString(JSON.stringify(source));
  try {
    assert.equal(thrown(() => doc.toCityGmlWithReport("urn:cadkit:synthetic", 2, false))?.kind, "unsupported");
    const result = JSON.parse(doc.toCityGmlWithReport("urn:cadkit:synthetic", 2, true));
    assert.equal(result.report.metadata_only_entities, 1);
    assert.deepEqual(result.report.issues[0].path, [0]);
    assert.match(result.xml, /cadkit.geometry_status/);
    const native = wasm.readCityGml(new TextEncoder().encode(result.xml));
    try {
      assert.equal(native.validate().ids, 1);
    } finally {
      native.free();
    }
  } finally {
    doc.free();
  }
});

test("DGN export from neutral JSON", (t) => {
  const seedPath = path.join(here, "../../../corpus/public/gdal/test_dgnv8.dgn");
  if (!existsSync(seedPath)) return t.skip("public seed missing");
  const seed = readFileSync(seedPath);
  const seedDoc = wasm.read(seed);
  const original = wasm.read(new TextEncoder().encode(DXF));
  const source = original.toJSON();
  original.free();
  source.units = seedDoc.toJSON().units;
  source.models[0].is_3d = seedDoc.toJSON().models[0].is_3d;
  seedDoc.free();
  source.models[0].entities = source.models[0].entities.filter(e => e.kind.type === "line");
  const doc = wasm.CadDocument.fromJsonString(JSON.stringify(source));
  try {
    const output = doc.toDgn(seed, JSON.stringify({ clear_seed_model: true }));
    const readback = wasm.read(output);
    try {
      assert.equal(readback.toJSON().models[0].entities.length, 1);
    } finally {
      readback.free();
    }
  } finally {
    doc.free();
  }
});
