// The JS contract: one options parser for every function, argument validation without traps,
// and error objects with documented properties.
import test from "node:test";
import assert from "node:assert/strict";
import { wasm, thrown, DXF, KINDS, encode, captureWarnings, blockDxf } from "./helpers.mjs";

const bytes = encode(DXF);
const GML = encode("<CityModel xmlns='http://www.opengis.net/citygml/2.0'/>");

function readOk(options) {
  const doc = wasm.read(bytes, options);
  try {
    return doc.info().models[0].entities;
  } finally {
    doc.free();
  }
}

test("read options: object and JSON text give the same limits, both spellings", () => {
  for (const options of [
    { maxObjects: 1 }, '{"maxObjects":1}', { max_objects: 1 }, '{"max_objects":1}',
    { limits: { maxObjects: 1 } }, '{"limits":{"max_objects":1}}', { preset: "strict", maxObjects: 1 },
  ]) {
    const e = thrown(() => wasm.read(bytes, options));
    assert.equal(e?.kind, "limit_exceeded", JSON.stringify(options));
    assert.equal(e.limit, "maxObjects");
    assert.equal(e.limitValue, 1);
  }
  for (const options of [undefined, null, "", "  ", {}, "{}", { maxObjects: 2 }, '{"maxObjects":2}', { maxObjects: undefined }]) {
    assert.equal(readOk(options), 2, JSON.stringify(options));
  }
});

test("read options: every limit is reachable", () => {
  const poly = encode([
    "0", "SECTION", "2", "ENTITIES",
    "0", "LWPOLYLINE", "8", "0", "90", "3", "10", "0", "20", "0", "10", "1", "20", "1", "10", "2", "20", "0",
    "0", "TEXT", "8", "0", "10", "0", "20", "0", "40", "1", "1", "some longer text",
    "0", "ENDSEC", "0", "EOF", "",
  ].join("\n"));
  for (const [key, value] of [["maxObjects", 1], ["maxStringBytes", 1], ["maxVertices", 1], ["maxInputBytes", 10]]) {
    for (const options of [{ [key]: value }, JSON.stringify({ [key]: value }), { limits: { [key]: value } }]) {
      const e = thrown(() => wasm.read(poly, options));
      assert.equal(e?.kind, "limit_exceeded", key);
      assert.equal(e.limit, key);
      assert.equal(e.limitValue, value);
    }
  }
  // Accepted with valid values (their effect needs inputs too large for a unit test).
  for (const key of ["maxDecompressedBytes", "maxDepth", "maxTotalVertices"]) {
    const d = wasm.read(poly, { [key]: 1000, preset: "default" });
    d.free();
  }
  const e = thrown(() => wasm.read(new Uint8Array(65 << 20), { preset: "strict" }));
  assert.equal(e?.kind, "limit_exceeded");
  assert.equal(e.limit, "maxInputBytes");
  assert.equal(e.limitValue, 64 << 20);
  assert.equal(e.needed, 65 << 20);
});

test("read options: a typo is warned about once per call and ignored", () => {
  const [count, warnings] = captureWarnings(() => readOk({ maxObject: 1, foo: true, limits: { maxObjectz: 1 } }));
  assert.equal(count, 2);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /^cadkit-wasm: read\(\): unknown option /);
  for (const key of ['"maxObject"', '"foo"', '"limits.maxObjectz"']) assert.ok(warnings[0].includes(key), key);
  assert.match(warnings[0], /next minor release/);
  const [, none] = captureWarnings(() => readOk({ maxObjects: 5 }));
  assert.deepEqual(none, []);
  const [, json] = captureWarnings(() => readOk('{"maxObject":1}'));
  assert.equal(json.length, 1);
  const [, svg] = captureWarnings(() => {
    const doc = wasm.read(bytes);
    try { doc.toSvg({ widthPX: 10 }); } finally { doc.free(); }
  });
  assert.match(svg[0], /toSvg\(\): unknown option "widthPX"/);
});

test("read options: wrong types, NaN, negative and fractional values name the option", () => {
  for (const [key, value] of [
    ["maxObjects", NaN], ["maxObjects", -1], ["maxObjects", 1.5], ["maxObjects", "10"], ["maxObjects", Infinity],
    ["maxObjects", true], ["maxObjects", 2 ** 53], ["maxInputBytes", -Infinity], ["maxDepth", 0.5],
    ["maxStringBytes", []], ["maxVertices", {}], ["maxTotalVertices", "1"], ["maxDecompressedBytes", NaN],
    ["keepRaw", "yes"], ["keepRaw", 1], ["codepage", 1252], ["preset", "lenient"], ["preset", 1], ["limits", 5],
  ]) {
    const e = thrown(() => wasm.read(bytes, { [key]: value }));
    assert.equal(e?.kind, "bad_argument", `${key}: ${String(value)}`);
    assert.match(e.message, new RegExp(key.replace(/[A-Z]/g, (c) => c)), e.message);
  }
  for (const options of [42, true, [1], "not json", "[1]", "42", () => 1, new Date(), new Map()]) {
    assert.equal(thrown(() => wasm.read(bytes, options))?.kind, "bad_argument", String(options));
  }
  const both = thrown(() => wasm.read(bytes, { maxObjects: 1, max_objects: 2 }));
  assert.equal(both?.kind, "bad_argument");
  // Shared arrays are cheap in JS but would be copied item by item: the copy is bounded.
  const row = Array(20_000).fill(0);
  const shared = Array(20_000).fill(row);
  assert.equal(thrown(() => wasm.read(bytes, { extra: shared }))?.kind, "bad_argument");
  const doc = wasm.read(bytes);
  try {
    assert.equal(thrown(() => doc.transform({ matrix: shared }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.pick(0, 0, 0, { tolerance: 1, excludedLayers: Array(20_000).fill("x".repeat(2048)) }))?.kind, "bad_argument");
  } finally {
    doc.free();
  }
});

test("read options: values above the wasm ceilings are lowered, not errors", () => {
  assert.equal(readOk({ maxInputBytes: 1e15, maxDecompressedBytes: 1e15, maxObjects: 1e15, maxDepth: 1e6,
    maxStringBytes: 1e12, maxVertices: 1e12, maxTotalVertices: 1e15 }), 2);
  assert.equal(readOk({ maxInputBytes: Number.MAX_SAFE_INTEGER }), 2);
  // The ceiling of maxInputBytes is 512 MiB whatever is asked for.
  const e = thrown(() => wasm.inspect(new Uint8Array((512 << 20) + 1), { maxInputBytes: 1e15 }));
  assert.equal(e?.kind, "limit_exceeded");
  assert.equal(e.limitValue, 512 << 20);
});

test("other option sets use the same parser", () => {
  const doc = wasm.read(bytes);
  try {
    for (const [name, call] of [
      ["toSvg", (o) => doc.toSvg(o)],
      ["toSvgBounded", (o) => doc.toSvgBounded({ maxOutputBytes: 1 << 20, ...o })],
      ["pickEntity", (o) => doc.pickEntity(0, 5, 2.5, 0.5, false, o)],
      ["pick", (o) => doc.pick(0, 5, 2.5, { tolerance: 0.5, ...o })],
      ["entities", (o) => doc.entities(0, o)],
      ["explodeInserts", (o) => doc.clone().explodeInserts(o)],
    ]) {
      const [, warnings] = captureWarnings(() => call({ bogusKey: 1 }));
      assert.equal(warnings.length, 1, name);
      assert.match(warnings[0], new RegExp(`${name}\\(\\): unknown option "bogusKey"`));
    }
    assert.equal(thrown(() => doc.toSvg({ widthPx: "800" }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.toSvg({ text: 1 }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.toSvg({ model: 0.5 }))?.kind, "bad_argument");
    assert.equal(doc.toSvg('{"widthPx": 800}'), doc.toSvg({ widthPx: 800 }));
    assert.equal(doc.toSvg({ background: null }), doc.toSvg({ background: "none" }));
    assert.equal(thrown(() => doc.toDgn(new Uint8Array(8), { clearSeedModel: "yes" }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.toDgn(new Uint8Array(8), { planarityTolerance: NaN }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.explodeInserts({ maxEntities: -1 }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.entities(0, { limit: 0 }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.entities(0, { kinds: ["lines"] }))?.kind, "bad_argument");
  } finally {
    doc.free();
  }
  const native = wasm.readCityGml(GML, { maxObjects: 100 });
  try {
    assert.equal(thrown(() => native.validate({ geometry: "no" }))?.kind, "bad_argument");
    assert.equal(native.validate('{"geometry": false}').elements, 1);
    assert.equal(thrown(() => native.toGml({ maxObjects: -5 }))?.kind, "bad_argument");
    const e = thrown(() => native.validate({ maxObjects: 0 }));
    assert.ok(e === undefined || e.kind === "limit_exceeded");
  } finally {
    native.free();
  }
  // stripDgnSeed keeps rejecting unknown keys (a misspelt option must not keep what it meant to remove).
  assert.equal(thrown(() => wasm.stripDgnSeed(new Uint8Array(8), { keepCellDefinition: false }))?.kind, "bad_argument");
});

// --- wrong argument types -----------------------------------------------------------------

const BAD = {
  bytes: [123, "abc", {}, [1, 2, 3], true, null, undefined, () => 0],
  options: [123, true, [1], "not json", "[1]", () => 1],
  requiredOptions: [123, true, [1], "not json", null, undefined],
  model: [NaN, -1, 1.5, "0", {}, null, undefined, 99, Infinity],
  number: [NaN, Infinity, -Infinity, "1", {}, null, undefined, []],
  string: [123, {}, [], true, null, undefined, 1n],
  optString: [123, {}, [], true],
  version: [123, {}, [], true, "r99"],
  path: [NaN, -1, 1.5, "0", {}, [], ["a"], [0, -1], null, undefined],
  optPath: [NaN, -1, 1.5, "0", {}, [], ["a"]],
  optBool: [1, "true", {}, []],
  optLod: [-1, 5, 1.5, "1", NaN, {}],
  lod: [-1, 5, 1.5, "1", NaN, null, undefined],
  strings: [123, "a", [1], {}, null, undefined, [null]],
  kinds: [123, "line", [1], ["lines"], null, undefined],
  spec: [123, true, null, undefined, [1, 2], { matrix: [1] }, { scale: "x" }, { translate: [NaN, 0] }, { scale: 0 },
    { rotateZ: "1" }, { matrix: Array(12).fill(0) }, "[1,2]", { scale: 2, matrix: Array(12).fill(1) }],
  profile: [123, {}, null, undefined, "nope"],
};

test("every export validates its arguments and throws bad_argument without trapping", () => {
  const doc = wasm.read(blockDxf());
  const native = wasm.readCityGml(GML);
  const seed = new Uint8Array(16);
  const pick = { tolerance: 1, maxOutputBytes: 4096 };
  // [name, call(args), valid args, kind of each argument]
  const cases = [
    ["detect", (a) => wasm.detect(...a), [bytes], ["bytes"]],
    ["inspect", (a) => wasm.inspect(...a), [bytes, {}], ["bytes", "options"]],
    ["read", (a) => wasm.read(...a), [bytes, {}], ["bytes", "options"]],
    ["readCityGml", (a) => wasm.readCityGml(...a), [GML, {}], ["bytes", "options"]],
    ["stripDgnSeed", (a) => wasm.stripDgnSeed(...a), [seed, {}, {}], ["bytes", "options", "options"]],
    ["CadDocument.fromJsonString", (a) => wasm.CadDocument.fromJsonString(...a), ["{}", {}], ["string", "options"]],
    ["CadDocument.fromCityGml", (a) => wasm.CadDocument.fromCityGml(...a), [GML, 1, {}], ["bytes", "optLod", "options"]],
    ["CityGmlDocument.fromJsonString", (a) => wasm.CityGmlDocument.fromJsonString(...a), ["{}"], ["string"]],
    ["bbox", (a) => doc.bbox(...a), [0, [1]], ["model", "optPath"]],
    ["block", (a) => doc.block(...a), ["B"], ["string"]],
    ["convertUnits", (a) => doc.convertUnits(...a), ["mm"], ["string"]],
    ["entities", (a) => doc.entities(...a), [0, {}], ["model", "options"]],
    ["entityCount", (a) => doc.entityCount(...a), [0], ["model"]],
    ["entity", (a) => doc.entity(...a), [0, 0], ["model", "path"]],
    ["explodeInserts", (a) => doc.clone().explodeInserts(...a), [{}], ["options"]],
    ["pick", (a) => doc.pick(...a), [0, 1, 0, pick], ["model", "number", "number", "requiredOptions"]],
    ["pickEntity", (a) => doc.pickEntity(...a), [0, 1, 0, 1, false, {}], ["model", "number", "number", "number", "optBool", "options"]],
    ["pickEntityBounded", (a) => doc.pickEntityBounded(...a), [0, 1, 0, 1, { maxOutputBytes: 4096 }, false],
      ["model", "number", "number", "number", "requiredOptions", "optBool"]],
    ["retainKinds", (a) => doc.clone().retainKinds(...a), [["line"]], ["kinds"]],
    ["retainLayers", (a) => doc.clone().retainLayers(...a), [["0"]], ["strings"]],
    ["stats", (a) => doc.stats(...a), [0], ["optPath"]],
    ["toCityGml", (a) => doc.toCityGml(...a), ["EPSG:4979", 1], ["string", "lod"]],
    ["toCityGmlReport", (a) => doc.toCityGmlReport(...a), ["EPSG:4979", 1, true], ["string", "lod", "optBool"]],
    ["toCityGmlWithReport", (a) => doc.toCityGmlWithReport(...a), ["EPSG:4979", 1, true], ["string", "lod", "optBool"]],
    ["toDgn", (a) => doc.toDgn(...a), [seed, {}], ["bytes", "options"]],
    ["toDxf", (a) => doc.toDxf(...a), ["r2000"], ["version"]],
    ["toDxfWithReport", (a) => doc.toDxfWithReport(...a), ["r2000"], ["version"]],
    ["toDxfBytes", (a) => doc.toDxfBytes(...a), ["r2000", "windows-1254"], ["version", "optString"]],
    ["toDxfBytesWithReport", (a) => doc.toDxfBytesWithReport(...a), ["r2000", "windows-1254"], ["version", "optString"]],
    ["toJsonString", (a) => doc.toJsonString(...a), [false], ["optBool"]],
    ["toSvg", (a) => doc.toSvg(...a), [{}], ["options"]],
    ["toSvgBounded", (a) => doc.toSvgBounded(...a), [{ maxOutputBytes: 1 << 20 }], ["requiredOptions"]],
    ["transform", (a) => doc.clone().transform(...a), [{ scale: 2 }], ["spec"]],
    ["viewports", (a) => doc.viewports(...a), [0], ["model"]],
    ["preflightTkgm", (a) => native.preflightTkgm(...a), ["city-model-tender"], ["profile"]],
    ["toDocument", (a) => native.toDocument(...a), [1], ["optLod"]],
    ["toGml", (a) => native.toGml(...a), [{}], ["options"]],
    ["validate", (a) => native.validate(...a), [{}], ["options"]],
  ];
  let checked = 0;
  try {
    for (const [name, call, valid, kinds] of cases) {
      kinds.forEach((kind, i) => {
        const values = kind === "path" || kind === "optPath" ? (name === "stats" ? BAD.optLod.concat(["0"]) : BAD[kind]) : BAD[kind];
        for (const value of values) {
          const args = valid.slice();
          args[i] = value;
          const e = thrown(() => call(args));
          assert.equal(e?.kind, "bad_argument", `${name} argument ${i} = ${typeof value === "string" ? JSON.stringify(value) : String(value)}: ${e?.kind} ${e?.message}`);
          assert.ok(e instanceof Error);
          checked++;
        }
      });
    }
  } finally {
    doc.free();
    native.free();
  }
  assert.ok(checked > 250, `${checked} cases`);
  // The module is still usable.
  assert.equal(wasm.detect(bytes), "dxf");
});

test("byte arguments accept Uint8Array, Buffer, ArrayBuffer, typed arrays and DataView", () => {
  const padded = new Uint8Array(bytes.length + 16);
  padded.set(bytes, 8);
  const inputs = [
    bytes, Buffer.from(bytes), bytes.buffer.slice(0), new DataView(padded.buffer, 8, bytes.length),
    padded.subarray(8, 8 + bytes.length), new Uint8Array(padded.buffer, 8, bytes.length),
  ];
  if (bytes.length % 2 === 0) inputs.push(new Uint16Array(bytes.slice().buffer));
  for (const input of inputs) {
    assert.equal(wasm.detect(input), "dxf", input.constructor.name);
    const doc = wasm.read(input);
    assert.equal(doc.info().models[0].entities, 2);
    doc.free();
  }
});

// --- errors ---------------------------------------------------------------------------------

test("errors carry kind, offset, limit and needed as properties", () => {
  const trunc = thrown(() => wasm.read(encode("AC1015" + "\0".repeat(20))));
  assert.equal(trunc?.kind, "truncated");
  assert.equal(typeof trunc.offset, "number");
  assert.equal(typeof trunc.needed, "number");
  const invalid = thrown(() => wasm.CadDocument.fromJsonString("{}"));
  assert.equal(invalid?.kind, "invalid");
  assert.equal(invalid.offset, 0);
  const limit = thrown(() => wasm.read(bytes, { maxObjects: 1 }));
  assert.deepEqual([limit.kind, limit.limit, limit.limitValue], ["limit_exceeded", "maxObjects", 1]);
  const input = thrown(() => wasm.read(bytes, { maxInputBytes: 16 }));
  assert.deepEqual([input.kind, input.limit, input.limitValue, input.needed], ["limit_exceeded", "maxInputBytes", 16, bytes.length]);
  const unknown = thrown(() => wasm.read(new Uint8Array(32)));
  assert.equal(unknown?.kind, "unknown_format");
  assert.equal(unknown.offset, undefined);
  const doc = wasm.read(bytes);
  try {
    const svg = thrown(() => doc.toSvgBounded({ maxOutputBytes: 10 }));
    assert.equal(svg.kind, "limit_exceeded");
    const work = thrown(() => doc.pick(0, 5, 2.5, { tolerance: 0.5, maxWork: 1 }));
    assert.deepEqual([work.kind, work.limit, work.limitValue], ["limit_exceeded", "maxWork", 1]);
    const out = thrown(() => doc.pickEntityBounded(0, 5, 2.5, 0.5, { maxOutputBytes: 8 }));
    assert.deepEqual([out.kind, out.limit, out.limitValue], ["limit_exceeded", "maxOutputBytes", 8]);
  } finally {
    doc.free();
  }
  // Argument errors are bad_argument, not "invalid at offset 0".
  const native = wasm.readCityGml(GML);
  try {
    const e = thrown(() => native.preflightTkgm("nope"));
    assert.equal(e.kind, "bad_argument");
    assert.equal(e.offset, undefined);
  } finally {
    native.free();
  }
  const d = wasm.read(bytes);
  try {
    assert.equal(thrown(() => d.toCityGml("", 1))?.kind, "bad_argument");
    assert.equal(thrown(() => d.toCityGml("EPSG:4979", 9))?.kind, "bad_argument");
    assert.equal(thrown(() => d.toCityGmlWithReport("", 9, false))?.kind, "bad_argument");
  } finally {
    d.free();
  }
});

test("messages do not repeat their prefix", () => {
  const seen = [];
  const collect = (fn) => {
    const e = thrown(fn);
    if (e) seen.push(e);
  };
  collect(() => wasm.read(new Uint8Array(32)));
  collect(() => wasm.read(bytes, { maxObjects: 1 }));
  collect(() => wasm.read(encode("AC1015" + "\0".repeat(20))));
  const doc = wasm.read(bytes);
  try {
    collect(() => doc.toCityGmlWithReport("urn:x", 2, false));
    collect(() => doc.convertUnits("parsec-ish"));
    collect(() => doc.toDxfBytes("r2000", "no-such-code-page"));
  } finally {
    doc.free();
  }
  assert.ok(seen.length >= 4);
  for (const e of seen) {
    assert.ok(KINDS.includes(e.kind), e.kind);
    assert.doesNotMatch(e.message, /^([^:]{1,40}): \1[: ]/i, e.message);
  }
});
