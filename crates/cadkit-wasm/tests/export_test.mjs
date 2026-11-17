// Writer options added for viewers and exchange: SVG origin and entity references, CityGML
// buildings from surfaces, and DGN V8 writing with a report and seed preparation.
import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import {
  wasm, thrown, encode, blockDxf, geoDxf, polylineDocument, boxRings, dgnSeed, samplesDir, readBytes,
  corpusDir, walkFiles,
} from "./helpers.mjs";

/** The `d` attributes of an SVG as lists of numbers (paths of lines and polylines only). */
function pathNumbers(svg) {
  return [...svg.matchAll(/ d="([^"]+)"/g)].map((m) => {
    assert.match(m[1], /^[MLZ0-9.\-e ]+$/, "only straight paths in this fixture");
    return [...m[1].matchAll(/-?\d+(?:\.\d+)?(?:e-?\d+)?/g)].map((n) => Number(n[0]));
  });
}

const viewBox = (svg) => svg.match(/viewBox="([^"]+)"/)[1].split(" ").map(Number);

test("SVG origin: a georeferenced drawing keeps its geometry relative to data-origin", () => {
  const doc = wasm.read(geoDxf());
  try {
    const plain = doc.toSvg();
    assert.doesNotMatch(plain, /data-origin/);
    assert.equal(doc.toSvg({ origin: "none" }), plain);
    assert.equal(doc.toSvg({ origin: null }), plain);
    for (const origin of ["auto", [500_000, 4_400_000], { x: 499_990.5, y: 4_400_010.25 }]) {
      const shifted = doc.toSvg({ origin });
      const [ox, oy] = shifted.match(/data-origin="([^"]+)"/)[1].split(" ").map(Number);
      if (Array.isArray(origin)) assert.deepEqual([ox, oy], origin);
      if (origin.x !== undefined) assert.deepEqual([ox, oy], [origin.x, origin.y]);
      const a = pathNumbers(plain);
      const b = pathNumbers(shifted);
      assert.equal(b.length, a.length);
      assert.ok(a.length >= 3);
      a.forEach((numbers, i) => {
        assert.equal(b[i].length, numbers.length);
        for (let k = 0; k < numbers.length; k += 2) {
          // An SVG point (sx, sy) is the drawing point (ox + sx, oy - sy); unshifted, sy = -y.
          assert.ok(Math.abs(ox + b[i][k] - numbers[k]) < 1e-6, `x ${b[i][k]} ${numbers[k]}`);
          assert.ok(Math.abs(oy - b[i][k + 1] + numbers[k + 1]) < 1e-6, `y ${b[i][k + 1]} ${numbers[k + 1]}`);
        }
      });
      const [vx, vy, vw, vh] = viewBox(shifted);
      const [px, py, pw, ph] = viewBox(plain);
      assert.ok(Math.abs(ox + vx - px) < 1e-6 && Math.abs(vy - oy - py) < 1e-6);
      assert.deepEqual([vw, vh], [pw, ph]);
      if (origin === "auto") assert.ok(Math.abs(vx) < 100 && Math.abs(vy) < 100, "auto keeps the numbers small");
    }
    // toSvgBounded takes the same options and gives the same text.
    const auto = doc.toSvg({ origin: "auto" });
    assert.equal(doc.toSvgBounded({ origin: "auto", maxOutputBytes: Buffer.byteLength(auto) }), auto);
  } finally {
    doc.free();
  }
});

test("SVG entity references resolve with entity(model, path)", () => {
  const doc = wasm.read(blockDxf({ inserts: 2 }));
  try {
    const svg = doc.toSvg({ entityRefs: true });
    const refs = [...svg.matchAll(/data-e="([\d.]+)"/g)].map((m) => m[1]);
    assert.ok(refs.length >= 5, refs.join(" "));
    for (const ref of new Set(refs)) {
      const p = ref.split(".").map(Number);
      const entity = doc.entity(0, p);
      assert.ok(entity, ref);
    }
    // Block geometry carries the path of the insert that draws it.
    assert.deepEqual([...new Set(refs)].sort(), ["0", "1", "2"]);
    assert.equal(doc.entity(0, [1]).kind.type, "insert");
    assert.doesNotMatch(doc.toSvg(), /data-e=/);
  } finally {
    doc.free();
  }
  // A colour replaced for contrast keeps the original as data-c.
  const white = wasm.read(encode([
    "0", "SECTION", "2", "ENTITIES",
    "0", "LINE", "8", "0", "62", "255", "10", "0", "20", "0", "11", "1", "21", "1",
    "0", "POINT", "8", "0", "10", "3", "20", "3",
    "0", "ENDSEC", "0", "EOF", "",
  ].join("\n")));
  try {
    const svg = white.toSvg({ entityRefs: true, pointPx: 4 });
    assert.match(svg, /data-e="0" data-c="#ffffff"/);
    assert.match(svg, /data-e="1" d="M3 -3h0"[^>]*stroke-width="4"/, "points as dots of 4 px");
  } finally {
    white.free();
  }
});

test("SVG text and hatch options are passed through", () => {
  const doc = wasm.read(readBytes(path.join(samplesDir, "cadkit-sample-plan.dxf")));
  try {
    const plain = doc.toSvg();
    const styled = doc.toSvg({ fontFamily: "Inter, sans-serif", capHeightRatio: 0.7 });
    assert.match(styled, /^<svg [^>]*font-family="Inter, sans-serif"/);
    assert.notEqual(styled, plain);
    assert.equal(doc.toSvg({ fontFamily: "" }), plain);
    assert.equal(doc.toSvg({ fontFamily: null, capHeightRatio: null, pointPx: null }), plain);
    for (const key of ["hatchNativeSpacing", "layoutViewports"]) {
      assert.match(doc.toSvg({ [key]: true }), /<\/svg>\s*$/, key);
    }
    // A layout with viewports draws the model through them.
    const layout = doc.info().models.findIndex((m) => m.index > 0);
    if (layout > 0) assert.match(doc.toSvg({ model: layout, layoutViewports: true }), /<\/svg>\s*$/);
  } finally {
    doc.free();
  }
});

test("SVG option values are validated", () => {
  const doc = wasm.read(geoDxf());
  try {
    for (const options of [
      { origin: "center" }, { origin: [1] }, { origin: [1, NaN] }, { origin: [1, "2"] }, { origin: { x: 1 } },
      { origin: { x: 1, y: 2, z: 3 } }, { origin: 5 }, { origin: true }, { entityRefs: "yes" },
      { pointPx: 0 }, { pointPx: -1 }, { pointPx: 1001 }, { pointPx: "3" }, { capHeightRatio: 0.01 },
      { capHeightRatio: 21 }, { capHeightRatio: NaN }, { fontFamily: 5 }, { fontFamily: "x".repeat(1025) },
      { hatchNativeSpacing: 1 }, { layoutViewports: "true" },
    ]) {
      assert.equal(thrown(() => doc.toSvg(options))?.kind, "bad_argument", JSON.stringify(options));
      assert.equal(thrown(() => doc.toSvgBounded({ maxOutputBytes: 1 << 20, ...options }))?.kind, "bad_argument");
    }
    assert.equal(doc.toSvg('{"origin": [500000, 4400000], "entity_refs": true}'),
      doc.toSvg({ origin: [500000, 4400000], entityRefs: true }));
  } finally {
    doc.free();
  }
});

// --- CityGML ----------------------------------------------------------------------------

const SRS = "urn:ogc:def:crs:EPSG::5254";

test("a box of closed polylines becomes one building with 4 walls, a roof and a ground", () => {
  const doc = wasm.CadDocument.fromJsonString(polylineDocument(boxRings([500_000, 4_400_000, 100])));
  try {
    const { xml, report } = doc.toCityGmlBuildings({ srsName: SRS, lod: 2, closedPolylinesAsSurfaces: true });
    assert.ok(xml instanceof Uint8Array);
    assert.equal(wasm.detect(xml), "city_gml");
    assert.equal(report.buildings.length, 1);
    assert.deepEqual(
      [report.wallSurfaces, report.roofSurfaces, report.groundSurfaces, report.unclassifiedFaces, report.skippedFaces],
      [4, 1, 1, 0, 0],
    );
    assert.deepEqual(report.buildings[0], {
      id: report.buildings[0].id, layer: null, topLevel: null,
      wallSurfaces: 4, roofSurfaces: 1, groundSurfaces: 1, unclassifiedFaces: 0,
    });
    const native = wasm.readCityGml(xml);
    try {
      const checked = native.validate({ geometry: true });
      assert.equal(checked.polygons, 6);
      assert.deepEqual(native.geometryIssues(), []);
      // Local checks only: the TKGM preflight lists what a registry would also need.
      const located = native.preflightTkgmLocated("digital-building-registration");
      assert.equal(located.acceptance_verified, false);
      assert.ok(located.issues.some((i) => i.building_id === report.buildings[0].id));
      const anonymous = native.preflightTkgm("digital-building-registration");
      assert.equal(anonymous.issues.length, located.issues.length);
      assert.ok(anonymous.issues.every((i) => !("id" in i) && !("building_id" in i)));
    } finally {
      native.free();
    }
    // The same buildings as a native document, not yet validated.
    const { document, report: same } = doc.toCityGmlBuildingsDocument({ srsName: SRS, lod: 2, closedPolylinesAsSurfaces: true });
    try {
      assert.ok(document instanceof wasm.CityGmlDocument);
      assert.deepEqual(same, report);
      assert.equal(document.validate({ geometry: true }).polygons, 6);
      assert.deepEqual(document.toGml(), xml);
    } finally {
      document.free();
    }
    // Without the option closed polylines are not surfaces, and the report says so.
    const off = doc.toCityGmlBuildings({ srsName: SRS, lod: 2 });
    assert.deepEqual([off.report.buildings.length, off.report.closedPolylinesIgnored], [0, 6]);
    // One building per top-level entity, with a tilted roof threshold.
    const split = doc.toCityGmlBuildings({ srsName: SRS, lod: 3, closedPolylinesAsSurfaces: true, grouping: "top_level", wallMinTiltDegrees: 85 });
    assert.equal(split.report.buildings.length, 6);
    assert.deepEqual(split.report.buildings.map((b) => b.topLevel), [0, 1, 2, 3, 4, 5]);
  } finally {
    doc.free();
  }
});

test("building issues carry entity paths that resolve", () => {
  const rings = boxRings();
  // A wall that is not planar and a degenerate face.
  rings.push([[0, 0, 20], [10, 0, 20], [10, 10, 25], [0, 10, 20.5]]);
  rings.push([[0, 0, 30], [1, 0, 30], [2, 0, 30]]);
  const doc = wasm.CadDocument.fromJsonString(polylineDocument(rings));
  try {
    const { xml, report } = doc.toCityGmlBuildings({ srsName: SRS, lod: 2, closedPolylinesAsSurfaces: true, grouping: "connected" });
    assert.equal(report.skippedFaces, 2);
    assert.deepEqual(Object.keys(report.skippedByCode).sort(), ["gml.face_degenerate", "gml.face_not_planar"]);
    for (const issue of report.issues) {
      assert.equal(doc.entity(report.model, issue.path).kind.type, issue.entityType);
      assert.deepEqual(issue.modelPath, issue.path);
      assert.equal(issue.layer, "0");
    }
    assert.deepEqual(report.issues.map((i) => i.path).sort(), [[6], [7]]);
    const native = wasm.readCityGml(xml);
    native.free();
  } finally {
    doc.free();
  }
});

test("closedPolylinesAsSurfaces on the generic CityGML exports", () => {
  const doc = wasm.CadDocument.fromJsonString(polylineDocument(boxRings()));
  try {
    const lines = new TextDecoder().decode(doc.toCityGml(SRS, 2));
    assert.doesNotMatch(lines, /gml:Polygon/);
    const surfaces = new TextDecoder().decode(doc.toCityGml(SRS, 2, { closedPolylinesAsSurfaces: true }));
    assert.equal(surfaces.match(/<gml:Polygon/g)?.length, 6);
    const { xml, report } = doc.toCityGmlReport(SRS, 2, false, { closed_polylines_as_surfaces: true, planarityTolerance: 1e-4 });
    assert.equal(report.geometry_entities, 6);
    assert.deepEqual(report.issues, []);
    const native = wasm.readCityGml(encode(xml));
    try {
      assert.equal(native.validate({ geometry: true }).polygons, 6);
    } finally {
      native.free();
    }
  } finally {
    doc.free();
  }
});

test("building options are validated", () => {
  const doc = wasm.CadDocument.fromJsonString(polylineDocument(boxRings()));
  try {
    const ok = { srsName: SRS, lod: 2 };
    for (const options of [
      {}, { lod: 2 }, { srsName: SRS }, { srsName: "", lod: 2 }, { srsName: SRS, lod: 1 }, { srsName: SRS, lod: 5 },
      { ...ok, model: 3 }, { ...ok, grouping: "rows" }, { ...ok, wallMinTiltDegrees: 91 },
      { ...ok, horizontalMaxTiltDegrees: 80 }, { ...ok, wallMinTiltDegrees: 5 }, { ...ok, groundTolerance: -1 },
      { ...ok, weldTolerance: NaN }, { ...ok, maxWork: 0 }, { ...ok, planarityTolerance: 0 },
      { ...ok, maxPlanarityDeviation: -1 }, { ...ok, closedPolylinesAsSurfaces: "yes" }, { ...ok, bogus: 1 },
    ]) {
      assert.equal(thrown(() => doc.toCityGmlBuildings(options))?.kind, "bad_argument", JSON.stringify(options));
      assert.equal(thrown(() => doc.toCityGmlBuildingsDocument(options))?.kind, "bad_argument", JSON.stringify(options));
    }
    assert.equal(doc.toCityGmlBuildings(JSON.stringify({ srs_name: SRS, lod: 2 })).report.model, 0);
    const e = thrown(() => doc.toCityGmlBuildings({ ...ok, closedPolylinesAsSurfaces: true, maxObjects: 2 }));
    assert.deepEqual([e?.kind, e?.limit], ["limit_exceeded", "maxObjects"]);
  } finally {
    doc.free();
  }
});

// --- DGN V8 -----------------------------------------------------------------------------

test("DGN writing with a report: model selection, warnings, options", (t) => {
  const seed = dgnSeed();
  if (!seed) return t.skip("the public DGN V8 sample is missing");
  const doc = wasm.read(readBytes(path.join(samplesDir, "cadkit-sample-plan.dxf")));
  try {
    assert.ok(doc.info().models.length > 1, "a model and a paper layout");
    // The DXF does not fit the seed as it is (units, symbology, blocks) ...
    assert.ok(thrown(() => doc.toDgn(seed)));
    // ... but a prepared copy does.
    const result = doc.toDgnWithReport(seed, { prepare: true, timestampMs: 1.7e12 });
    assert.ok(result.bytes instanceof Uint8Array);
    assert.equal(result.modelIndex, 0);
    assert.deepEqual(result.warnings.map((w) => [w.code, w.object]), [["dgn.model_not_written", 1]]);
    assert.deepEqual(doc.toDgn(seed, { prepare: true, timestampMs: 1.7e12 }), result.bytes);
    assert.deepEqual(doc.toDgn(seed, JSON.stringify({ prepare: { explode_inserts: true }, timestamp_ms: 1.7e12 })), result.bytes);
    // prepareForDgn returns the copy the writer writes.
    const prepared = doc.prepareForDgn(seed);
    const back = wasm.read(result.bytes);
    try {
      assert.equal(prepared.info().models.length, 1);
      assert.equal(back.entityCount(0), prepared.entityCount(0));
      assert.ok(back.entityCount(0) > 0);
      assert.deepEqual(prepared.toDgn(seed, { timestampMs: 1.7e12 }), result.bytes);
      const direct = prepared.toDgnWithReport(seed, { modelIndex: 0, dimensionPolicy: "strict", estimateTextLength: true });
      assert.equal(direct.modelIndex, 0);
      assert.ok(direct.warnings.every((w) => w.code !== "dgn.model_not_written"));
    } finally {
      prepared.free();
      back.free();
    }
    // Option errors.
    for (const options of [
      { modelIndex: 9 }, { modelIndex: -1 }, { dimensionPolicy: "loose" }, { estimateTextLength: 1 },
      { prepare: "yes" }, { prepare: 1 }, { prepare: { dimensionPolicy: "flat" } },
    ]) {
      assert.equal(thrown(() => doc.toDgn(seed, options))?.kind, "bad_argument", JSON.stringify(options));
      assert.equal(thrown(() => doc.toDgnWithReport(seed, options))?.kind, "bad_argument", JSON.stringify(options));
    }
    // New methods reject unknown keys, also inside prepare; toDgn warns as before.
    assert.equal(thrown(() => doc.toDgnWithReport(seed, { prepare: { bogus: true } }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.prepareForDgn(seed, { bogus: true }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.prepareForDgn(seed, { modelIndex: 5 }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.toDgnWithReport(seed, { prepare: true, maxInputBytes: 16 }))?.limit, "maxInputBytes");
  } finally {
    doc.free();
  }
});

test("every public DXF and DWG sample writes into the stripped seed once prepared", (t) => {
  const seed = dgnSeed();
  if (!seed) return t.skip("the public DGN V8 sample is missing");
  const files = [path.join(samplesDir, "cadkit-sample-plan.dxf"), path.join(samplesDir, "cadkit-sample-site.dxf")];
  for (const f of walkFiles(corpusDir())) if (/\.(dxf|dwg)$/i.test(f)) files.push(f);
  let written = 0;
  for (const file of files) {
    let doc;
    try {
      doc = wasm.read(readBytes(file));
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) throw e;
      continue;
    }
    try {
      const prepared = doc.prepareForDgn(seed);
      const { bytes, modelIndex } = doc.toDgnWithReport(seed, { prepare: true });
      const back = wasm.read(bytes);
      try {
        assert.equal(back.entityCount(0), prepared.entityCount(0), path.basename(file));
        assert.equal(doc.info().models[modelIndex].index, modelIndex);
        written++;
      } finally {
        back.free();
        prepared.free();
      }
    } finally {
      doc.free();
    }
  }
  assert.ok(written >= 2, `${written} of ${files.length}`);
  t.diagnostic(`${written} of ${files.length} samples written and read back`);
});
