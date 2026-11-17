// Axonometric projection used for the landing page district drawing (scripts/lib/axonometric.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { collectPolygons, axonometric, newell } from '../scripts/lib/axonometric.mjs';

const require = createRequire(import.meta.url);

/** A closed box as six polygons; `flip` reverses some rings to mimic files with mixed winding. */
function box(x0, y0, z0, w, d, h, flip = false) {
	const p = (x, y, z) => ({ x: x0 + x, y: y0 + y, z: z0 + z });
	const faces = [
		[p(0, 0, 0), p(0, d, 0), p(w, d, 0), p(w, 0, 0)], // ground
		[p(0, 0, h), p(w, 0, h), p(w, d, h), p(0, d, h)], // roof
		[p(0, 0, 0), p(w, 0, 0), p(w, 0, h), p(0, 0, h)], // south
		[p(w, 0, 0), p(w, d, 0), p(w, d, h), p(w, 0, h)], // east
		[p(w, d, 0), p(0, d, 0), p(0, d, h), p(w, d, h)], // north
		[p(0, d, 0), p(0, 0, 0), p(0, 0, h), p(0, d, h)], // west
	];
	return faces.map((ring, i) => ({ owner: 0, ring: flip && i % 2 ? [...ring].reverse() : ring }));
}

test('a box shows its roof and two walls, whatever the ring winding', () => {
	for (const flip of [false, true]) {
		const { faces, viewBox } = axonometric(box(0, 0, 0, 10, 6, 4, flip));
		assert.equal(faces.length, 3, `flip=${flip}`);
		assert.deepEqual(faces.map((f) => f.kind).sort(), ['roof', 'wall-a', 'wall-b']);
		assert.equal(viewBox[0], 0);
		assert.equal(viewBox[1], 0);
		assert.ok(viewBox[2] > 0 && viewBox[3] > 0);
		for (const f of faces) assert.match(f.d, /^M[\d. L]+Z$/);
	}
});

test('georeferenced coordinates project like local ones', () => {
	const local = axonometric(box(0, 0, 0, 10, 6, 4));
	const geo = axonometric(box(500_000, 5_600_000, 50, 10, 6, 4));
	assert.deepEqual(geo, local);
});

test('faces are painted far to near', () => {
	// Two boxes in a row along the view direction: the nearer one is drawn last.
	const polys = [...box(0, 40, 0, 4, 4, 4).map((p) => ({ ...p, owner: 1 })), ...box(0, 0, 0, 4, 4, 4)];
	const { faces } = axonometric(polys);
	assert.equal(faces.length, 6);
	const firstRoof = faces.findIndex((f) => f.kind === 'roof');
	const lastRoof = faces.findLastIndex((f) => f.kind === 'roof');
	assert.ok(firstRoof < lastRoof);
});

test('collectPolygons walks groups, skips hidden entities and stops at the depth limit', () => {
	const poly = { visible: true, kind: { type: 'polygon', exterior: [{ x: 0, y: 0, z: 0 }, { x: 1, y: 0, z: 0 }, { x: 1, y: 1, z: 0 }] } };
	const hidden = { ...poly, visible: false };
	const group = (children) => ({ visible: true, kind: { type: 'group', children } });
	assert.equal(collectPolygons([group([poly, hidden]), poly]).length, 2);
	let deep = poly;
	for (let i = 0; i < 12; i++) deep = group([deep]);
	assert.equal(collectPolygons([deep]).length, 0);
	assert.equal(collectPolygons([deep], 16).length, 1);
	assert.deepEqual(collectPolygons([{ kind: { type: 'line' } }, null]), []);
});

test('newell normal of a counter-clockwise square points up', () => {
	const n = newell([{ x: 0, y: 0, z: 0 }, { x: 1, y: 0, z: 0 }, { x: 1, y: 1, z: 0 }, { x: 0, y: 1, z: 0 }]);
	assert.deepEqual([n.x, n.y, Math.round(n.z * 1e9) / 1e9], [0, 0, 1]);
});

test('the district sample gives every building a visible roof', () => {
	const { read } = require('cadkit-wasm/node');
	const doc = read(new Uint8Array(readFileSync(new URL('../public/samples/cadkit-sample-district.gml', import.meta.url))));
	let entities;
	try {
		entities = doc.toJSON().models[0].entities;
	} finally {
		doc.free();
	}
	const polygons = collectPolygons(entities);
	const owners = new Set(polygons.map((p) => p.owner));
	assert.equal(owners.size, entities.length);
	const { faces } = axonometric(polygons);
	const roofs = faces.filter((f) => f.kind === 'roof').length;
	assert.ok(roofs >= entities.length, `${roofs} roofs for ${entities.length} buildings`);
	assert.ok(faces.length < polygons.length, 'back faces are hidden');
});
