// Inputs that used to exhaust the 4 GiB wasm address space (a trap leaves the module unusable)
// are answered with limit_exceeded or a truncated result. Runs in its own process: the wasm
// memory grows to about 1.5 GB here and never shrinks.
import test from "node:test";
import assert from "node:assert/strict";
import { wasm, thrown, blockDxf } from "./helpers.mjs";

/** A DXF of `count` POINT entities, built without one huge JS string. */
function pointsDxf(count) {
  const head = new TextEncoder().encode("0\nSECTION\n2\nENTITIES\n");
  const tail = new TextEncoder().encode("0\nENDSEC\n0\nEOF\n");
  const point = new TextEncoder().encode("0\nPOINT\n10\n1\n20\n2\n");
  const out = new Uint8Array(head.length + point.length * count + tail.length);
  out.set(head, 0);
  for (let i = 0; i < count; i++) out.set(point, head.length + i * point.length);
  out.set(tail, head.length + point.length * count);
  return out;
}

test("4.9 million points stop at the default maxObjects instead of trapping", () => {
  const bytes = pointsDxf(4_900_000);
  const e = thrown(() => wasm.read(bytes));
  assert.equal(e?.kind, "limit_exceeded");
  assert.equal(e.limit, "maxObjects");
  assert.equal(e.limitValue, 1_500_000);
  assert.ok(wasm.memoryBytes() < 3.5 * 2 ** 30, `${wasm.memoryBytes()} bytes`);
  // The module still works, and the memory is reused rather than grown again.
  const grown = wasm.memoryBytes();
  assert.equal(thrown(() => wasm.read(bytes))?.kind, "limit_exceeded");
  assert.equal(wasm.memoryBytes(), grown);
  const small = wasm.read(pointsDxf(10));
  assert.equal(small.info().models[0].entities, 10);
  small.free();
});

test("a 32767 x 32767 insert array is refused by explodeInserts before it allocates", () => {
  const bytes = blockDxf({ columns: 32767, rows: 32767 });
  assert.ok(bytes.length < 1024);
  const doc = wasm.read(bytes);
  try {
    const before = wasm.memoryBytes();
    for (const options of [undefined, { maxEntities: 3_000_000 }, { maxEntities: 1e12 }]) {
      const e = thrown(() => doc.explodeInserts(options));
      assert.equal(e?.kind, "limit_exceeded");
      assert.equal(e.limit, "maxEntities");
      assert.ok(e.needed >= 32767 * 32767 * 2, String(e.needed));
    }
    assert.equal(wasm.memoryBytes(), before, "nothing was allocated");
    // The document is unchanged and usable.
    assert.equal(doc.entityCount(0), 2);
    assert.equal(doc.stats(0).kinds.insert, 1);
    // Rendering and picking the array stay bounded by their budgets.
    assert.equal(thrown(() => doc.toSvgBounded({ maxOutputBytes: 1 << 20 }))?.kind, "limit_exceeded");
    assert.equal(thrown(() => doc.pick(0, 1, 1, { tolerance: 1 }))?.kind, "limit_exceeded");
  } finally {
    doc.free();
  }
  // An array that fits is expanded.
  const fits = wasm.read(blockDxf({ columns: 300, rows: 300 }));
  try {
    const r = fits.explodeInserts();
    assert.deepEqual([r.truncated, r.entities], [false, 1 + 300 * 300 * 2]);
  } finally {
    fits.free();
  }
});
