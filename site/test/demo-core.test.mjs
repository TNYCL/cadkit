// The demo's engine logic (src/demo/core.ts) run in Node against the Node build of cadkit-wasm:
// the same requests the page sends to its Web Worker, on the bundled samples.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createCore, toWire } from '../src/demo/core.ts';
import { MAX_JSON_ENTITIES } from '../src/demo/protocol.ts';
import { planLayout } from '../src/demo/scene2d.ts';

const load = createRequire(import.meta.url);
const site = fileURLToPath(new URL('..', import.meta.url));
const api = load('cadkit-wasm/node');
const sample = (name) => site + 'public/samples/' + name;
const bufferOf = (bytes) => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);

/** A core with a sample open. */
function opened(name) {
	const core = createCore(api);
	const bytes = readFileSync(sample(name));
	const doc = core.handle({ type: 'open', name, buffer: bufferOf(bytes) }).value;
	return { core, doc, calls: core.takeCalls() };
}
const kindOf = (fn) => {
	try {
		fn();
	} catch (e) {
		return toWire(e).kind;
	}
	return null;
};

test('open: facts, header, kinds and the calls that were made', () => {
	const { doc, calls } = opened('cadkit-sample-plan.dxf');
	assert.equal(doc.info.format, 'dxf');
	assert.equal(doc.header.codepage, 'ANSI_1252');
	assert.equal(doc.bytes, readFileSync(sample('cadkit-sample-plan.dxf')).length);
	assert.deepEqual(doc.kinds, { polyline: 27, line: 96, arc: 6, circle: 6, text: 26 });
	assert.deepEqual(doc.notDrawn, {});
	assert.deepEqual(doc.extras[0], { viewports: 0, surfaces: 0, shapes: 0, spatial: false, open3d: false });
	assert.deepEqual(doc.changes, { units: null, exploded: false });
	assert.equal(doc.model, 0);
	assert.deepEqual(calls.map((c) => c.call.replace(/\(.*/, '')), ['inspect', 'read', 'doc.toJSON']);
	for (const c of calls) {
		assert.ok(c.ms >= 0 && c.note, c.call);
		assert.ok(['read', 'inspect'].includes(c.task), c.call);
	}
});

test('requests before a document is open, and files that are not drawings', () => {
	const core = createCore(api);
	assert.equal(kindOf(() => core.handle({ type: 'render', model: 0 })), 'bad_argument');
	assert.equal(kindOf(() => core.handle({ type: 'check', op: 'inspect' })), 'bad_argument');
	const junk = new TextEncoder().encode('this is not a drawing');
	assert.equal(kindOf(() => core.handle({ type: 'open', name: 'x.dgn', buffer: bufferOf(junk) })), 'unknown_format');
	assert.ok(core.takeCalls().some((c) => c.note === 'throws unknown_format'));
	// A failed open keeps the document that was open before.
	const { core: c2 } = opened('cadkit-sample-plan.dxf');
	assert.equal(kindOf(() => c2.handle({ type: 'open', name: 'x.dxf', buffer: bufferOf(junk) })), 'unknown_format');
	assert.match(c2.handle({ type: 'render', model: 0 }).value.svg, /data-layer="WALLS"/);
	assert.equal(c2.handle({ type: 'check', op: 'inspect' }).value.header.format, 'dxf');
});

test('render, pick and the entity with its highlight', () => {
	const { core } = opened('cadkit-sample-plan.dxf');
	const r = core.handle({ type: 'render', model: 0 }).value;
	assert.equal(r.layout, null);
	assert.match(r.svg, /^<svg[^>]*viewBox=/);
	// Coordinates near zero keep their numbers: no local origin, nothing to report.
	assert.deepEqual([r.origin, r.frames, r.relocation], [{ x: 0, y: 0 }, null, null]);
	// The sheet is always rendered for the light background; the page restyles it for the dark one.
	assert.match(r.svg, /<rect[^>]*fill="#ffffff"/);
	const pick = (x, y, tolerance, excludedLayers = [], excludedKinds = []) => core.handle({ type: 'pick', model: 0, x, y, tolerance, excludedLayers, excludedKinds }).value;
	const { hit, target } = pick(0.6, 0, 0.05);
	assert.equal(hit.kind, 'polyline');
	assert.equal(hit.layer, 'WALLS');
	assert.deepEqual(target, { part: target.owner, owner: target.owner, blocks: [] });
	assert.equal(pick(0.6, 0, 0.05, ['WALLS']), null);
	assert.notEqual(pick(0.6, 0, 0.05, [], ['polyline'])?.hit.kind, 'polyline', 'excluded kinds are not picked');
	const e = core.handle({ type: 'entity', model: 0, path: target.owner, highlight: true, origin: r.origin }).value;
	assert.equal(e.id, hit.id);
	assert.equal(e.kind, 'polyline');
	assert.equal(e.layer, 'WALLS');
	assert.equal(e.block, null);
	assert.deepEqual(e.parents, []);
	assert.equal(JSON.parse(e.json).kind.type, 'polyline');
	assert.ok(!e.jsonTruncated);
	// The highlight is this entity alone, in the same coordinates as the sheet.
	assert.equal((e.highlight.match(/<path /g) ?? []).length, 1);
	assert.match(e.highlight, /<path d="M0 0L1\.2 0L1\.2 -0\.3L0 -0\.3L0 0Z"/);
	assert.equal(core.handle({ type: 'entity', model: 0, path: target.owner, highlight: false, origin: r.origin }).value.highlight, null);
	assert.equal(kindOf(() => core.handle({ type: 'entity', model: 0, path: [99999], highlight: false, origin: r.origin })), 'bad_argument');
});

test('block references: the entity is the reference, with its block and attributes', () => {
	const { core, doc } = opened('cadkit-sample-site.dxf');
	assert.equal(doc.layers.find((l) => l.name === 'SURVEY').visible, false);
	const r = core.handle({ type: 'render', model: 0 }).value;
	assert.match(r.svg, /data-layer="SURVEY"/, 'layers that are off in the file are rendered; the page hides them');
	const pick = (x, y, tolerance, excludedLayers = []) => core.handle({ type: 'pick', model: 0, x, y, tolerance, excludedLayers, excludedKinds: [] }).value;
	const { hit, target } = pick(21, 15, 0.1);
	assert.equal(hit.kind, 'polyline', 'the pick reports the part of the block that was hit');
	assert.deepEqual(target.blocks, ['BAY']);
	assert.equal(target.part.length, 2);
	const e = core.handle({ type: 'entity', model: 0, path: target.owner, highlight: true, origin: r.origin }).value;
	assert.equal(e.kind, 'insert');
	assert.equal(e.block, 'BAY');
	assert.deepEqual(e.attributes, [{ tag: 'NO', value: 'P03', invisible: false }]);
	assert.match(e.highlight, /<path /, 'the highlight draws the block content');
	// The part path names the entity inside the block that was hit.
	assert.equal(core.handle({ type: 'entity', model: 0, path: target.part, highlight: false, origin: r.origin }).value.id, hit.id);
	// A point on the layer that is off in the file can be picked once the page shows the layer.
	assert.equal(pick(2, 2, 0.2, ['SURVEY']), null);
	assert.equal(pick(2, 2, 0.2).hit.layer, 'SURVEY');
});

test('exports are read back and compared', () => {
	const { core } = opened('cadkit-sample-site.dxf');
	const dxf = core.handle({ type: 'export', format: 'dxf', version: 'r2000', codepage: 'windows-1254' });
	assert.equal(dxf.value.extension, 'dxf');
	assert.ok(dxf.value.data instanceof Uint8Array && dxf.value.bytes === dxf.value.data.byteLength);
	assert.deepEqual(dxf.transfer, [dxf.value.data.buffer]);
	assert.deepEqual(dxf.value.check.rows, [
		{ label: 'Entities', source: 89, back: 89 },
		{ label: 'Layers', source: 10, back: 10 },
		{ label: 'Blocks', source: 4, back: 4 },
	]);
	assert.equal(dxf.value.check.warnings, 0);
	assert.ok(core.takeCalls().some((c) => c.call === 'doc.toDxfBytes("r2000", "windows-1254")'));
	// A code page on a UTF-8 version is refused by cadkit, not silently ignored.
	assert.equal(kindOf(() => core.handle({ type: 'export', format: 'dxf', version: 'r2018', codepage: 'windows-1254' })), 'unsupported');
	assert.ok(core.handle({ type: 'export', format: 'dxf', version: 'r2018', codepage: '' }).value.bytes > 1000);
	const json = core.handle({ type: 'export', format: 'json' }).value;
	assert.equal(typeof json.data, 'string');
	assert.equal(JSON.parse(json.data).models[0].entities.length, 89);
	assert.ok(json.check.rows.every((r) => r.source === r.back));
	assert.equal(kindOf(() => core.handle({ type: 'export', format: 'citygml' })), 'unsupported');
});

test('changing the document: units, explode, reset', () => {
	const { core } = opened('cadkit-sample-site.dxf');
	const mm = core.handle({ type: 'transform', op: 'units', unit: 'millimeter' }).value;
	assert.equal(mm.doc.info.units, 'millimeter');
	assert.deepEqual(mm.doc.changes, { units: 'millimeter', exploded: false });
	assert.match(mm.note, /scaled by 1000/);
	assert.match(core.handle({ type: 'render', model: 0 }).value.svg, /viewBox="-?\d{3,}/, 'coordinates are in millimetres now');
	const ex = core.handle({ type: 'transform', op: 'explode' }).value;
	assert.deepEqual(ex.doc.changes, { units: 'millimeter', exploded: true });
	assert.ok(ex.doc.info.models[0].entities > 89);
	assert.equal(ex.doc.kinds.insert, undefined);
	const back = core.handle({ type: 'transform', op: 'reset' }).value;
	assert.equal(back.doc.info.units, 'meter');
	assert.equal(back.doc.info.models[0].entities, 89);
	assert.deepEqual(back.doc.changes, { units: null, exploded: false });
});

test('checks: header only, an object limit, a truncated file', () => {
	const { core } = opened('cadkit-sample-plan.dxf');
	const head = core.handle({ type: 'check', op: 'inspect' }).value;
	assert.equal(head.detected, 'dxf');
	assert.equal(head.header.version, 'AC1032');
	const low = core.handle({ type: 'check', op: 'limits', maxObjects: 10 }).value;
	assert.deepEqual([low.ok, low.kind], [false, 'limit_exceeded']);
	const high = core.handle({ type: 'check', op: 'limits', maxObjects: 100000 }).value;
	assert.deepEqual([high.ok, high.entities, high.layers], [true, 161, 7]);
	const cut = core.handle({ type: 'check', op: 'truncate', fraction: 0.5 }).value;
	assert.ok(cut.ok && cut.entities > 0 && cut.entities < 161);
	assert.equal(cut.warningCodes['dxf.truncated'], 1);
	assert.equal(cut.bytes, Math.floor(readFileSync(sample('cadkit-sample-plan.dxf')).length / 2));
	// None of the checks touched the open document.
	assert.equal(core.handle({ type: 'export', format: 'json' }).value.check.rows[0].source, 161);
	// Every fraction returns: a document or a typed error, never a throw.
	for (const fraction of [0, 0.05, 0.3, 0.95, 1]) {
		const r = core.handle({ type: 'check', op: 'truncate', fraction }).value;
		assert.ok(r.ok ? r.entities >= 0 : typeof r.kind === 'string', String(fraction));
	}
});

test('CityGML: 3D mesh, entity by face, validation, preflight and the native export', () => {
	const { core, doc } = opened('cadkit-sample-buildings.gml');
	assert.deepEqual(doc.extras[0], { viewports: 0, surfaces: 26, shapes: 0, spatial: true, open3d: true });
	assert.equal(doc.info.units, 'unitless');
	const m = core.handle({ type: 'mesh', model: 0 });
	assert.equal(m.value.faces.length, 13);
	assert.equal(m.transfer.length, 5);
	const roof = m.value.faces.find((f) => f.layer === 'RoofSurface');
	const e = core.handle({ type: 'entity', model: 0, path: roof.path, highlight: false, origin: { x: 0, y: 0 } }).value;
	assert.equal(e.kind, 'polygon');
	assert.equal(e.parents[0], 'House');
	assert.ok(e.props.some(([k, v]) => k === 'gml.id' && /roof/.test(v)));
	const v = core.handle({ type: 'gml', op: 'validate' }).value;
	assert.equal(v.counters.polygons, 13);
	const p = core.handle({ type: 'gml', op: 'preflight', profile: 'city-model-tender' }).value;
	assert.equal(p.acceptanceVerified, false);
	assert.equal(p.buildings, 2);
	assert.ok(p.issueCount > 0 && p.issues.length > 0 && p.issues.reduce((n, i) => n + i.count, 0) === p.issueCount);
	assert.ok(p.uncheckedRules.length > 0);
	assert.equal(kindOf(() => core.handle({ type: 'gml', op: 'preflight', profile: 'nope' })), 'invalid');
	const x = core.handle({ type: 'export', format: 'citygml' }).value;
	assert.equal(x.extension, 'gml');
	assert.ok(x.check.rows.length >= 5 && x.check.rows.every((r) => r.source === r.back));
	assert.equal(x.check.warnings, null);
	assert.equal(kindOf(() => core.handle({ type: 'transform', op: 'units', unit: 'foot' })), 'unsupported');
	// A drawing without surfaces has no mesh, and CityGML checks need a CityGML file.
	const { core: plan } = opened('cadkit-sample-plan.dxf');
	assert.equal(kindOf(() => plan.handle({ type: 'mesh', model: 0 })), 'unsupported');
	assert.equal(kindOf(() => plan.handle({ type: 'gml', op: 'validate' })), 'unsupported');
});

const dwg = 'third-party/acadsharp/sample_AC1032.dwg';
test('DWG test sheet: unknown entities are named, layouts come with their viewports', { skip: existsSync(sample(dwg)) ? false : 'not fetched yet (run the prebuild step)' }, () => {
	const { core, doc } = opened(dwg);
	assert.equal(doc.notDrawn['dwg.MULTILEADER'], 15);
	assert.equal(Object.values(doc.notDrawn).reduce((a, b) => a + b, 0), doc.kinds.unknown);
	assert.deepEqual(doc.extras.map((x) => x.viewports), [0, 2, 2, 2]);
	const r = core.handle({ type: 'render', model: 1 }).value;
	assert.equal(r.layout.source, 0);
	assert.equal(r.layout.viewports.length, 2);
	assert.match(r.layout.sourceSvg, /data-layer="0"/);
	// The first viewport is the sheet itself; the second is the window into the model.
	const plan = planLayout(r.layout.viewports, null);
	assert.equal(plan.frames.length, 1);
	assert.ok(plan.frames[0].scale > 1);
	// Model space itself is not a layout.
	assert.equal(core.handle({ type: 'render', model: 0 }).value.layout, null);
	// What cadkit could not draw is not written either, and the export says so.
	const x = core.handle({ type: 'export', format: 'dxf', version: 'r2018', codepage: '' }).value;
	const entities = x.check.rows.find((row) => row.label === 'Entities');
	assert.ok(entities.back < entities.source);
});

test('documents too large to keep as JSON still open, render and pick', () => {
	const n = MAX_JSON_ENTITIES + 1;
	const parts = ['0', 'SECTION', '2', 'ENTITIES'];
	for (let i = 0; i < n; i++) parts.push('0', 'POINT', '8', 'P', '10', String(i % 400), '20', String(Math.floor(i / 400)));
	parts.push('0', 'ENDSEC', '0', 'EOF', '');
	const core = createCore(api);
	const doc = core.handle({ type: 'open', name: 'points.dxf', buffer: bufferOf(new TextEncoder().encode(parts.join('\n'))) }).value;
	assert.equal(doc.info.models[0].entities, n);
	assert.equal(doc.kinds, null);
	assert.equal(doc.extras, null);
	assert.ok(!core.takeCalls().some((c) => c.call === 'doc.toJSON()'));
	assert.ok(core.handle({ type: 'render', model: 0 }).value.svg.length > n * 10);
	const picked = core.handle({ type: 'pick', model: 0, x: 7, y: 3, tolerance: 0.2, excludedLayers: [], excludedKinds: [] }).value;
	assert.equal(picked.hit.kind, 'point');
	assert.equal(picked.target, null, 'without the JSON copy there is nothing to map the pick to');
	assert.equal(kindOf(() => core.handle({ type: 'entity', model: 0, path: [0], highlight: true, origin: { x: 0, y: 0 } })), 'unsupported');
	assert.equal(kindOf(() => core.handle({ type: 'mesh', model: 0 })), 'unsupported');
});
