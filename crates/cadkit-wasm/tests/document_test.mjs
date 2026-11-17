// The access API against toJSON(), pick paths, document operations and the DXF report.
import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { wasm, thrown, DXF, encode, blockDxf, sampleFiles, readBytes, samplesDir, corpusDir } from "./helpers.mjs";

/** Opens every available sample that reads; skips the others. */
function openSamples() {
  const out = [];
  for (const file of sampleFiles()) {
    try {
      out.push({ name: path.basename(file), file, doc: wasm.read(readBytes(file)) });
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) throw e;
    }
  }
  return out;
}

/** Every item of entities(model, options), page by page. */
function allPages(doc, model, options) {
  const items = [];
  let offset = 0;
  for (;;) {
    const page = doc.entities(model, { ...options, offset });
    items.push(...page.entities);
    if (page.next === null) return { total: page.total, items };
    offset = page.next;
  }
}

function walkJson(entities, visit, prefix = []) {
  entities.forEach((e, i) => {
    const p = [...prefix, i];
    visit(e, p);
    if (e.kind.type === "group") walkJson(e.kind.children, visit, p);
  });
}

test("access API returns the same entities as toJSON()", () => {
  const samples = openSamples();
  assert.ok(samples.length >= 4, "the site samples are committed");
  for (const { name, doc } of samples) {
    try {
      const json = doc.toJSON();
      const models = doc.models();
      assert.equal(models.length, json.models.length, name);
      json.models.forEach((m, mi) => {
        assert.deepEqual(
          [models[mi].name, models[mi].kind, models[mi].is3d, models[mi].entities, models[mi].id ?? null],
          [m.name, m.kind, m.is_3d, m.entities.length, m.id ?? null],
          name,
        );
        assert.deepEqual(models[mi].props, m.props);
        assert.equal(doc.entityCount(mi), m.entities.length);
        // Paged reading in small pages returns every top-level entity, in order.
        const seen = [];
        let offset = 0;
        for (;;) {
          const page = doc.entities(mi, { offset, limit: 7 });
          assert.equal(page.total, m.entities.length);
          assert.equal(page.offset, offset);
          for (const item of page.entities) seen.push(item);
          if (page.next === null) break;
          assert.equal(page.next, offset + page.entities.length);
          offset = page.next;
        }
        assert.equal(seen.length, m.entities.length, name);
        seen.forEach((item, i) => {
          assert.equal(item.index, i);
          assert.deepEqual(item.entity, m.entities[i]);
        });
        // entity(path) at every level of the model tree, and into blocks.
        walkJson(m.entities, (e, p) => {
          assert.deepEqual(doc.entity(mi, p), e, `${name} ${p}`);
          if (p.length === 1) assert.deepEqual(doc.entity(mi, p[0]), e);
          if (e.kind.type === "insert") {
            const block = json.blocks.find((b) => b.name === e.kind.block);
            if (block && block.entities.length) {
              assert.deepEqual(doc.entity(mi, [...p, block.entities.length - 1]), block.entities.at(-1));
            }
          }
        });
        assert.equal(doc.entity(mi, [m.entities.length]), null);
        // Filters.
        const kinds = new Set(m.entities.map((e) => e.kind.type));
        for (const kind of kinds) {
          const page = allPages(doc, mi, { kinds: [kind], limit: 100000 });
          const expected = m.entities.map((e, i) => [e, i]).filter(([e]) => e.kind.type === kind);
          assert.equal(page.total, expected.length);
          assert.deepEqual(page.items.map((x) => x.index), expected.map(([, i]) => i));
        }
        const layer = m.entities[0]?.layer ?? "0";
        const onLayer = m.entities.filter((e) => (e.layer ?? "0") === layer).length;
        assert.equal(doc.entities(mi, { layers: [layer] }).total, onLayer);
        // Viewports.
        const vps = [];
        walkJson(m.entities, (e, p) => e.kind.type === "viewport" && vps.push({ path: p, entity: e }));
        assert.deepEqual(doc.viewports(mi), vps);
        // Stats of one model.
        const kindsCount = {};
        const layers = {};
        const unknown = {};
        let nested = 0;
        walkJson(m.entities, (e, p) => {
          kindsCount[e.kind.type] = (kindsCount[e.kind.type] ?? 0) + 1;
          layers[e.layer ?? "0"] = (layers[e.layer ?? "0"] ?? 0) + 1;
          if (e.kind.type === "unknown") unknown[e.kind.type_name] = (unknown[e.kind.type_name] ?? 0) + 1;
          if (p.length > 1) nested++;
        });
        const stats = doc.stats(mi);
        assert.deepEqual(stats.kinds, kindsCount, name);
        assert.deepEqual(stats.layers, layers);
        assert.deepEqual(stats.unknownTypes, unknown);
        assert.equal(stats.entities, m.entities.length);
        assert.equal(stats.nested, nested);
        assert.equal(stats.blockEntities, 0);
      });
      // Blocks.
      const blocks = doc.blocks();
      assert.equal(blocks.length, json.blocks.length);
      json.blocks.forEach((b, i) => {
        assert.deepEqual(
          [blocks[i].name, blocks[i].entities, blocks[i].basePoint, blocks[i].description ?? null, blocks[i].isXref],
          [b.name, b.entities.length, b.base_point, b.description ?? null, b.is_xref],
        );
        assert.deepEqual(doc.block(b.name), b);
      });
      assert.equal(doc.block("no such block \u0000"), null);
      const all = doc.stats();
      let blockEntities = 0;
      for (const b of json.blocks) walkJson(b.entities, () => blockEntities++);
      assert.equal(all.blockEntities, blockEntities);
    } finally {
      doc.free();
    }
  }
});

test("entities() pages are bounded by maxOutputBytes", () => {
  const doc = wasm.read(readBytes(path.join(samplesDir, "cadkit-sample-plan.dxf")));
  try {
    const total = doc.entityCount(0);
    const small = doc.entities(0, { limit: 100000, maxOutputBytes: 4096 });
    assert.ok(small.entities.length > 0 && small.entities.length < total);
    assert.ok(JSON.stringify(small.entities).length <= 4096 + 64);
    assert.equal(small.next, small.entities.length);
    const big = doc.toJSON().models[0].entities.reduce((a, e) => Math.max(a, JSON.stringify(e).length), 0);
    const e = thrown(() => doc.entities(0, { maxOutputBytes: 64, kinds: ["polyline"] }));
    assert.ok(big > 64);
    assert.equal(e?.kind, "limit_exceeded");
    assert.equal(e.limit, "maxOutputBytes");
  } finally {
    doc.free();
  }
});

test("bbox of a model and of an entity path", () => {
  const doc = wasm.read(blockDxf({ columns: 2, rows: 1 }));
  try {
    const all = doc.bbox(0);
    assert.deepEqual(all.min.slice(0, 2), [0, -10]);
    const free = doc.bbox(0, 0);
    assert.deepEqual([free.min, free.max], [[0, -10, 0], [10, -10, 0]]);
    // The insert: both array cells (5 units apart) of a 4 x 3 block.
    const insert = doc.bbox(0, [1]);
    assert.deepEqual([insert.min.slice(0, 2), insert.max.slice(0, 2)], [[0, 0], [9, 3]]);
    // One part of the block, in every cell.
    const circle = doc.bbox(0, [1, 1]);
    assert.deepEqual([circle.min.slice(0, 2), circle.max.slice(0, 2)], [[1, 1], [8, 3]]);
    assert.equal(doc.bbox(0, [5]), null);
  } finally {
    doc.free();
  }
});

/** Picks on a grid over the model and checks every hit's paths. */
function pickGrid(doc, model, steps = 24) {
  const bb = doc.bbox(model);
  if (!bb) return { hits: 0, agree: 0, inBlocks: 0 };
  const w = bb.max[0] - bb.min[0] || 1;
  const h = bb.max[1] - bb.min[1] || 1;
  const tolerance = Math.max(w, h) / 40;
  let hits = 0;
  let agree = 0;
  let inBlocks = 0;
  const failures = [];
  for (let i = 0; i <= steps; i++) {
    for (let j = 0; j <= steps; j++) {
      const x = bb.min[0] + (w * i) / steps;
      const y = bb.min[1] + (h * j) / steps;
      const hit = doc.pick(model, x, y, { tolerance });
      if (!hit) continue;
      hits++;
      const e = doc.entity(model, hit.path);
      const owner = doc.entity(model, hit.modelPath);
      const ok =
        e !== null && owner !== null &&
        e.kind.type === hit.kind && (e.id ?? null) === hit.id &&
        owner.kind.type === hit.modelKind && (owner.id ?? null) === hit.modelId &&
        hit.path.length >= hit.modelPath.length &&
        hit.modelPath.every((v, k) => hit.path[k] === v);
      if (ok) agree++;
      else failures.push({ x, y, hit, e: e && { type: e.kind.type, id: e.id } });
      if (hit.path.length > hit.modelPath.length) inBlocks++;
      // The legacy pick finds the same candidate.
      const legacy = doc.pickEntity(model, x, y, tolerance);
      assert.deepEqual(legacy?.bbox, hit.bbox);
      assert.equal(legacy.distance, hit.distance);
    }
  }
  assert.deepEqual(failures.slice(0, 3), []);
  return { hits, agree, inBlocks };
}

test("pick paths resolve to the entity that was hit (100 % of a grid)", (t) => {
  const site = wasm.read(readBytes(path.join(samplesDir, "cadkit-sample-site.dxf")));
  try {
    const r = pickGrid(site, 0);
    assert.ok(r.hits >= 50, `${r.hits} hits`);
    assert.equal(r.agree, r.hits);
    assert.ok(r.inBlocks >= 5, `${r.inBlocks} hits in blocks`);
  } finally {
    site.free();
  }
  for (const name of ["cadkit-sample-buildings.gml", "cadkit-sample-district.gml"]) {
    const doc = wasm.read(readBytes(path.join(samplesDir, name)));
    try {
      const r = pickGrid(doc, 0);
      assert.ok(r.hits >= 20, `${name}: ${r.hits} hits`);
      assert.equal(r.agree, r.hits);
      // A polygon inside groups: groups belong to the model's own tree, so the model entity
      // that owns the hit is the polygon itself and both paths are equal.
      const bb = doc.bbox(0);
      const hit = doc.pick(0, (bb.min[0] + bb.max[0]) / 2, (bb.min[1] + bb.max[1]) / 2, { tolerance: 1e9 });
      assert.ok(hit.path.length >= 2);
      assert.deepEqual(hit.modelPath, hit.path);
      assert.equal(hit.modelKind, "polygon");
      assert.equal(doc.entity(0, hit.path.slice(0, 1)).kind.type, "group");
    } finally {
      doc.free();
    }
  }
  // Third-party and corpus drawings with blocks, when present.
  let extra = 0;
  for (const file of sampleFiles({ maxBytes: 1 << 20 })) {
    if (file.startsWith(samplesDir) && !file.includes("third-party")) continue;
    if (!/\.(dxf|dwg)$/i.test(file) || extra >= 6) continue;
    let doc;
    try {
      doc = wasm.read(readBytes(file));
    } catch {
      continue;
    }
    try {
      if (!doc.stats(0).kinds.insert) continue;
      extra++;
      const r = pickGrid(doc, 0, 12);
      assert.equal(r.agree, r.hits, file);
    } finally {
      doc.free();
    }
  }
  if (!extra) t.diagnostic("no third-party or corpus drawing with blocks available");
});

test("pick result shape, filters and budgets", () => {
  const doc = wasm.read(blockDxf());
  try {
    const hit = doc.pick(0, 2, 3, { tolerance: 0.1 });
    assert.deepEqual(hit.path, [1, 1]);
    assert.deepEqual(hit.modelPath, [1]);
    assert.equal(hit.kind, "circle");
    assert.equal(hit.modelKind, "insert");
    assert.equal(hit.block, "B");
    assert.equal(hit.layer, "PARTS");
    assert.equal(hit.modelLayer, "PARTS");
    assert.equal(hit.truncated, false);
    assert.equal(doc.pick(0, 2, 3, { tolerance: 0.1, excludedKinds: ["circle"] }), null);
    assert.equal(doc.pick(0, 2, 3, { tolerance: 0.1, excludedLayers: ["PARTS"] }), null);
    assert.equal(doc.pick(0, 500, 500, { tolerance: 0.1 }), null);
    assert.equal(thrown(() => doc.pick(0, 2, 3, {}))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.pick(0, 2, 3, { tolerance: -1 }))?.kind, "bad_argument");
    assert.equal(thrown(() => doc.pick(0, 2, 3, { tolerance: 0.1, maxOutputBytes: 64 }))?.kind, "limit_exceeded");
    assert.deepEqual(doc.pick(0, 2, 3, '{"tolerance": 0.1}'), hit);
    // pickEntity's index is the visiting order: the circle of the block is the third entity visited.
    assert.equal(doc.pickEntity(0, 2, 3, 0.1).index, 2);
  } finally {
    doc.free();
  }
});

test("explodeInserts honours maxDepth and maxEntities", () => {
  const doc = wasm.read(blockDxf({ inserts: 10 }));
  try {
    const copy = doc.clone();
    const r = copy.explodeInserts();
    assert.deepEqual(r, { truncated: false, work: r.work, entities: 21 });
    assert.equal(copy.stats(0).kinds.insert, undefined);
    copy.free();
    const cut = doc.clone();
    const e = thrown(() => cut.explodeInserts({ maxEntities: 19 }));
    assert.deepEqual([e?.kind, e.limit, e.limitValue, e.needed], ["limit_exceeded", "maxEntities", 19, 20]);
    assert.equal(cut.entityCount(0), 11, "unchanged after the error");
    assert.equal(cut.explodeInserts({ maxEntities: 20 }).entities, 21);
    cut.free();
    const shallow = doc.clone();
    assert.equal(shallow.explodeInserts({ maxDepth: 0 }).entities, 11);
    shallow.free();
    assert.equal(doc.entityCount(0), 11, "the original is unchanged");
  } finally {
    doc.free();
  }
});

test("explodeInserts expands an insert array completely or not at all", () => {
  const doc = wasm.read(blockDxf({ columns: 3, rows: 2 }));
  try {
    // 6 cells of a line and a circle: 12 entities.
    const refused = doc.clone();
    const e = thrown(() => refused.explodeInserts({ maxEntities: 11 }));
    assert.deepEqual([e?.kind, e.needed], ["limit_exceeded", 12]);
    assert.equal(refused.stats(0).kinds.insert, 1, "the array is still one insert");
    refused.free();
    const done = doc.clone();
    const r = done.explodeInserts({ maxEntities: 12 });
    assert.deepEqual([r.truncated, r.entities], [false, 13]);
    assert.deepEqual(done.stats(0).kinds, { line: 7, circle: 6 });
    done.free();
  } finally {
    doc.free();
  }
});

test("transform, retainLayers, retainKinds, purgeUnused and clone change the document in place", () => {
  const doc = wasm.read(blockDxf());
  try {
    const copy = doc.clone();
    copy.transform({ translate: [100, 50] });
    assert.deepEqual(copy.entity(0, 0).kind.start, { x: 100, y: 40, z: 0 });
    assert.deepEqual(doc.entity(0, 0).kind.start, { x: 0, y: -10, z: 0 }, "clone is independent");
    copy.transform([1, 0, 0, -100, 0, 1, 0, -50, 0, 0, 1, 0]);
    assert.deepEqual(copy.entity(0, 0).kind.start, { x: 0, y: -10, z: 0 });
    copy.transform([[2, 0, 0, 0], [0, 2, 0, 0], [0, 0, 2, 0], [0, 0, 0, 1]]);
    assert.deepEqual(copy.entity(0, 0).kind.end, { x: 20, y: -20, z: 0 });
    copy.transform({ rotateZ: Math.PI / 2, origin: [0, -20] });
    const end = copy.entity(0, 0).kind.end;
    assert.ok(Math.abs(end.x - 0) < 1e-9 && Math.abs(end.y - 0) < 1e-9, JSON.stringify(end));
    copy.transform({ scale: [0.5, 0.5, 1] });
    copy.transform('{"matrix":[1,0,0,1,0,1,0,1,0,0,1,0]}');
    copy.free();

    const layers = doc.clone();
    const r = layers.retainLayers(["FREE"]);
    assert.equal(r.entities, 1);
    assert.equal(layers.entityCount(0), 1);
    assert.equal(layers.entity(0, 0).layer, "FREE");
    const purged = layers.purgeUnused();
    assert.deepEqual(Object.keys(purged).sort(), ["blocks", "layers", "linetypes", "textStyles"]);
    assert.equal(purged.blocks, 1);
    assert.equal(layers.blocks().length, 0);
    layers.free();

    const kinds = doc.clone();
    assert.deepEqual(kinds.retainKinds(["insert"]), { layers: 0, entities: 1 });
    assert.equal(kinds.entityCount(0), 1);
    assert.equal(kinds.retainKinds(["mtext", "line"]).entities, 1);
    assert.equal(kinds.entityCount(0), 0);
    kinds.free();
  } finally {
    doc.free();
  }
});

test("toDxfWithReport lists what DXF output left out", (t) => {
  const doc = wasm.read(encode(DXF));
  try {
    const plain = doc.toDxf("r2000");
    const report = doc.toDxfWithReport("r2000");
    assert.equal(report.text, plain);
    assert.deepEqual(report.warnings, []);
    const bytes = doc.toDxfBytesWithReport("r2000", "windows-1254");
    assert.ok(bytes.bytes instanceof Uint8Array);
    assert.deepEqual(bytes.bytes, doc.toDxfBytes("r2000", "windows-1254"));
  } finally {
    doc.free();
  }
  const json = JSON.parse(wasm.read(encode(DXF)).toJsonString());
  json.models[0].entities.push({ ...json.models[0].entities[0], kind: { type: "unknown", type_name: "test.record" } });
  const unknown = wasm.CadDocument.fromJsonString(JSON.stringify(json));
  try {
    const r = unknown.toDxfWithReport();
    assert.equal(r.warnings.length, 1);
    assert.equal(r.warnings[0].code, "dxf.write.skipped");
    assert.match(r.warnings[0].message, /test\.record/);
  } finally {
    unknown.free();
  }
  const corpus = corpusDir();
  const sheet = corpus && path.join(corpus, "acadsharp", "sample_AC1032.dwg");
  if (!sheet) return t.diagnostic("corpus missing: DWG report not checked");
  let dwg;
  try {
    dwg = wasm.read(readBytes(sheet));
  } catch {
    return t.diagnostic("DWG sample missing");
  }
  try {
    // Entities DXF has no record for (unknown ones the writer cannot reproduce) are listed.
    const r = dwg.toDxfWithReport();
    const unknown = dwg.stats().unknownTypes;
    assert.ok(r.warnings.length > 0);
    for (const w of r.warnings) {
      assert.equal(w.code, "dxf.write.skipped");
      const type = w.message.split(":")[0];
      assert.ok(unknown[type] > 0, type);
    }
    assert.equal(r.text, dwg.toDxf());
  } finally {
    dwg.free();
  }
});

test("toCityGmlReport returns an object, toCityGmlWithReport the same as text", () => {
  const json = JSON.parse(wasm.read(encode(DXF)).toJsonString());
  json.models[0].entities = [json.models[0].entities[0], { ...json.models[0].entities[0], kind: { type: "unknown", type_name: "x" } }];
  const doc = wasm.CadDocument.fromJsonString(JSON.stringify(json));
  try {
    const r = doc.toCityGmlReport("urn:cadkit:synthetic", 2, true);
    assert.deepEqual(r, JSON.parse(doc.toCityGmlWithReport("urn:cadkit:synthetic", 2, true)));
    assert.equal(r.report.entities, 2);
    assert.equal(r.report.geometry_entities, 1);
    assert.equal(r.report.metadata_only_entities, 1);
    assert.deepEqual(Object.keys(r.report.issues[0]).sort(), ["code", "entity_type", "model", "path"]);
    assert.equal(thrown(() => doc.toCityGmlReport("urn:cadkit:synthetic", 2))?.kind, "unsupported");
  } finally {
    doc.free();
  }
});

test("memoryBytes reports the wasm memory size", () => {
  const before = wasm.memoryBytes();
  assert.ok(Number.isInteger(before) && before % 65536 === 0 && before > 0);
  const doc = wasm.read(blockDxf({ inserts: 2000 }));
  doc.explodeInserts();
  assert.ok(wasm.memoryBytes() >= before);
  doc.free();
});
