// Renderer buffers: surfaceMesh() and lineBuffer() hand out typed arrays, an item table whose
// paths resolve with entity(), budgets that truncate instead of trapping, and a ray pick.
import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { wasm, thrown, blockDxf, encode, polylineDocument, boxRings, samplesDir, readBytes } from "./helpers.mjs";

const BUILDINGS = path.join(samplesDir, "cadkit-sample-buildings.gml");

/** Every item's path from the columnar table, checked against item(i) and entity(). */
function checkItems(doc, buffer) {
  const table = buffer.items();
  assert.equal(table.count, buffer.itemCount());
  for (const column of ["pathStart", "pathLength", "modelPathLength", "layer"]) {
    assert.ok(table[column] instanceof Uint32Array, column);
    assert.equal(table[column].length, table.count, column);
  }
  assert.ok(table.kind instanceof Uint8Array && table.surface instanceof Uint8Array);
  assert.ok(table.paths instanceof Uint32Array);
  const layers = buffer.layers();
  for (let i = 0; i < table.count; i++) {
    const p = [...table.paths.subarray(table.pathStart[i], table.pathStart[i] + table.pathLength[i])];
    const item = buffer.item(i);
    assert.deepEqual(item.path, p);
    assert.deepEqual(item.modelPath, p.slice(0, table.modelPathLength[i]));
    assert.equal(item.kind, table.kindNames[table.kind[i]]);
    assert.equal(item.surface, table.surface[i] === 1);
    assert.equal(item.layer, layers[table.layer[i]].name);
    const entity = doc.entity(0, p);
    assert.ok(entity, `item ${i} path ${p} resolves`);
    assert.equal(entity.kind.type, item.kind);
    assert.ok(doc.entity(0, item.modelPath));
  }
  assert.equal(buffer.item(table.count), null);
  return table;
}

test("surfaceMesh of the buildings sample: faces, duplicates, buffers and item paths", () => {
  const doc = wasm.read(readBytes(BUILDINGS));
  const mesh = doc.surfaceMesh();
  try {
    const report = mesh.report();
    assert.equal(report.faces, 13);
    assert.equal(report.duplicates, 13);
    assert.equal(report.truncated, false);
    assert.equal(report.triangles, mesh.triangleItems().length);
    assert.equal(report.segments, mesh.segmentItems().length);
    const positions = mesh.positions();
    const indices = mesh.indices();
    assert.ok(positions instanceof Float32Array && mesh.normals() instanceof Float32Array);
    assert.equal(mesh.normals().length, positions.length);
    assert.equal(mesh.colors().length, positions.length);
    assert.equal(indices.length, 3 * report.triangles);
    assert.ok(indices.every((i) => i < positions.length / 3));
    assert.equal(mesh.segments().length, 6 * report.segments);
    assert.equal(mesh.segmentColors().length, 6 * report.segments);
    const { origin, min, max, maxError } = mesh.bounds();
    assert.equal(origin.length, 3);
    assert.ok(origin[0] > 400_000, "map coordinates stay in the f64 origin");
    for (let i = 0; i < positions.length; i += 3) {
      for (let k = 0; k < 3; k++) assert.ok(positions[i + k] >= min[k] - 1e-6 && positions[i + k] <= max[k] + 1e-6);
    }
    assert.ok(maxError >= 0 && maxError < 1e-3);
    // The arrays are copies: changing one does not change the mesh.
    positions[0] = 1e9;
    assert.notEqual(mesh.positions()[0], 1e9);
    // Layers are contiguous ranges that cover every triangle and segment.
    const layers = mesh.layers();
    assert.equal(layers.reduce((n, l) => n + l.triangleCount, 0), report.triangles);
    assert.equal(layers.reduce((n, l) => n + l.segmentCount, 0), report.segments);
    assert.deepEqual(layers.filter((l) => l.triangleCount).map((l) => l.name).sort(), ["GroundSurface", "RoofSurface", "WallSurface"]);
    const table = checkItems(doc, mesh);
    assert.equal(table.count, 13);
    assert.ok(mesh.byteLength() >= 4 * positions.length);
  } finally {
    mesh.free();
    doc.free();
  }
});

test("SurfaceMesh.pick hits the nearest visible face", () => {
  const doc = wasm.read(readBytes(BUILDINGS));
  const mesh = doc.surfaceMesh();
  try {
    const { max } = mesh.bounds();
    const layers = mesh.layers();
    const top = max[2] + 100;
    const roof = mesh.pick([0, 0, top], [0, 0, -1]);
    assert.ok(roof, "a ray down through the middle hits a roof");
    assert.equal(mesh.item(roof.item).layer, "RoofSurface");
    assert.equal(mesh.triangleItems()[roof.triangle], roof.item);
    // The hit lies on the triangle's plane: its corners are at the hit height or around it.
    const positions = mesh.positions();
    const corners = [...mesh.indices().subarray(3 * roof.triangle, 3 * roof.triangle + 3)].map((i) => positions[3 * i + 2]);
    const z = top - roof.distance;
    assert.ok(Math.min(...corners) - 1e-4 <= z && z <= Math.max(...corners) + 1e-4, `${z} in ${corners}`);
    const roofLayer = layers.findIndex((l) => l.name === "RoofSurface");
    const below = mesh.pick([0, 0, top], [0, 0, -1], [roofLayer]);
    assert.ok(below && below.distance > roof.distance);
    assert.notEqual(mesh.item(below.item).layer, "RoofSurface");
    assert.equal(mesh.pick([0, 0, top], [0, 0, -1], layers.map((_, i) => i)), null);
    assert.equal(mesh.pick([0, 0, top], [0, 0, 1]), null, "a ray away from the model");
    assert.deepEqual(mesh.pick(new Float64Array([0, 0, top]), new Float32Array([0, 0, -1])), roof);
    for (const [o, d, h] of [
      [[0, 0], [0, 0, -1]], [[0, 0, NaN], [0, 0, -1]], [[0, 0, 0], [0, 0, 0]], ["0,0,0", [0, 0, -1]],
      [[0, 0, 0], [0, 0, -1], [layers.length]], [[0, 0, 0], [0, 0, -1], [-1]], [[0, 0, 0], [0, 0, -1], "RoofSurface"],
      [[0, 0, 0], [0, 0, -1], ["RoofSurface"]], [[0, 0, 0], new DataView(new ArrayBuffer(24))],
    ]) {
      assert.equal(thrown(() => mesh.pick(o, d, h))?.kind, "bad_argument", JSON.stringify([o, d, h]));
    }
  } finally {
    mesh.free();
    doc.free();
  }
});

test("a box of closed polylines is a mesh only when polylines are faces", () => {
  const doc = wasm.CadDocument.fromJsonString(polylineDocument(boxRings()));
  try {
    const auto = doc.surfaceMesh();
    assert.equal(auto.report().faces, 6, "auto: the model is 3D");
    assert.equal(auto.report().triangles, 12);
    auto.free();
    const never = doc.surfaceMesh({ polylines: "never" });
    assert.deepEqual([never.report().faces, never.report().wires], [0, 6]);
    never.free();
    const bare = doc.surfaceMesh({ polylines: "always", outlines: false, wires: false });
    assert.deepEqual([bare.report().faces, bare.report().segments], [6, 0]);
    bare.free();
  } finally {
    doc.free();
  }
});

test("lineBuffer: 2D and 3D segments, paths through block references", () => {
  const doc = wasm.read(blockDxf({ inserts: 2 }));
  try {
    const flat = doc.lineBuffer();
    try {
      assert.equal(flat.components(), 2);
      const report = flat.report();
      // The free line and, per insert, the block's line and circle.
      assert.equal(report.drawn, 5);
      assert.equal(flat.segments().length, 2 * 2 * report.segments);
      assert.equal(flat.segmentItems().length, report.segments);
      const table = checkItems(doc, flat);
      // Geometry drawn from a block leads back to the insert that draws it.
      const fromBlock = [...Array(table.count).keys()].map((i) => flat.item(i)).filter((i) => i.path.length === 2);
      assert.equal(fromBlock.length, 4);
      for (const item of fromBlock) assert.equal(doc.entity(0, item.modelPath).kind.type, "insert");
      assert.ok(report.flattened >= 2, "circles are flattened");
    } finally {
      flat.free();
    }
    const deep = doc.lineBuffer({ threeD: true, tolerance: 0.5, background: null });
    assert.equal(deep.components(), 3);
    assert.equal(deep.segments().length, 2 * 3 * deep.report().segments);
    deep.free();
    const unexpanded = doc.lineBuffer({ expandInserts: false });
    assert.equal(unexpanded.report().drawn, 1);
    assert.equal(unexpanded.report().skipped.insert, 2);
    unexpanded.free();
  } finally {
    doc.free();
  }
});

test("budgets truncate, precision limits throw, ceilings are applied", () => {
  const doc = wasm.read(readBytes(BUILDINGS));
  try {
    const few = doc.surfaceMesh({ maxTriangles: 3 });
    assert.equal(few.report().truncated, true);
    assert.ok(few.report().triangles <= 3);
    few.free();
    const lines = doc.lineBuffer({ maxSegments: 1 });
    assert.equal(lines.report().truncated, true);
    assert.ok(lines.report().segments <= 1);
    lines.free();
    const walk = doc.surfaceMesh({ maxWork: 1 });
    assert.equal(walk.report().truncated, true);
    walk.free();
    // Larger budgets than the module can hold are lowered, not errors.
    const big = doc.surfaceMesh({ maxTriangles: 1e15, maxSegments: Number.MAX_SAFE_INTEGER, max_geometry_work: 1e12 });
    assert.equal(big.report().faces, 13);
    big.free();
  } finally {
    doc.free();
  }
  // A decimal offset such as 3.27 is not exact in f32 relative to the origin.
  const geo = wasm.read(encode([
    "0", "SECTION", "2", "ENTITIES",
    "0", "LINE", "8", "0", "10", "500000.1", "20", "4400000.3", "11", "500003.37", "21", "4400001.91",
    "0", "ENDSEC", "0", "EOF", "",
  ].join("\n")));
  try {
    const e = thrown(() => geo.lineBuffer({ maxError: 1e-15 }));
    assert.deepEqual([e?.kind, e?.limit], ["limit_exceeded", "maxError"]);
    const ok = geo.lineBuffer({ maxError: 1e-3 });
    assert.ok(ok.bounds().maxError > 1e-15 && ok.bounds().maxError <= 1e-3);
    assert.ok(Math.abs(ok.bounds().origin[0] - 500_000) < 100);
    ok.free();
  } finally {
    geo.free();
  }
});

test("buffers stay usable after errors and report a JS error after free()", () => {
  const doc = wasm.read(blockDxf());
  const mesh = doc.lineBuffer();
  mesh.free();
  const e = thrown(() => mesh.report());
  assert.ok(e instanceof Error);
  assert.ok(!(e instanceof WebAssembly.RuntimeError));
  const again = doc.lineBuffer();
  assert.ok(again.itemCount() > 0);
  again.free();
  doc.free();
});
