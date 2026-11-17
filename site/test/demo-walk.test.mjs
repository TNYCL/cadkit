// The pick walk (src/demo/walk.ts) against cadkit-wasm itself: synthetic documents that exercise
// every rule of the engine's walk, then a grid of picks over every bundled sample (and the public
// corpus when it is there) where each hit must map to the entity the engine reported.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createCore } from '../src/demo/core.ts';
import { SAMPLES } from '../src/demo/samples.ts';
import { entityAt, locatePick, walkCount, WALK_MAX_DEPTH } from '../src/demo/walk.ts';

const load = createRequire(import.meta.url);
const site = fileURLToPath(new URL('..', import.meta.url));
const api = load('cadkit-wasm/node');
const bufferOf = (bytes) => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);

// ---------------------------------------------------------------------------------------------
// Synthetic documents: every line is one unit long at a position of its own, so a pick at its
// midpoint hits exactly that line.

let nextId = 1;
const P = (x, y) => ({ x, y, z: 0 });
const entity = (kind, extra = {}) => ({ id: nextId++, layer: '0', color: { type: 'by_layer' }, linetype: null, lineweight: { type: 'by_layer' }, visible: true, kind, attributes: [], props: {}, ...extra });
const line = (x, y, extra) => entity({ type: 'line', start: P(x, y), end: P(x + 1, y) }, extra);
const group = (children, extra) => entity({ type: 'group', group_kind: 'other', name: null, origin: P(0, 0), children }, extra);
const insert = (block, x, y, more = {}) =>
	entity({ type: 'insert', block, position: P(x, y), scale: { x: 1, y: 1, z: 1 }, rotation: 0, normal: { x: 0, y: 0, z: 1 }, columns: 1, rows: 1, column_spacing: 0, row_spacing: 0, ...more });
const block = (name, entities) => ({ name, id: nextId++, base_point: P(0, 0), entities, is_xref: false, xref_path: null, description: null, props: {} });
const docOf = (entities, blocks = []) => ({
	schema_version: 1,
	source: { format: 'dxf', version: 'AC1032', application: null, codepage: null },
	units: { unit: 'meter', meters_per_unit: 1 },
	layers: [],
	linetypes: [],
	text_styles: [],
	blocks,
	models: [{ name: 'Model', id: nextId++, kind: 'model', is_3d: false, entities, props: {} }],
	props: {},
	warnings: [],
});

/** Picks at (x + 0.5, y) with the engine and maps the counter back through the walk. */
function pickAt(json, x, y) {
	const d = api.CadDocument.fromJsonString(JSON.stringify(json));
	try {
		const hit = d.pickEntityBounded(0, x + 0.5, y, 0.05, { maxOutputBytes: 16384, maxWork: 2_000_000 }, true);
		if (!hit) return null;
		const target = locatePick(json, 0, hit.index);
		return { hit, target, found: target && entityAt(json, 0, target.part) };
	} finally {
		d.free();
	}
}

test('walk: groups, invisible entities and plain block references', () => {
	const json = docOf(
		[line(0, 0), group([line(0, 10), group([line(0, 20)]), line(0, 30, { visible: false })]), group([line(0, 40)], { visible: false }), insert('B', 100, 0), line(0, 50)],
		[block('B', [line(0, 0), line(0, 5)])]
	);
	const cases = [
		[0, 0, [0], [0], []],
		[0, 10, [1, 0], [1, 0], []],
		[0, 20, [1, 1, 0], [1, 1, 0], []],
		[100, 0, [3, 0], [3], ['B']],
		[100, 5, [3, 1], [3], ['B']],
		[0, 50, [4], [4], []],
	];
	for (const [x, y, part, owner, blocks] of cases) {
		const r = pickAt(json, x, y);
		assert.ok(r, `hit at ${x},${y}`);
		assert.deepEqual(r.target, { part, owner, blocks }, `${x},${y}`);
		assert.equal(r.found.entity.id, r.hit.id);
	}
	assert.equal(pickAt(json, 0, 30), null, 'invisible entities are not picked');
	assert.equal(pickAt(json, 0, 40), null, 'nor anything below an invisible group');
	// Visited: 1 + 2 (group) + 2 (block) + 1; the invisible ones are not counted.
	assert.equal(walkCount(json, 0), 6);
});

test('walk: arrays, nested and missing blocks, cycles and dimensions', () => {
	const json = docOf(
		[
			insert('ROW', 0, 0, { columns: 3, rows: 2, column_spacing: 10, row_spacing: 100 }),
			insert('OUTER', 0, 300),
			insert('NOPE', 0, 400),
			insert('SELF', 0, 500),
			entity({ type: 'dimension', dimension_type: 'linear', points: [P(0, 600), P(1, 600), P(0, 601)], text_position: P(0, 602), measurement: 1, text: null, block: 'DIM', style: null }),
			insert('EMPTY', 0, 700),
			line(0, 800),
		],
		[
			block('ROW', [line(0, 0), line(0, 3)]),
			block('OUTER', [line(0, 0), insert('ROW', 50, 0, { columns: 2, column_spacing: 10 })]),
			block('SELF', [line(0, 0), insert('SELF', 0, 1000)]),
			// Dimension blocks are drawn where they are: no insertion transform.
			block('DIM', [line(0, 600)]),
			block('EMPTY', []),
			// A second block of the same name is ignored, as in the engine.
			block('ROW', [line(0, 7)]),
		]
	);
	// Cell (column 2, row 1) of the array: the part is the block entity, whatever the cell.
	let r = pickAt(json, 20, 103);
	assert.deepEqual(r.target, { part: [0, 1], owner: [0], blocks: ['ROW'] });
	r = pickAt(json, 50 + 10, 303);
	assert.deepEqual(r.target, { part: [1, 1, 1], owner: [1], blocks: ['OUTER', 'ROW'] });
	assert.equal(r.found.entity.id, r.hit.id);
	// A reference to a missing block is visited itself (and has no geometry to pick).
	assert.equal(pickAt(json, 0, 400), null);
	// A block that inserts itself is expanded once; the inner reference is visited as an entity.
	r = pickAt(json, 0, 500);
	assert.deepEqual(r.target, { part: [3, 0], owner: [3], blocks: ['SELF'] });
	r = pickAt(json, 0, 600);
	assert.equal(r.hit.kind, 'line');
	assert.deepEqual(r.target, { part: [4, 0], owner: [4], blocks: ['DIM'] });
	r = pickAt(json, 0, 800);
	assert.deepEqual(r.target, { part: [6], owner: [6], blocks: [] });
	assert.equal(r.found.entity.id, r.hit.id);
	// 6 cells x 2, 1 + 2 x 2, the missing reference, 1 + the inner self reference, 1, 0, 1.
	assert.equal(walkCount(json, 0), 12 + 5 + 1 + 2 + 1 + 0 + 1);
});

test('walk: nesting stops at the engine depth', () => {
	// Groups 40 deep: the walk stops descending at depth 32, so the innermost line is never visited.
	let deep = line(0, 0);
	for (let i = 0; i < 40; i++) deep = group([deep]);
	let shallow = line(0, 10);
	for (let i = 0; i < WALK_MAX_DEPTH - 1; i++) shallow = group([shallow]);
	const json = docOf([deep, shallow, line(0, 20)]);
	assert.equal(pickAt(json, 0, 0), null);
	const r = pickAt(json, 0, 10);
	assert.equal(r.target.part.length, WALK_MAX_DEPTH);
	assert.equal(r.found.entity.id, r.hit.id);
	assert.deepEqual(pickAt(json, 0, 20).target.part, [2]);
	// Blocks nested deeper than the limit: the reference at depth 32 is visited, not expanded.
	const blocks = [];
	for (let i = 0; i < 40; i++) blocks.push(block('N' + i, [insert('N' + (i + 1), 0, 0), line(0, 5)]));
	blocks.push(block('N40', [line(0, 9)]));
	const nested = docOf([insert('N0', 0, 0)], blocks);
	assert.equal(walkCount(nested, 0), WALK_MAX_DEPTH + 1);
	const hit = pickAt(nested, 0, 5);
	assert.equal(hit.target.owner.length, 1);
	assert.equal(hit.found.entity.id, hit.hit.id);
});

test('walk: out-of-range counters and malformed input give null, not a throw', () => {
	const json = docOf([line(0, 0)]);
	assert.equal(locatePick(json, 0, 1), null);
	assert.equal(locatePick(json, 0, -1), null);
	assert.equal(locatePick(json, 0, 0.5), null);
	assert.equal(locatePick(json, 3, 0), null);
	assert.equal(locatePick({}, 0, 0), null);
	assert.deepEqual(locatePick({ models: [{ entities: [null, { kind: null }, line(0, 0)] }] }, 0, 0), { part: [2], owner: [2], blocks: [] });
	assert.equal(entityAt(json, 0, [0, 0]), null);
	assert.equal(entityAt(json, 0, []), null);
});

// ---------------------------------------------------------------------------------------------
// Every hit on a grid over real files maps to the entity the engine reported.

const PICK_GRID = 24;

function* drawingFiles() {
	for (const s of SAMPLES) {
		const p = join(site, 'public', s.path);
		if (existsSync(p)) yield [p, PICK_GRID];
	}
	const corpus = process.env.CADKIT_PUBLIC_CORPUS ?? join(site, '..', 'corpus', 'public');
	if (!existsSync(corpus)) return;
	const walk = function* (dir) {
		for (const n of readdirSync(dir).sort()) {
			const p = join(dir, n);
			if (statSync(p).isDirectory()) yield* walk(p);
			else if (/\.(dxf|dwg|dgn|gml)$/i.test(n)) yield [p, PICK_GRID / 2];
		}
	};
	yield* walk(corpus);
}

/** Grid picks through the worker core: hits, how many mapped to the reported entity, the first misses. */
function gridPicks(path, grid = PICK_GRID) {
	const core = createCore(api);
	let opened;
	try {
		opened = core.handle({ type: 'open', name: path, buffer: bufferOf(readFileSync(path)) }).value;
	} catch {
		return null; // Not a file cadkit reads; the reader tests cover that.
	}
	if (!opened.extras) return null;
	let hits = 0;
	let mapped = 0;
	/** Hits the old reading (the counter as a model index) would have shown right. */
	let asIndex = 0;
	const misses = [];
	opened.info.models.forEach((m, model) => {
		if (!m.entities) return;
		const r = core.handle({ type: 'render', model }).value;
		const [x, y, w, h] = /viewBox="([^"]*)"/.exec(r.svg)[1].split(' ').map(Number);
		const tolerance = Math.max(w, h) / 80;
		for (let i = 0; i <= grid; i++)
			for (let j = 0; j <= grid; j++) {
				// Back to drawing coordinates: the sheet is relative to its origin, Y flipped.
				const px = x + (w * i) / grid + r.origin.x;
				const py = -(y + (h * j) / grid + r.origin.y);
				let picked;
				try {
					picked = core.handle({ type: 'pick', model, x: px, y: py, tolerance, excludedLayers: [], excludedKinds: [] }).value;
				} catch {
					continue; // Work budget: the page reports it; nothing to map.
				}
				if (!picked) continue;
				hits++;
				const t = picked.target;
				const e = t ? core.handle({ type: 'entity', model, path: t.part, highlight: false, origin: r.origin }).value : null;
				const same = e && (picked.hit.id === null ? e.id === null && e.kind.replace(/_/g, '') === picked.hit.kind : e.id === picked.hit.id);
				if (same) mapped++;
				if (t && t.part.length === 1 && t.part[0] === picked.hit.index) asIndex++;
				else if (misses.length < 3) misses.push({ model, index: picked.hit.index, id: picked.hit.id, target: t });
			}
	});
	return { hits, mapped, asIndex, misses };
}

test('pick grid: every hit on every bundled sample and public corpus file maps to its entity', (t) => {
	let files = 0;
	let total = 0;
	for (const [path, grid] of drawingFiles()) {
		const r = gridPicks(path, grid);
		if (!r) continue;
		files++;
		total += r.hits;
		assert.equal(r.mapped, r.hits, `${path.slice(site.length - 5)}: ${JSON.stringify(r.misses)}`);
	}
	t.diagnostic(`${total} hits in ${files} files, all mapped`);
	assert.ok(files >= 4, 'the bundled samples are there');
	assert.ok(total > 1000, `${total} hits`);
});

test('pick grid: block references own their parts (site plan sample)', () => {
	const r = gridPicks(join(site, 'public/samples/cadkit-sample-site.dxf'));
	assert.ok(r.hits > 100);
	// Read as a model index, the counter names the wrong entity for most hits on this sheet.
	assert.ok(r.asIndex < r.hits / 2, `${r.asIndex} of ${r.hits}`);
	const core = createCore(api);
	core.handle({ type: 'open', name: 'site.dxf', buffer: bufferOf(readFileSync(join(site, 'public/samples/cadkit-sample-site.dxf'))) });
	const picked = core.handle({ type: 'pick', model: 0, x: 21, y: 15, tolerance: 0.1, excludedLayers: [], excludedKinds: [] }).value;
	assert.equal(picked.hit.kind, 'polyline');
	assert.deepEqual(picked.target.blocks, ['BAY']);
	const owner = core.handle({ type: 'entity', model: 0, path: picked.target.owner, highlight: false, origin: { x: 0, y: 0 } }).value;
	assert.equal(owner.kind, 'insert');
	assert.equal(owner.block, 'BAY');
	const part = core.handle({ type: 'entity', model: 0, path: picked.target.part, highlight: false, origin: { x: 0, y: 0 } }).value;
	assert.equal(part.id, picked.hit.id);
	assert.deepEqual(part.parents, ['insert of block BAY']);
});

// Client drawings, when present: structural facts only (hit and mapping counts).
const privateCorpus = process.env.CADKIT_PRIVATE_CORPUS ?? join(site, '..', 'corpus', 'private');
test('pick grid: private corpus', { skip: !existsSync(privateCorpus) }, (t) => {
	const files = [];
	const walk = (dir) => {
		for (const n of readdirSync(dir)) {
			const p = join(dir, n);
			if (statSync(p).isDirectory()) walk(p);
			else if (/\.(dxf|dwg|dgn)$/i.test(n)) files.push(p);
		}
	};
	walk(privateCorpus);
	// A spread sample keeps the run short: every 20th file.
	let hits = 0;
	let mapped = 0;
	for (let i = 0; i < files.length; i += 20) {
		const r = gridPicks(files[i], PICK_GRID / 2);
		if (!r) continue;
		hits += r.hits;
		mapped += r.mapped;
	}
	t.diagnostic(`${mapped} of ${hits} hits mapped`);
	assert.equal(mapped, hits);
});
