// A deterministic mutation run over the drawing samples available in this checkout (the
// committed site samples always; third-party samples and the public corpus when present):
// truncated and bit-flipped inputs must give a document or an error with a documented kind,
// never a trap. Bounded to a fixed number of inputs per sample and a time budget.
import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { wasm, KINDS, sampleFiles, readBytes } from "./helpers.mjs";

const TARGET_INPUTS = 600;
const MAX_FILES = 48;
let PER_FILE = 10;
const TIME_BUDGET_MS = 40_000;

/** mulberry32: a small seeded generator, so every run mutates the same bytes. */
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function* mutations(bytes, seed) {
  const r = rng(seed);
  for (let i = 0; i < PER_FILE; i++) {
    const out = bytes.slice();
    switch (i % 5) {
      case 0: // truncation
        yield ["truncate", bytes.subarray(0, Math.floor(r() * bytes.length))];
        continue;
      case 1: // a few bit flips
        for (let k = 0; k < 1 + Math.floor(r() * 8); k++) {
          out[Math.floor(r() * out.length)] ^= 1 << Math.floor(r() * 8);
        }
        break;
      case 2: // a run of 0xFF (large counts and offsets)
      case 3: { // a run of random bytes
        const at = Math.floor(r() * out.length);
        const len = Math.min(out.length - at, 1 + Math.floor(r() * 64));
        for (let k = 0; k < len; k++) out[at + k] = i % 5 === 2 ? 0xff : Math.floor(r() * 256);
        break;
      }
      case 4: { // the header kept, a chunk repeated
        const at = Math.floor(r() * out.length);
        const chunk = bytes.subarray(at, Math.min(bytes.length, at + 4096));
        const joined = new Uint8Array(out.length + chunk.length);
        joined.set(out.subarray(0, at));
        joined.set(chunk, at);
        joined.set(out.subarray(at), at + chunk.length);
        yield ["repeat", joined];
        continue;
      }
    }
    yield [["", "flip", "ff", "random"][i % 5], out];
  }
}

/** Runs one operation; returns "ok" or the error kind, failing on a trap or a kind-less error. */
function run(label, fn, stats) {
  try {
    fn();
    stats.ok++;
  } catch (e) {
    if (e instanceof WebAssembly.RuntimeError) {
      throw new Error(`trap in ${label}: ${e.message}`);
    }
    assert.ok(e instanceof Error, `${label}: not an Error`);
    assert.ok(KINDS.includes(e.kind), `${label}: kind ${e.kind} (${e.message})`);
    assert.doesNotMatch(e.message, /^([^:]{1,40}): \1[: ]/i, e.message);
    stats.errors[e.kind] = (stats.errors[e.kind] ?? 0) + 1;
  }
}

test("mutated samples never trap and every error has a kind", (t) => {
  const files = sampleFiles({ maxBytes: 2 << 20 }).slice(0, MAX_FILES);
  assert.ok(files.length >= 4);
  PER_FILE = Math.max(10, Math.ceil(TARGET_INPUTS / files.length));
  const stats = { inputs: 0, ok: 0, errors: {} };
  const started = Date.now();
  let seed = 1;
  outer: for (const file of files) {
    const bytes = readBytes(file);
    for (const [how, input] of mutations(bytes, seed++)) {
      if (Date.now() - started > TIME_BUDGET_MS) break outer;
      stats.inputs++;
      const label = `${path.basename(file)} ${how} #${stats.inputs}`;
      let doc;
      run(label, () => {
        doc = wasm.read(input, { maxObjects: 200_000 });
      }, stats);
      if (!doc) continue;
      try {
        run(`${label} info`, () => doc.info(), stats);
        run(`${label} stats`, () => doc.stats(), stats);
        run(`${label} entities`, () => doc.entities(0, { limit: 50 }), stats);
        run(`${label} svg`, () => doc.toSvgBounded({ maxOutputBytes: 2 << 20, maxWork: 2_000_000 }), stats);
        run(`${label} pick`, () => {
          const bb = doc.bbox(0);
          if (bb) doc.pick(0, (bb.min[0] + bb.max[0]) / 2, (bb.min[1] + bb.max[1]) / 2, { tolerance: 1 });
        }, stats);
        run(`${label} explode`, () => doc.clone().explodeInserts({ maxEntities: 20_000 }), stats);
        run(`${label} dxf`, () => doc.toDxfWithReport(), stats);
      } finally {
        doc.free();
      }
    }
  }
  t.diagnostic(`${files.length} files, ${stats.inputs} inputs, ${stats.ok} calls ok, errors ${JSON.stringify(stats.errors)}, ${Date.now() - started} ms`);
  assert.ok(stats.inputs >= 40, `${stats.inputs} inputs`);
  // The module is still alive after the run.
  assert.equal(wasm.detect(new TextEncoder().encode("0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n")), "dxf");
});
