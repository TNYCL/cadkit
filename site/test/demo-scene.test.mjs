// Pure helpers of the /demo/ viewer that need no browser: sheet colours, point markers,
// paper-space layouts, the measure tool, triangulation, the 3D mesh and the orbit camera.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import * as C from '../src/demo/colors.ts';
import * as S from '../src/demo/scene2d.ts';
import * as M from '../src/demo/mesh.ts';
import * as K from '../src/demo/camera.ts';

const load = createRequire(import.meta.url);
const site = fileURLToPath(new URL('..', import.meta.url));
const close = (a, b, eps = 1e-9) => assert.ok(Math.abs(a - b) <= eps * Math.max(1, Math.abs(a), Math.abs(b)), `${a} != ${b}`);

// ---- colours --------------------------------------------------------------------------------

test('hex parsing and formatting', () => {
	assert.deepEqual(C.parseHex('#0a1B2c'), [10, 27, 44]);
	assert.deepEqual(C.parseHex('#fff'), [255, 255, 255]);
	for (const bad of ['none', 'url(#cadkit-hatch-1)', 'currentColor', '', null, '#12345']) assert.equal(C.parseHex(bad), null);
	assert.equal(C.toHex([255, 0.4, 300]), '#ff00ff');
});

test('contrast follows WCAG and cadkit\'s own SVG rule', () => {
	close(C.contrast([0, 0, 0], [255, 255, 255]), 21);
	// cadkit_core::svg::with_contrast: below 1.25:1 becomes the sheet foreground.
	assert.deepEqual(C.withContrast([255, 255, 255], [255, 255, 255]), [0, 0, 0]);
	assert.deepEqual(C.withContrast([250, 250, 250], [255, 255, 255]), [0, 0, 0]);
	assert.deepEqual(C.withContrast([0, 0, 0], [16, 20, 25]), [255, 255, 255]);
	assert.deepEqual(C.withContrast([0, 0, 255], [16, 20, 25]), [0, 0, 255]);
});

test('display colours: light sheet unchanged, dark sheet legible', () => {
	const bg = C.parseHex(C.SHEET_BG.dark);
	for (const c of ['#000000', '#0000ff', '#333333', '#ff0000', '#00ffff', '#808080', '#1747b8']) {
		assert.equal(C.displayColor(c, 'light'), c);
		const shown = C.parseHex(C.displayColor(c, 'dark'));
		assert.ok(C.contrast(shown, bg) >= C.DARK_MIN_CONTRAST - 0.02, `${c} -> ${C.toHex(shown)}`);
	}
	// Black strokes of the light render are the sheet foreground on dark.
	assert.equal(C.displayColor('#000000', 'dark'), '#ffffff');
	// Colours that are legible already are kept exactly.
	assert.equal(C.displayColor('#00ffff', 'dark'), '#00ffff');
	assert.equal(C.displayColor('#ff0000', 'dark'), '#ff0000');
	// Pure blue keeps its hue but is lightened.
	const blue = C.parseHex(C.displayColor('#0000ff', 'dark'));
	assert.ok(blue[2] === 255 && blue[0] === blue[1] && blue[0] > 0);
	// Non-colours pass through.
	assert.equal(C.displayColor('url(#cadkit-hatch-1)', 'dark'), 'url(#cadkit-hatch-1)');
	assert.equal(C.displayColor('none', 'dark'), 'none');
});

test('colour index palette and layer swatches', () => {
	assert.deepEqual(C.aciRgb(1, false), [255, 0, 0]);
	assert.deepEqual(C.aciRgb(7, false), [0, 0, 0]);
	assert.deepEqual(C.aciRgb(7, true), [255, 255, 255]);
	assert.deepEqual(C.aciRgb(10, false), [255, 0, 0]);
	assert.equal(C.aciRgb(0, false), null);
	assert.equal(C.aciRgb(256, false), null);
	for (let i = 1; i <= 255; i++) assert.ok(C.aciRgb(i, false).every((v) => v >= 0 && v <= 255), 'index ' + i);
	assert.equal(C.layerSwatch({ type: 'rgb', r: 17, g: 146, b: 238 }, false), '#1192ee');
	assert.equal(C.layerSwatch({ type: 'aci', index: 4 }, false), '#00ffff');
	assert.equal(C.layerSwatch({ type: 'by_layer' }, false), '');
});

// ---- point markers --------------------------------------------------------------------------

test('pointCross recognises the engine\'s point marker and nothing else', () => {
	assert.deepEqual(S.pointCross('M-6.204 -1.491L9.193 -1.491M1.494 -9.19L1.494 6.208'), { x: 1.494, y: -1.491 });
	assert.deepEqual(S.pointCross('M0 0L2 0M1 -1L1 1'), { x: 1, y: 0 });
	for (const d of [
		'M0 0L2 0', // a line
		'M0 0L2 0M1 -1L1 2', // unequal arms
		'M0 0L2 0M0.5 -1L0.5 1', // not crossing at the midpoints
		'M0 0L2 1M1 -1L1 1', // first segment not horizontal
		'M0 0L2 0M1 -1L1.5 1', // second segment not vertical
		'M0 0L2 0M1 -1L1 1Z',
		'M0 0L2 0L2 2',
	])
		assert.equal(S.pointCross(d), null, d);
});

test('the point marker shape is what cadkit-wasm emits for a POINT', () => {
	const { CadDocument } = load('cadkit-wasm/node');
	const entity = (kind) => ({ id: null, layer: '0', color: { type: 'by_layer' }, linetype: null, lineweight: { type: 'by_layer' }, visible: true, kind, attributes: [], props: {} });
	const doc = CadDocument.fromJsonString(
		JSON.stringify({
			schema_version: 1,
			source: { format: 'dxf', version: 'AC1032', application: null, codepage: null },
			units: { unit: 'unitless', meters_per_unit: null },
			layers: [],
			linetypes: [],
			text_styles: [],
			blocks: [],
			models: [
				{
					name: 'Model',
					id: null,
					kind: 'model',
					is_3d: false,
					entities: [
						entity({ type: 'point', position: { x: 40, y: 25, z: 0 } }),
						entity({ type: 'line', start: { x: 0, y: 0, z: 0 }, end: { x: 1000, y: 500, z: 0 } }),
					],
					props: {},
				},
			],
			props: {},
			warnings: [],
		})
	);
	try {
		const svg = doc.toSvg({ model: 0 });
		const paths = [...svg.matchAll(/<path d="([^"]+)"/g)].map((m) => m[1]);
		const centres = paths.map((d) => S.pointCross(d)).filter(Boolean);
		assert.equal(centres.length, 1, svg);
		close(centres[0].x, 40, 1e-3);
		close(centres[0].y, -25, 1e-3);
	} finally {
		doc.free();
	}
});

// ---- layouts --------------------------------------------------------------------------------

test('layout: the sheet viewport is skipped, windows map paper to model', () => {
	const sheet = { cx: 7.6, cy: 4, w: 24.4, h: 9, vcx: 7.6, vcy: 4, vh: 9 };
	const win = { cx: 5.25, cy: 4, w: 8.4, h: 6.4, vcx: 6, vcy: 4.5, vh: 9.6 };
	assert.ok(S.isSheetViewport(sheet));
	assert.ok(!S.isSheetViewport(win));
	const plan = S.planLayout([sheet, win], null);
	assert.equal(plan.frames.length, 1);
	const f = plan.frames[0];
	close(f.x, 1.05);
	close(f.y, -7.2);
	close(f.scale, 1.5);
	close(f.viewBox.w / f.viewBox.h, f.w / f.h);
	close(f.viewBox.x + f.viewBox.w / 2, 6);
	close(-(f.viewBox.y + f.viewBox.h / 2), 4.5);
	for (const k of ['x', 'y', 'w', 'h']) close(plan.home[k], f[k]);
	// The frame centre shows the view centre; outside the frame there is no model point.
	const hit = S.paperToModel(plan, 5.25, 4);
	close(hit.x, 6);
	close(hit.y, 4.5);
	close(hit.scale, 1.5);
	const corner = S.paperToModel(plan, 1.05, 0.8);
	close(corner.x, 6 - 6.3);
	close(corner.y, 4.5 - 4.8);
	assert.equal(S.paperToModel(plan, 20, 4), null);
});

test('layout: no model window gives null; own entities extend the home box', () => {
	const sheet = { cx: 0, cy: 0, w: 10, h: 5, vcx: 0, vcy: 0, vh: 5 };
	assert.equal(S.planLayout([sheet], null), null);
	assert.equal(S.planLayout([], null), null);
	assert.equal(S.planLayout([{ cx: 0, cy: 0, w: 0, h: 5, vcx: 0, vcy: 0, vh: 5 }], null), null);
	// A first viewport that does show the model (no sheet viewport in the file) is kept.
	const only = S.planLayout([{ cx: 0, cy: 0, w: 10, h: 5, vcx: 100, vcy: 50, vh: 50 }], { x: -8, y: -4, w: 2, h: 2 });
	assert.equal(only.frames.length, 1);
	assert.deepEqual(only.home, { x: -8, y: -4, w: 13, h: 6.5 });
});

// ---- measure --------------------------------------------------------------------------------

test('measure: areas stay exact far from zero', () => {
	const sq = [{ x: 0, y: 0 }, { x: 1.5, y: 0 }, { x: 1.5, y: 1.5 }, { x: 0, y: 1.5 }];
	const far = sq.map((q) => ({ x: q.x + 512345.125, y: q.y + 4087654.375 }));
	assert.equal(S.polygonArea(far), 2.25);
});

test('measure: length, area and formatting', () => {
	const sq = [
		{ x: 0, y: 0 },
		{ x: 4, y: 0 },
		{ x: 4, y: 3 },
		{ x: 0, y: 3 },
	];
	close(S.pathLength(sq), 11);
	close(S.polygonArea(sq), 12);
	close(S.polygonArea([...sq].reverse()), 12);
	assert.equal(S.polygonArea(sq.slice(0, 2)), 0);
	assert.equal(S.pathLength([]), 0);
	assert.equal(S.formatMeasure(12), '12');
	assert.equal(S.formatMeasure(12.3456), '12.346');
	assert.equal(S.formatMeasure(123.456), '123.46');
	assert.equal(S.formatMeasure(1250.04), '1,250');
	assert.equal(S.formatMeasure(0.0042), '0.0042');
	assert.equal(S.formatMeasure(0), '0');
	assert.equal(S.formatMeasure(NaN), '-');
});

// ---- triangulation --------------------------------------------------------------------------

const triArea = (pts, tris) => {
	let a = 0;
	for (let i = 0; i < tris.length; i += 3) {
		const [p, q, r] = [pts[tris[i]], pts[tris[i + 1]], pts[tris[i + 2]]];
		a += Math.abs((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])) / 2;
	}
	return a;
};

test('triangulate: convex, concave, either winding', () => {
	const square = [[0, 0], [2, 0], [2, 2], [0, 2]];
	for (const ring of [square, [...square].reverse()]) {
		const t = M.triangulate([ring]);
		assert.equal(t.length, 6);
		close(triArea(ring, t), 4);
	}
	const ell = [[0, 0], [3, 0], [3, 1], [1, 1], [1, 3], [0, 3]];
	const t = M.triangulate([ell]);
	assert.equal(t.length / 3, 4);
	close(triArea(ell, t), 5);
	// A gable wall (pentagon) as in the CityGML sample.
	const gable = [[0, 0], [8, 0], [8, 6], [4, 9], [0, 6]];
	close(triArea(gable, M.triangulate([gable])), 60);
});

test('triangulate: holes are cut out', () => {
	const outer = [[0, 0], [10, 0], [10, 10], [0, 10]];
	const h1 = [[2, 2], [4, 2], [4, 4], [2, 4]];
	const h2 = [[6, 6], [8, 6], [8, 8], [6, 8]];
	for (const holes of [[h1], [h1, h2], [[...h1].reverse(), h2]]) {
		const rings = [outer, ...holes];
		const pts = rings.flat();
		const t = M.triangulate(rings);
		close(triArea(pts, t), 100 - 4 * holes.length);
		for (const i of t) assert.ok(i >= 0 && i < pts.length);
	}
});

test('triangulate: degenerate input terminates', () => {
	assert.deepEqual(M.triangulate([]), []);
	assert.deepEqual(M.triangulate([[[0, 0], [1, 1]]]), []);
	assert.deepEqual(M.triangulate([[[0, 0], [1, 1], [2, 2]]]), []);
	// Repeated points and a collinear point.
	const ring = [[0, 0], [1, 0], [2, 0], [2, 2], [2, 2], [0, 2]];
	close(triArea(ring, M.triangulate([ring])), 4);
	// A bow-tie (self-intersecting) must not hang.
	assert.ok(M.triangulate([[[0, 0], [2, 2], [2, 0], [0, 2]]]).length % 3 === 0);
	// Many points.
	const circle = Array.from({ length: 500 }, (_, i) => [Math.cos((i / 500) * 2 * Math.PI), Math.sin((i / 500) * 2 * Math.PI)]);
	const t = M.triangulate([circle]);
	assert.equal(t.length / 3, 498);
	close(triArea(circle, t), Math.PI, 1e-3);
});

// ---- mesh -----------------------------------------------------------------------------------

const p = (x, y, z) => ({ x, y, z });
const poly = (layer, pts, extra = {}) => ({ layer, color: { type: 'by_layer' }, visible: true, kind: { type: 'polygon', exterior: pts, interiors: [] }, props: {}, ...extra });

test('mesh: faces, normals, layer ranges, duplicates and paths', () => {
	const floor = [p(0, 0, 0), p(4, 0, 0), p(4, 3, 0), p(0, 3, 0), p(0, 0, 0)];
	const wall = [p(0, 0, 0), p(4, 0, 0), p(4, 0, 2), p(0, 0, 2), p(0, 0, 0)];
	const doc = {
		layers: [
			{ name: 'Building', color: { type: 'rgb', r: 180, g: 186, b: 196 } },
			{ name: 'Ground', color: { type: 'rgb', r: 10, g: 20, b: 30 } },
			{ name: 'Wall', color: { type: 'aci', index: 7 } },
		],
		models: [
			{
				entities: [
					{
						layer: 'Building',
						kind: {
							type: 'group',
							children: [
								poly('Building', floor), // the solid's copy
								{ layer: 'Ground', kind: { type: 'group', children: [poly('Ground', floor)] } }, // the boundary surface: wins
								poly('Wall', wall),
								{ layer: 'Wall', visible: false, kind: { type: 'polygon', exterior: [p(0, 0, 0), p(1, 0, 0), p(1, 0, 9)], interiors: [] } },
							],
						},
					},
					{ layer: 'Wires', kind: { type: 'line', start: p(0, 0, 0), end: p(0, 0, 2) } },
				],
			},
		],
	};
	assert.deepEqual(M.surfaceStats(doc, 0), { surfaces: 3, spatial: true });
	// A hole whose nearest outline corner lies behind the hole itself.
	const ring = [[0, 0], [10, 0], [10, 10], [0, 10]];
	const hole = [[2, 4], [4, 4], [4, 2], [2, 2]];
	assert.equal(M.triangulate([ring, hole]).length / 3, 8);
	const mesh = M.buildMesh(doc, 0);
	assert.equal(mesh.duplicates, 1);
	assert.equal(mesh.faces.length, 2);
	assert.equal(mesh.faceOfTri.length, 4);
	assert.equal(mesh.positions.length, 4 * 9);
	assert.deepEqual(mesh.center, [2, 1.5, 1]);
	assert.deepEqual(mesh.layers.map((l) => l.name), ['Ground', 'Wall', 'Wires']);
	assert.deepEqual(mesh.layers.map((l) => l.tri), [[0, 2], [2, 2], [4, 0]]);
	assert.deepEqual(mesh.layers.map((l) => l.seg), [[0, 4], [4, 4], [8, 1]]);
	assert.deepEqual(mesh.faces[0], { path: [0, 1, 0], layer: 'Ground', kind: 'polygon' });
	assert.deepEqual(mesh.faces[1].path, [0, 2]);
	// Colours: the layer colour; index 7 (sheet foreground) becomes the neutral surface colour.
	assert.deepEqual([...mesh.colors.slice(0, 3)], [10, 20, 30]);
	assert.deepEqual([...mesh.colors.slice(18, 21)], [201, 204, 210]);
	// Normals are unit vectors along Z (floor) and Y (wall).
	assert.equal(Math.abs(mesh.normals[2]), 1);
	assert.equal(Math.abs(mesh.normals[19]), 1);
	// Path lookup.
	const hit = M.entityAt(doc, 0, [0, 2]);
	assert.equal(hit.entity.layer, 'Wall');
	assert.equal(hit.parents.length, 1);
	assert.equal(M.entityAt(doc, 0, [0, 9]), null);
	assert.equal(M.entityAt(doc, 0, [5]), null);
	// Picking: a ray straight down through the floor centre hits the floor.
	const face = M.pickFace(mesh.positions, mesh.faceOfTri, [0.5, 0.5, 10], [0, 0, -1]);
	assert.equal(mesh.faces[face].layer, 'Ground');
	assert.equal(M.pickFace(mesh.positions, mesh.faceOfTri, [50, 50, 10], [0, 0, -1]), -1);
	assert.equal(M.pickFace(mesh.positions, mesh.faceOfTri, [0.5, 0.5, 10], [0, 0, -1], () => false), -1);
});

test('mesh: flat drawings are not offered in 3D; models without surfaces give null', () => {
	const flat = { models: [{ entities: [{ layer: '0', kind: { type: 'face', points: [p(0, 0, 0), p(1, 0, 0), p(1, 1, 0)] } }] }] };
	assert.deepEqual(M.surfaceStats(flat, 0), { surfaces: 1, spatial: false });
	const none = { models: [{ entities: [{ layer: '0', kind: { type: 'line', start: p(0, 0, 0), end: p(1, 1, 1) } }] }] };
	assert.deepEqual(M.surfaceStats(none, 0), { surfaces: 0, spatial: false });
	assert.equal(M.buildMesh(none, 0), null);
	assert.equal(M.buildMesh({}, 0), null);
	assert.equal(M.buildMesh({ models: [{ entities: [{ kind: { type: 'polygon', exterior: 'x' } }, null, {}] }] }, 0), null);
});

/** A closed polyline (a DGN shape), optionally with bulges. */
const shape = (layer, pts, extra = {}) => ({
	layer,
	color: { type: 'by_layer' },
	visible: true,
	kind: { type: 'polyline', vertices: pts.map((q) => ({ position: q, bulge: 0, start_width: 0, end_width: 0 })), closed: true, normal: p(0, 0, 1) },
	props: {},
	...extra,
});
/** Walls and a slab of a w x d x h box, as closed polylines. */
const boxShapes = (w, d, h) => [
	shape('Slab', [p(0, 0, 0), p(w, 0, 0), p(w, d, 0), p(0, d, 0)]),
	shape('Wall', [p(0, 0, 0), p(w, 0, 0), p(w, 0, h), p(0, 0, h), p(0, 0, 0)]),
	shape('Wall', [p(w, 0, 0), p(w, d, 0), p(w, d, h), p(w, 0, h)]),
	shape('Wall', [p(w, d, 0), p(0, d, 0), p(0, d, h), p(w, d, h)]),
	shape('Wall', [p(0, d, 0), p(0, 0, 0), p(0, 0, h), p(0, d, h)]),
];

test('mesh: closed planar polylines are surfaces in a 3D model', () => {
	const doc = { models: [{ entities: [...boxShapes(10, 8, 3), shape('Slab', [p(0, 0, 3), p(10, 0, 3), p(10, 8, 3), p(0, 8, 3)])] }] };
	// Off (a 2D model): outlines only, as before.
	assert.deepEqual(M.surfaceStats(doc, 0), { surfaces: 0, spatial: false });
	assert.equal(M.buildMesh(doc, 0), null);
	const info = M.surfaceInfo(doc, 0, { shapes: true });
	assert.deepEqual(info, { surfaces: 6, shapes: 6, spatial: true, open3d: true });
	const mesh = M.buildMesh(doc, 0, { shapes: true });
	assert.equal(mesh.faces.length, 6);
	assert.equal(mesh.faceOfTri.length, 12);
	assert.deepEqual(mesh.faces[0], { path: [0], layer: 'Slab', kind: 'polyline' });
	// The outlines come from the faces, not a second time as wires: 6 faces x 4 edges.
	assert.equal(mesh.segments.length / 6, 24);
	// Per-face picking: a ray down through the middle hits the top slab first.
	const face = M.pickFace(mesh.positions, mesh.faceOfTri, [0, 0, 10], [0, 0, -1]);
	assert.deepEqual(mesh.faces[face].path, [5]);
	// Duplicates: the same outline twice is drawn once.
	const twice = { models: [{ entities: [...boxShapes(10, 8, 3), ...boxShapes(10, 8, 3)] }] };
	assert.equal(M.buildMesh(twice, 0, { shapes: true }).duplicates, 5);
});

test('mesh: bulged, open, non-planar and degenerate polylines stay wires', () => {
	const bulged = shape('A', [p(0, 0, 0), p(1, 0, 0), p(1, 1, 1)]);
	bulged.kind.vertices[1].bulge = 0.5;
	const open = { ...shape('A', [p(0, 0, 0), p(1, 0, 0), p(1, 1, 1)]) };
	open.kind = { ...open.kind, closed: false };
	const warped = shape('A', [p(0, 0, 0), p(10, 0, 0), p(10, 10, 0.5), p(0, 10, 0)]);
	const almost = shape('A', [p(0, 0, 0), p(10, 0, 0), p(10, 10, 0.005), p(0, 10, 0)]);
	const line = shape('A', [p(0, 0, 0), p(1, 1, 1), p(2, 2, 2)]);
	const two = shape('A', [p(0, 0, 0), p(1, 1, 1)]);
	for (const e of [bulged, open, warped, line, two]) assert.equal(M.surfaceInfo({ models: [{ entities: [e] }] }, 0, { shapes: true }).surfaces, 0, JSON.stringify(e.kind.vertices.map((v) => v.position)));
	// Within the tolerance (relative to its size) a polyline is planar.
	assert.equal(M.surfaceInfo({ models: [{ entities: [almost] }] }, 0, { shapes: true }).surfaces, 1);
	assert.ok(0.005 / 10 < M.SHAPE_PLANAR_TOLERANCE && 0.5 / 10 > M.SHAPE_PLANAR_TOLERANCE);
});

test('mesh: models of polylines only open in 3D when they are tall enough', () => {
	const tall = { models: [{ entities: boxShapes(10, 8, 1.2) }] };
	const flat = { models: [{ entities: boxShapes(10, 8, 1.1) }] };
	assert.ok(1.2 / 10 >= M.SHAPES_OPEN_3D_RATIO && 1.1 / 10 < M.SHAPES_OPEN_3D_RATIO);
	assert.deepEqual(M.surfaceInfo(tall, 0, { shapes: true }), { surfaces: 5, shapes: 5, spatial: true, open3d: true });
	// Flatter: offered in 3D, opened on the sheet.
	assert.deepEqual(M.surfaceInfo(flat, 0, { shapes: true }), { surfaces: 5, shapes: 5, spatial: true, open3d: false });
	// Exactly at the ratio, with coordinates whose difference does not come out exact in binary.
	const wall = (z0, z1) => ({ models: [{ entities: [shape('A', [p(0, 0, z0), p(3, 0, z0), p(3, 0, z1), p(0, 0, z1)])] }] });
	assert.ok(1.46 - 1.1 < 0.12 * 3, 'the float difference is just below the ratio');
	assert.equal(M.surfaceInfo(wall(1.1, 1.46), 0, { shapes: true }).open3d, true);
	assert.equal(M.surfaceInfo(wall(1.1, 1.459), 0, { shapes: true }).open3d, false);
	// A true surface keeps today's rule: any height opens in 3D.
	const mixed = { models: [{ entities: [...boxShapes(10, 8, 0.5), { layer: '0', kind: { type: 'face', points: [p(0, 0, 0), p(1, 0, 0), p(1, 0, 0.5)] } }] }] };
	assert.equal(M.surfaceInfo(mixed, 0, { shapes: true }).open3d, true);
	// All at one height: nothing to see in 3D.
	const plan = { models: [{ entities: [shape('A', [p(0, 0, 2), p(4, 0, 2), p(4, 4, 2)])] }] };
	assert.deepEqual(M.surfaceInfo(plan, 0, { shapes: true }), { surfaces: 1, shapes: 1, spatial: false, open3d: false });
});

test('mesh: the triangle budget holds for many shapes', () => {
	// 10,000 shapes (2,000 boxes) at map coordinates, as a district of DGN buildings arrives.
	const entities = [];
	for (let i = 0; i < 2000; i++) {
		const dx = 512000 + (i % 50) * 20;
		const dy = 4087000 + Math.floor(i / 50) * 20;
		entities.push(...boxShapes(10, 8, 3).map((e) => ({ ...e, kind: { ...e.kind, vertices: e.kind.vertices.map((v) => ({ ...v, position: p(v.position.x + dx, v.position.y + dy, v.position.z) })) } })));
	}
	const doc = { models: [{ entities }] };
	assert.deepEqual(M.surfaceInfo(doc, 0, { shapes: true }), { surfaces: 10000, shapes: 10000, spatial: true, open3d: false });
	const mesh = M.buildMesh(doc, 0, { shapes: true });
	assert.equal(mesh.faces.length, 10000);
	assert.equal(mesh.duplicates, 0);
	// Relative to the mesh centre: the float32 positions keep centimetres.
	assert.ok(mesh.positions.every((v) => Math.abs(v) < 1000));
	assert.ok(!mesh.truncated);
	assert.ok(mesh.faceOfTri.length <= M.MAX_MESH_TRIANGLES);
});

test('mesh: the CityGML sample gives 13 faces on three surface layers', () => {
	const { read } = load('cadkit-wasm/node');
	const doc = read(new Uint8Array(readFileSync(site + 'public/samples/cadkit-sample-buildings.gml')));
	try {
		const json = doc.toJSON();
		assert.deepEqual(M.surfaceStats(json, 0), { surfaces: 26, spatial: true });
		const mesh = M.buildMesh(json, 0);
		assert.equal(mesh.faces.length, 13);
		assert.equal(mesh.duplicates, 13);
		assert.deepEqual(mesh.layers.filter((l) => l.tri[1] > 0).map((l) => l.name), ['GroundSurface', 'RoofSurface', 'WallSurface']);
		assert.ok(!mesh.truncated);
		// Coordinates are relative to the centre, so f32 keeps centimetres at UTM magnitudes.
		assert.ok(Math.max(...mesh.max, ...mesh.min.map(Math.abs)) < 20);
		assert.ok(mesh.center[1] > 5_000_000);
		const hit = M.entityAt(json, 0, mesh.faces[0].path);
		assert.equal(hit.entity.kind.type, 'polygon');
		assert.equal(hit.parents[0].kind.type, 'group');
	} finally {
		doc.free();
	}
});

// ---- camera ---------------------------------------------------------------------------------

test('camera: basis is orthonormal and the target projects to the centre', () => {
	const cam = { target: [1, 2, 3], yaw: K.HOME_YAW, pitch: K.HOME_PITCH, distance: 20 };
	const { right, up, forward } = K.basis(cam);
	const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
	for (const v of [right, up, forward]) close(dot(v, v), 1);
	close(dot(right, up), 0, 1e-9);
	close(dot(right, forward), 0, 1e-9);
	close(dot(up, forward), 0, 1e-9);
	assert.ok(up[2] > 0, 'Z stays up');
	const m = K.viewProjection(cam, 1.5, 5);
	const c = K.project(m, cam.target);
	close(c[0], 0, 1e-6);
	close(c[1], 0, 1e-6);
	assert.ok(c[2] > -1 && c[2] < 1);
	// A point to the camera's right lands right of centre; one above lands above.
	assert.ok(K.project(m, [1 + right[0], 2 + right[1], 3 + right[2]])[0] > 0);
	assert.ok(K.project(m, [1, 2, 4])[1] > 0);
});

test('camera: rays invert the projection; fit, orbit, pan and dolly behave', () => {
	const cam = { target: [0, 0, 0], yaw: 0.7, pitch: 0.4, distance: 30 };
	const aspect = 16 / 9;
	const m = K.viewProjection(cam, aspect, 10);
	const point = [2, -3, 1.5];
	const ndc = K.project(m, point);
	const ray = K.screenRay(cam, aspect, ndc[0], ndc[1]);
	// The ray passes through the point.
	const t = [0, 1, 2].reduce((n, i) => n + (point[i] - ray.origin[i]) * ray.dir[i], 0);
	for (let i = 0; i < 3; i++) close(ray.origin[i] + ray.dir[i] * t, point[i], 1e-5);
	// A sphere that fits: its silhouette stays inside the viewport.
	for (const a of [0.5, 1, 2.5]) {
		const d = K.fitDistance(10, a);
		const half = Math.min(K.FOV_Y / 2, Math.atan(Math.tan(K.FOV_Y / 2) * a));
		assert.ok(Math.asin(10 / d) <= half + 1e-9);
	}
	// A flat, wide box is framed much closer than its bounding sphere, and still fits.
	const home = { target: [0, 0, 0], yaw: K.HOME_YAW, pitch: K.HOME_PITCH, distance: 1 };
	const [lo, hi] = [[-60, -55, -4], [60, 55, 4]];
	const d = K.fitBoxDistance(home, 1.5, lo, hi);
	const r = Math.hypot(120, 110, 8) / 2;
	assert.ok(d < K.fitDistance(r, 1.5) * 0.9, 'closer than the sphere fit');
	const vp = K.viewProjection({ ...home, distance: d }, 1.5, r);
	let extent = 0;
	for (const x of [lo[0], hi[0]]) for (const y of [lo[1], hi[1]]) for (const z of [lo[2], hi[2]]) {
		const n = K.project(vp, [x, y, z]);
		extent = Math.max(extent, Math.abs(n[0]), Math.abs(n[1]));
	}
	assert.ok(extent <= 0.8601 && extent > 0.8, 'corners reach the margin: ' + extent);
	assert.equal(K.clampPitch(5), K.PITCH_LIMIT);
	assert.equal(K.orbit(cam, 0, 1e6).pitch, K.PITCH_LIMIT);
	const panned = K.panCamera(cam, 100, 0, 600);
	close(Math.hypot(...panned.target), (100 * 2 * 30 * Math.tan(K.FOV_Y / 2)) / 600);
	assert.equal(K.dolly(cam, 2, 10).distance, 15);
	assert.equal(K.dolly(cam, 1e9, 10).distance, 0.2);
	assert.equal(K.dolly(cam, 1e-9, 10).distance, 600);
	assert.equal(K.gridStep(18), 2);
	assert.equal(K.gridStep(100), 10);
	assert.equal(K.gridStep(0.42), 0.05);
	assert.equal(K.gridStep(0), 1);
});
