// The local origin of the 2D sheet (src/demo/origin.ts): georeferenced drawings keep their
// relative geometry, every bundled sample can be moved without losing an element, anything the
// rewrite does not know leaves the SVG untouched, and picks and highlights still line up.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createCore } from '../src/demo/core.ts';
import { chooseOrigin, localizeSvg, MAX_ERROR_PX, REF_STAGE_PX } from '../src/demo/origin.ts';
import { SAMPLES } from '../src/demo/samples.ts';
import { MAX_ZOOM_IN } from '../src/demo/view.ts';

const load = createRequire(import.meta.url);
const site = fileURLToPath(new URL('..', import.meta.url));
const api = load('cadkit-wasm/node');
const bufferOf = (bytes) => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
const SVG_OPTIONS = { maxOutputBytes: 64 * 1024 * 1024, widthPx: 1600, strokePx: 1, background: '#ffffff', text: true, images: true, expandBlocks: true, includeHiddenLayers: true };

/** Every number in the attribute values (the namespace URL aside), in order. */
const numbersOf = (svg) =>
	[...svg.matchAll(/([\w:-]+)="([^"]*)"/g)]
		.filter((m) => m[1] !== 'xmlns')
		.flatMap((m) => m[2].match(/-?\d+(?:\.\d+)?/g) ?? [])
		.map(Number);
const elements = (svg) => (svg.match(/<[a-z]/g) ?? []).length;

/** A small R12 drawing (16 x 9 units) moved by (dx, dy): lines, a bulged polyline, arcs, rotated text, a point. */
function drawing(dx, dy) {
	const p = [];
	const pair = (c, v) => p.push(String(c), String(v));
	const xy = (cx, cy, x, y) => {
		pair(cx, x + dx);
		pair(cy, y + dy);
		pair(cx + 20, 0);
	};
	pair(0, 'SECTION');
	pair(2, 'ENTITIES');
	for (const [x0, y0, x1, y1] of [[0, 0, 16, 0], [16, 0, 16, 9], [16, 9, 0, 9], [0, 9, 0, 0], [4.25, 1.5, 4.25, 7.75]]) {
		pair(0, 'LINE');
		pair(8, 'WALLS');
		xy(10, 20, x0, y0);
		xy(11, 21, x1, y1);
	}
	pair(0, 'POLYLINE');
	pair(8, 'DOORS');
	pair(66, 1);
	xy(10, 20, 0, 0);
	pair(70, 1);
	for (const [x, y, b] of [[8, 2, 0], [11, 2, 0.4], [11, 5, 0], [8, 5, -0.25]]) {
		pair(0, 'VERTEX');
		pair(8, 'DOORS');
		xy(10, 20, x, y);
		pair(42, b);
	}
	pair(0, 'SEQEND');
	pair(0, 'CIRCLE');
	pair(8, 'GRID');
	xy(10, 20, 13.5, 7);
	pair(40, 0.75);
	pair(0, 'ARC');
	pair(8, 'GRID');
	xy(10, 20, 2, 6);
	pair(40, 1.25);
	pair(50, 10);
	pair(51, 135);
	pair(0, 'TEXT');
	pair(8, 'TEXT');
	xy(10, 20, 5.5, 3.25);
	pair(40, 0.35);
	pair(50, 30);
	pair(1, 'LIVING');
	pair(0, 'POINT');
	pair(8, 'GRID');
	xy(10, 20, 14.5, 1.5);
	pair(0, 'ENDSEC');
	pair(0, 'EOF');
	return new TextEncoder().encode(p.join('\n') + '\n');
}

const DX = 512345;
const DY = 4087654;
function opened(bytes) {
	const core = createCore(api);
	core.handle({ type: 'open', name: 'plan.dxf', buffer: bufferOf(bytes) });
	return core;
}

test('chooseOrigin: plans near zero stay; georeferenced sheets move to a round integer origin', () => {
	assert.deepEqual(chooseOrigin({ x: -0.5, y: -9.5, w: 13, h: 10 }), { x: 0, y: 0 });
	assert.deepEqual(chooseOrigin({ x: -35.5, y: -148.2, w: 1924.7, h: 751.4 }), { x: 0, y: 0 }, 'a wide sheet around zero gains nothing');
	const vb = { x: DX - 0.68, y: -DY - 9.32, w: 17.36, h: 10.64 };
	const o = chooseOrigin(vb);
	assert.ok(Number.isInteger(o.x) && Number.isInteger(o.y));
	assert.deepEqual(o, { x: 512340, y: -4087670 });
	// After the move the numbers are below twice the extent.
	assert.ok(Math.max(Math.abs(vb.x - o.x), Math.abs(vb.x + vb.w - o.x), Math.abs(vb.y - o.y), Math.abs(vb.y + vb.h - o.y)) < 2 * vb.w);
	// The threshold: float32 rounding at these magnitudes, in pixels at the deepest zoom.
	const pixel = 17.36 / (REF_STAGE_PX * MAX_ZOOM_IN);
	assert.ok((DY * 2 ** -24) / pixel > MAX_ERROR_PX);
	assert.deepEqual(chooseOrigin({ x: 0, y: 0, w: 0, h: 0 }), { x: 0, y: 0 });
	assert.deepEqual(chooseOrigin({ x: Number.NaN, y: 0, w: 1, h: 1 }), { x: 0, y: 0 });
});

test('chooseOrigin: what the two conditions decide in practice', () => {
	// A 20 km map at small coordinates: moving cannot shrink its numbers enough to matter.
	assert.deepEqual(chooseOrigin({ x: 1000, y: -21000, w: 20000, h: 20000 }), { x: 0, y: 0 });
	assert.deepEqual(chooseOrigin({ x: 30000, y: -52000, w: 20000, h: 20000 }), { x: 0, y: 0 });
	// A drawing centred on zero is as small as it gets.
	assert.deepEqual(chooseOrigin({ x: -10000, y: -10000, w: 20000, h: 20000 }), { x: 0, y: 0 });
	assert.deepEqual(chooseOrigin({ x: -2.5, y: -1, w: 5, h: 2 }), { x: 0, y: 0 });
	// A 5 m detail at x = 6,000,000: an integer origin, local numbers below twice the extent.
	const vb = { x: 6_000_000.25, y: -1_250_002.5, w: 5, h: 2 };
	const o = chooseOrigin(vb);
	assert.ok(Number.isInteger(o.x) && Number.isInteger(o.y));
	assert.ok(Math.abs(vb.x - o.x) < 10 && Math.abs(vb.y - o.y) < 10, JSON.stringify(o));
	// The precision test can never pass at MAX_ZOOM_IN: a pixel at that zoom is finer than
	// float32 resolves even half an extent from zero, so the fourfold test decides alone.
	const pixelOfExtent = 1 / (REF_STAGE_PX * MAX_ZOOM_IN);
	assert.ok((0.5 * 2 ** -24) / pixelOfExtent > MAX_ERROR_PX);
	let seed = 7;
	const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
	for (let i = 0; i < 5000; i++) {
		const e = 10 ** (rnd() * 10 - 3);
		const box = { x: (rnd() - 0.5) * 10 ** (rnd() * 8), y: (rnd() - 0.5) * 10 ** (rnd() * 8), w: e, h: e * (0.01 + rnd()) };
		const grid = Math.max(1, 10 ** Math.floor(Math.log10(Math.max(box.w, box.h))));
		const g = { x: Math.floor(box.x / grid) * grid, y: Math.floor(box.y / grid) * grid };
		const reach = (ox, oy) => Math.max(Math.abs(box.x - ox), Math.abs(box.x + box.w - ox), Math.abs(box.y - oy), Math.abs(box.y + box.h - oy));
		const moves = reach(0, 0) > 4 * reach(g.x, g.y);
		assert.deepEqual(chooseOrigin(box), moves ? g : { x: 0, y: 0 }, JSON.stringify(box));
	}
});

test('numbers the plain notation cannot carry, and odd arcs, leave the SVG untouched', () => {
	const head = '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="512340 -4087670 20 20">';
	const path = (d) => head + '<g data-layer="A"><path d="' + d + '"/></g></svg>';
	for (const bad of [
		// toFixed takes at most 100 decimals: this threw a RangeError out of the worker's render.
		path('M512341.' + '1'.repeat(120) + ' -4087661L512342 -4087660'),
		// 1e21 and more come back from toFixed with an exponent.
		path('M1' + '0'.repeat(22) + ' -4087661L512342 -4087660'),
		path('M512341 -4087661L5.12342e5 -4087660'),
		path('M+512341 -4087661L512342 -4087660'),
		path('M.5 -4087661L512342 -4087660'),
		path('M512341. -4087661L512342 -4087660'),
		path('M512341,-4087661L512342 -4087660'),
		path('M512341  -4087661L512342 -4087660'),
		path('M512341 -4087661A1 1 0 2 0 512342 -4087660'),
		path('M512341 -4087661A1 1 0 1 512342 -4087660'),
		path('M512341 -4087661A1 1 0 01 512342 -4087660'),
		head + '<g data-layer="A"><text x="512341" y="-4087661" transform="rotate(30)">a</text></g></svg>',
		head + '<g data-layer="A"><text x="512341" y="-4087661" transform="rotate(3e1 512341 -4087661)">a</text></g></svg>',
		head + '<g data-layer="A"><text x="512341 512342" y="-4087661">a</text></g></svg>',
		head + '<defs><pattern id="p" x="1" patternUnits="userSpaceOnUse" width="1" height="1"></pattern></defs></svg>',
		head + '<g data-layer="A"><tspan x="512341">a</tspan></g></svg>',
	]) {
		let r;
		assert.doesNotThrow(() => (r = localizeSvg(bad)), bad.slice(0, 160));
		assert.equal(r.svg, bad, bad.slice(0, 160));
		assert.deepEqual(r.origin, { x: 0, y: 0 });
		assert.ok(r.problem, bad.slice(0, 160));
	}
	// What the engine does write is moved exactly: arc radii and flags stay, end points move;
	// negative zero and trailing zeros come out plain; tspan x/y move and dy stays.
	const ok = localizeSvg(
		head +
			'<g data-layer="A"><path d="M512341 -4087661A1.5 0.75 30 1 0 512342.50 -4087660.000L-0 -0.0Z"/>' +
			'<text x="512345.125" y="-4087665" transform="rotate(-30 512345.125 -4087665)">a<tspan x="512345.125" dy="1.2">b</tspan></text></g></svg>'
	);
	assert.equal(ok.problem, null);
	assert.match(ok.svg, /d="M1 9A1\.5 0\.75 30 1 0 2\.5 10L-512340 4087670Z"/);
	assert.match(ok.svg, /<text x="5\.125" y="5" transform="rotate\(-30 5\.125 5\)">a<tspan x="5\.125" dy="1\.2">b<\/tspan><\/text>/);
});

test('a georeferenced drawing renders with the same relative geometry as the unshifted one', () => {
	const plain = opened(drawing(0, 0)).handle({ type: 'render', model: 0 }).value;
	const geo = opened(drawing(DX, DY)).handle({ type: 'render', model: 0 }).value;
	assert.deepEqual(plain.origin, { x: 0, y: 0 });
	assert.notDeepEqual(geo.origin, { x: 0, y: 0 });
	assert.equal(geo.relocation, null);
	// The plain sheet moved by the same amount relative to its own coordinates is the georeferenced one.
	const same = localizeSvg(plain.svg, { x: geo.origin.x - DX, y: geo.origin.y + DY });
	assert.equal(same.problem, null);
	assert.equal(elements(same.svg), elements(geo.svg));
	const a = numbersOf(same.svg);
	const b = numbersOf(geo.svg);
	assert.equal(a.length, b.length);
	// The engine writes 5 decimals for a 16-unit drawing; allow the last one to round differently.
	for (let i = 0; i < a.length; i++) assert.ok(Math.abs(a[i] - b[i]) <= 2e-5, `number ${i}: ${a[i]} vs ${b[i]}`);
	// And every number is now small enough for float32 to resolve far below a pixel.
	assert.ok(b.every((n) => Math.abs(n) < 2000), 'no absolute coordinates left');
	assert.ok(!/512\d{3}|408\d{4}/.test(geo.svg.replace(/^<svg[^>]*>/, '')));
});

test('picks and highlights line up on a moved sheet', () => {
	const core = opened(drawing(DX, DY));
	const r = core.handle({ type: 'render', model: 0 }).value;
	// The interior wall at x = 4.25, picked in drawing coordinates.
	const picked = core.handle({ type: 'pick', model: 0, x: DX + 4.25, y: DY + 5, tolerance: 0.05, excludedLayers: [], excludedKinds: [] }).value;
	assert.equal(picked.hit.kind, 'line');
	assert.deepEqual(picked.hit.bbox.min.slice(0, 2), [DX + 4.25, DY + 1.5]);
	const e = core.handle({ type: 'entity', model: 0, path: picked.target.owner, highlight: true, origin: r.origin }).value;
	const d = /<path d="([^"]*)"/.exec(e.highlight)[1];
	// The highlight is in the sheet's coordinates: the same segment as the sheet draws.
	const [x0, y0, x1, y1] = d.match(/-?[\d.]+/g).map(Number);
	assert.deepEqual([x0, y0, x1, y1], [DX + 4.25 - r.origin.x, -(DY + 1.5) - r.origin.y, DX + 4.25 - r.origin.x, -(DY + 7.75) - r.origin.y]);
	assert.ok(r.svg.includes(`M${x0} ${y0}L${x1} ${y1}`));
	// Without an origin the highlight stays absolute, as the engine drew it.
	const abs = core.handle({ type: 'entity', model: 0, path: picked.target.owner, highlight: true, origin: { x: 0, y: 0 } }).value;
	assert.match(abs.highlight, /M512349\.25 -4087655\.5/);
});

test('every bundled sample can be moved and keeps every element', () => {
	let models = 0;
	for (const s of SAMPLES) {
		const path = join(site, 'public', s.path);
		if (!existsSync(path)) continue;
		const d = api.read(new Uint8Array(readFileSync(path)), { maxInputBytes: 64 * 1024 * 1024 });
		try {
			d.info().models.forEach((_, model) => {
				const svg = d.toSvgBounded({ ...SVG_OPTIONS, model });
				const moved = localizeSvg(svg, { x: 1000, y: -2000 });
				assert.equal(moved.problem, null, `${s.id} #${model}`);
				assert.equal(elements(moved.svg), elements(svg), `${s.id} #${model}`);
				const vb = (t) => /viewBox="([^"]*)"/.exec(t)[1].split(' ').map(Number);
				const [x, y, w, h] = vb(svg);
				const [mx, my, mw, mh] = vb(moved.svg);
				assert.ok(Math.abs(mx - (x - 1000)) < 1e-9 && Math.abs(my - (y + 2000)) < 1e-9 && mw === w && mh === h, s.id);
				// The automatic choice never fails on what the engine emits.
				assert.equal(localizeSvg(svg).problem, null, s.id);
				models++;
			});
		} finally {
			d.free();
		}
	}
	assert.ok(models >= 6);
});

test('the georeferenced CityGML samples are moved, the others are not', () => {
	const originOf = (name) => {
		const core = createCore(api);
		core.handle({ type: 'open', name, buffer: bufferOf(readFileSync(join(site, 'public/samples', name))) });
		return core.handle({ type: 'render', model: 0 }).value.origin;
	};
	assert.deepEqual(originOf('cadkit-sample-plan.dxf'), { x: 0, y: 0 });
	assert.deepEqual(originOf('cadkit-sample-site.dxf'), { x: 0, y: 0 });
	const o = originOf('cadkit-sample-buildings.gml');
	assert.ok(o.x > 400000 && o.y < -5000000, JSON.stringify(o));
});

test('markup the engine does not emit leaves the SVG untouched, never half-rewritten', () => {
	const head = '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="512340 -4087670 20 20">';
	const ok = head + '<g data-layer="A" fill="none" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"><path d="M512341 -4087661L512342.5 -4087660" stroke="#000000"/></g></svg>';
	const moved = localizeSvg(ok);
	assert.equal(moved.problem, null);
	assert.match(moved.svg, /viewBox="0 0 20 20"/);
	assert.match(moved.svg, /d="M1 9L2\.5 10"/);
	for (const bad of [
		head + '<g data-layer="A"><path d="M512341 -4087661C1 2 3 4 5 6"/></g></svg>',
		head + '<g data-layer="A"><path d="m512341 -4087661l1 1"/></g></svg>',
		head + '<g data-layer="A"><circle cx="512341" cy="-4087661" r="1"/></g></svg>',
		head + '<g data-layer="A"><text x="512341" y="-4087661" transform="matrix(1 0 0 1 0 0)">a</text></g></svg>',
		head + '<g data-layer="A"><path d="M512341 -4087661L512342 -4087660" transform="translate(1 1)"/></g></svg>',
		head + '<g data-layer="A"><path d="M5.1e5 -4087661L512342 -4087660"/></g></svg>',
		head + '<line x1="512341" y1="0" x2="512342" y2="0"/></svg>',
		head + '<!-- note --><g data-layer="A"></g></svg>',
		head + '<g data-layer="A"><path d="M512341 -4087661L512342 -4087660"/>',
	]) {
		const r = localizeSvg(bad);
		assert.equal(r.svg, bad, bad);
		assert.deepEqual(r.origin, { x: 0, y: 0 });
		assert.ok(r.problem, bad);
	}
	// Small coordinates need no move, so nothing is reported for them.
	assert.equal(localizeSvg('<svg viewBox="0 0 1 1"><circle/></svg>').problem, null);
});

test('hatch patterns keep their phase against the absolute grid', () => {
	const o = { x: 512340, y: -4087670 };
	const svg =
		'<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="512340 -4087670 20 20"><defs>' +
		'<pattern id="cadkit-hatch-1" patternUnits="userSpaceOnUse" width="1.5" height="1.25" patternTransform="rotate(-45)"><line x1="0" y1="0" x2="1.5" y2="0" stroke="#808080" stroke-width="0.75" vector-effect="non-scaling-stroke"/></pattern>' +
		'</defs></svg>';
	const r = localizeSvg(svg, o);
	assert.equal(r.problem, null);
	const m = /<pattern [^>]* x="([^"]*)" y="([^"]*)">/.exec(r.svg);
	assert.ok(m, r.svg);
	assert.match(r.svg, /<line x1="0" y1="0" x2="1\.5" y2="0"/, 'pattern lines are pattern-local');
	// A tile corner (x, y) in pattern space, rotated into user space, must be the old absolute
	// origin (-o in local coordinates) up to whole tiles.
	const [px, py] = [Number(m[1]), Number(m[2])];
	const t = (-45 * Math.PI) / 180;
	const ux = Math.cos(t) * px - Math.sin(t) * py + o.x;
	const uy = Math.sin(t) * px + Math.cos(t) * py + o.y;
	// Back into pattern space: whole multiples of the tile size.
	const qx = Math.cos(-t) * ux - Math.sin(-t) * uy;
	const qy = Math.sin(-t) * ux + Math.cos(-t) * uy;
	const frac = (v, s) => Math.abs(v / s - Math.round(v / s));
	assert.ok(frac(qx, 1.5) < 1e-6 && frac(qy, 1.25) < 1e-6, `${qx} ${qy}`);
});

test('raster image frames: outside the viewBox, counted and measured', () => {
	const core = createCore(api);
	const json = {
		schema_version: 1,
		source: { format: 'dxf', version: 'AC1032', application: null, codepage: null },
		units: { unit: 'meter', meters_per_unit: 1 },
		layers: [],
		linetypes: [],
		text_styles: [],
		blocks: [],
		models: [
			{
				name: 'Model',
				id: 1,
				kind: 'model',
				is_3d: false,
				entities: [
					{ id: 2, layer: '0', color: { type: 'by_layer' }, linetype: null, lineweight: { type: 'by_layer' }, visible: true, kind: { type: 'line', start: { x: 0, y: 0, z: 0 }, end: { x: 10, y: 5, z: 0 } }, attributes: [], props: {} },
					{
						id: 3,
						layer: '0',
						color: { type: 'by_layer' },
						linetype: null,
						lineweight: { type: 'by_layer' },
						visible: true,
						kind: { type: 'image', path: 'scan.tif', position: { x: -50, y: -40, z: 0 }, u_vector: { x: 120, y: 0, z: 0 }, v_vector: { x: 0, y: 90, z: 0 }, size_px: [1200, 900] },
						attributes: [],
						props: {},
					},
				],
				props: {},
			},
		],
		props: {},
		warnings: [],
	};
	const doc = api.CadDocument.fromJsonString(JSON.stringify(json));
	const dxf = doc.toDxfBytes('r2018', null);
	doc.free();
	core.handle({ type: 'open', name: 'raster.dxf', buffer: bufferOf(dxf) });
	const r = core.handle({ type: 'render', model: 0 }).value;
	assert.match(r.svg, /<g data-kind="images"/);
	const [x, y, w, h] = /viewBox="([^"]*)"/.exec(r.svg)[1].split(' ').map(Number);
	// cadkit-wasm 0.5 leaves the frame out of the viewBox: a fit to it ignores the frame.
	assert.ok(w < 20 && h < 10, `viewBox ${w} x ${h}`);
	assert.deepEqual(r.frames, { x: -50, y: -50, w: 120, h: 90 });
	assert.ok(r.frames.x < x && r.frames.y < y);
	assert.equal(localizeSvg(r.svg).imageFrames, 1);
	// Hidden frames cannot be picked.
	const at = { type: 'pick', model: 0, x: -50, y: 0, tolerance: 0.5, excludedLayers: [] };
	assert.equal(core.handle({ ...at, excludedKinds: [] }).value.hit.kind, 'image');
	assert.equal(core.handle({ ...at, excludedKinds: ['image'] }).value, null);
});
