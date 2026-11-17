#!/usr/bin/env node
// Generates the synthetic site plan used by the demo: public/samples/cadkit-sample-site.dxf.
//
// The drawing is our own work (MIT OR Apache-2.0): a fictional plot with an office building, a
// parking court, trees, a pond and utility lines. No real place, owner or project. It exists to
// show what the apartment plan does not: block references with attributes, hatches with a hole,
// dash patterns, lineweights, bulged polylines, an ellipse, a spline, a leader, multi-line text
// and a layer that is switched off in the file.
//
// The script is deterministic: it builds cadkit's format-neutral document as JSON, loads it with
// cadkit-wasm and writes the file with cadkit's own DXF writer (R2018). Running it twice gives
// identical bytes.
//
// Usage: node scripts/samples/siteplan.mjs [--check]
//   --check  exit 1 if the committed file differs from a fresh build (used by npm test).

import { createRequire } from 'node:module';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
export const SAMPLE_PATH = join(here, '..', '..', 'public', 'samples', 'cadkit-sample-site.dxf');

const r = (n) => Math.round(n * 10000) / 10000;
const P = (x, y) => ({ x: r(x), y: r(y), z: 0 });
const UP = { x: 0, y: 0, z: 1 };
const BY_LAYER = { type: 'by_layer' };
const mm = (value) => ({ type: 'millimeters', value });

/** An entity of the neutral document on a layer. */
const ent = (layer, kind, extra = {}) => ({ id: null, layer, color: BY_LAYER, linetype: null, lineweight: BY_LAYER, visible: true, kind, attributes: [], props: {}, ...extra });
const line = (layer, x1, y1, x2, y2) => ent(layer, { type: 'line', start: P(x1, y1), end: P(x2, y2) });
/** Polyline from [x, y, bulge?] points. */
const poly = (layer, pts, closed = false, extra = {}) =>
	ent(layer, { type: 'polyline', vertices: pts.map(([x, y, bulge = 0]) => ({ position: P(x, y), bulge, start_width: 0, end_width: 0 })), closed, normal: UP }, extra);
const circle = (layer, x, y, radius) => ent(layer, { type: 'circle', center: P(x, y), radius, normal: UP });
const text = (layer, x, y, height, value, { rotation = 0, halign = 'center', valign = 'middle' } = {}) =>
	ent(layer, { type: 'text', position: P(x, y), end_point: null, height, rotation, width_factor: 1, oblique: 0, value, style: null, halign, valign, normal: UP });
const loop = (pts, external) => ({ edges: [{ type: 'polyline', vertices: pts.map(([x, y]) => ({ position: P(x, y), bulge: 0, start_width: 0, end_width: 0 })), closed: true }], external });
const attribute = (tag, value, x, y, height) => ({
	tag,
	value,
	set: null,
	position: P(x, y),
	invisible: false,
	layer: null,
	color: null,
	lineweight: null,
	display: { height, width: 1, halign: 'center', valign: 'middle', rotation: 0, style: null },
	props: {},
});
const insert = (layer, block, x, y, { scale = 1, rotation = 0, attributes = [] } = {}) =>
	ent(layer, { type: 'insert', block, position: P(x, y), scale: { x: scale, y: scale, z: scale }, rotation, normal: UP, columns: 1, rows: 1, column_spacing: 0, row_spacing: 0 }, { attributes });

const LAYERS = [
	// name, colour, linetype, lineweight (mm), visible
	['0', { type: 'aci', index: 7 }, 'CONTINUOUS', null, true],
	['BOUNDARY', { type: 'aci', index: 1 }, 'DASHDOT', 0.5, true],
	['BUILDING', { type: 'aci', index: 7 }, 'CONTINUOUS', 0.5, true],
	['BUILDING-HATCH', { type: 'aci', index: 8 }, 'CONTINUOUS', null, true],
	['ROAD', { type: 'aci', index: 8 }, 'CONTINUOUS', 0.35, true],
	['PARKING', { type: 'aci', index: 4 }, 'CONTINUOUS', null, true],
	['LANDSCAPE', { type: 'aci', index: 3 }, 'CONTINUOUS', null, true],
	['UTILITIES', { type: 'aci', index: 6 }, 'DASHED', 0.35, true],
	['TEXT', { type: 'aci', index: 7 }, 'CONTINUOUS', null, true],
	// Spot levels of the survey: switched off in the file, as a drawing office would leave them.
	['SURVEY', { type: 'aci', index: 30 }, 'CONTINUOUS', null, false],
];

const BLOCKS = [
	{
		name: 'TREE',
		description: 'Tree symbol, crown radius 1 m',
		entities: [
			circle('LANDSCAPE', 0, 0, 1),
			circle('LANDSCAPE', 0, 0, 0.12),
			...[0, 60, 120].map((deg) => {
				const a = (deg * Math.PI) / 180;
				return line('LANDSCAPE', -0.7 * Math.cos(a), -0.7 * Math.sin(a), 0.7 * Math.cos(a), 0.7 * Math.sin(a));
			}),
		],
	},
	{
		name: 'BAY',
		description: 'Parking bay 2.5 m x 5 m, open towards the aisle',
		entities: [poly('PARKING', [[0, 0], [0, 5], [2.5, 5], [2.5, 0]])],
	},
	{
		name: 'MANHOLE',
		description: 'Manhole, 0.9 m cover',
		entities: [circle('UTILITIES', 0, 0, 0.45), line('UTILITIES', -0.45, 0, 0.45, 0), line('UTILITIES', 0, -0.45, 0, 0.45)].map((e) => ({ ...e, linetype: 'CONTINUOUS' })),
	},
	{
		name: 'NORTH',
		description: 'North arrow',
		entities: [poly('TEXT', [[0, 2.2], [0.8, -1], [0, -0.4], [-0.8, -1]], true), line('TEXT', 0, -0.4, 0, 2.2), text('TEXT', 0, 3, 0.9, 'N')],
	},
];

/** Small deterministic generator for the tree positions and sizes. */
function lcg(seed) {
	let s = seed >>> 0;
	return () => {
		s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
		return s / 4294967296;
	};
}

function entities() {
	const out = [];
	// Plot boundary: an irregular pentagon, 64 m x 44 m.
	const plot = [[0, 0], [64, 0], [64, 38], [46, 44], [0, 44]];
	out.push(poly('BOUNDARY', plot, true));

	// Access road from the south edge to the parking court, with rounded kerbs (bulged corners).
	const b = Math.tan(Math.PI / 8); // quarter-circle bulge
	out.push(
		poly('ROAD', [[40, 0], [40, 9, b], [37, 12], [15, 12, -b], [12, 15], [12, 27, -b], [15, 30], [43, 30, -b], [46, 27], [46, 0]], false, { lineweight: mm(0.35) })
	);
	out.push(poly('ROAD', [[43, 0], [43, 10.5]], false, { linetype: 'DASHED' }));

	// Office building: an L with a courtyard, hatched; the courtyard is a hole in the hatch.
	const building = [[18, 32], [44, 32], [44, 41], [30, 41], [30, 36.5], [18, 36.5]];
	const yard = [[34, 34], [41, 34], [41, 39], [34, 39]];
	out.push(ent('BUILDING-HATCH', { type: 'hatch', loops: [loop(building, true), loop(yard, false)], solid: false, pattern: 'ANSI31', pattern_scale: 0.6, pattern_angle: 0, normal: UP }));
	out.push(poly('BUILDING', building, true, { lineweight: mm(0.7) }));
	out.push(poly('BUILDING', yard, true));
	out.push(
		ent('TEXT', {
			type: 'm_text',
			position: P(24, 35),
			height: 0.75,
			width: 11,
			rotation: 0,
			value: 'OFFICE BUILDING\\PGFA 1 240 m²',
			plain: 'OFFICE BUILDING\nGFA 1 240 m²',
			style: null,
			attachment: 'middle_center',
			line_spacing: 1,
			normal: UP,
		})
	);
	out.push(text('TEXT', 37.5, 36.5, 0.5, 'COURTYARD'));
	out.push(ent('TEXT', { type: 'leader', vertices: [P(24, 32), P(21, 28.5), P(17.5, 28.5)], arrowhead: true }));
	out.push(text('TEXT', 17.2, 28.5, 0.55, 'MAIN ENTRY', { halign: 'right' }));

	// Parking court: two rows of six bays, numbered by a block attribute.
	for (let i = 0; i < 12; i++) {
		const row = i < 6 ? 0 : 1;
		const x = 16 + (i % 6) * 2.5;
		const y = row === 0 ? 13 : 24;
		const no = 'P' + String(i + 1).padStart(2, '0');
		out.push(insert('PARKING', 'BAY', x, y, { attributes: [attribute('NO', no, x + 1.25, y + (row === 0 ? 1.2 : 3.8), 0.55)] }));
	}
	out.push(poly('PARKING', [[33, 14.5], [36.5, 14.5], [36.5, 27.5], [33, 27.5]], true));
	out.push(text('PARKING', 34.75, 21, 0.5, 'BICYCLES', { rotation: Math.PI / 2 }));

	// Landscape: trees along the boundary, a pond (ellipse) and a winding footpath (spline).
	const rnd = lcg(20261116);
	const spots = [
		[4, 4], [10, 3.5], [17, 4], [24, 3.5], [31, 4], [52, 5], [59, 4], [60, 12], [59.5, 20], [60, 28],
		[4, 12], [3.5, 20], [4, 28], [4, 36], [10, 40.5], [52, 34], [57, 30.5], [5, 41],
	];
	for (const [x, y] of spots) out.push(insert('LANDSCAPE', 'TREE', x + rnd() * 0.8 - 0.4, y + rnd() * 0.8 - 0.4, { scale: r(0.9 + rnd() * 0.9), rotation: r(rnd() * Math.PI) }));
	out.push(ent('LANDSCAPE', { type: 'ellipse', center: P(54.5, 21), major_axis: { x: 3.4, y: 1.2, z: 0 }, ratio: 0.55, start_param: 0, end_param: 2 * Math.PI, normal: UP }));
	out.push(text('LANDSCAPE', 54.5, 21, 0.45, 'POND'));
	out.push(
		ent('LANDSCAPE', {
			type: 'spline',
			degree: 3,
			knots: [0, 0, 0, 0, 1, 2, 3, 3, 3, 3],
			control_points: [P(49, 13.5), P(52, 15), P(50.5, 18.5), P(51, 24), P(55.5, 27), P(54, 32)],
			weights: [],
			fit_points: [],
			closed: false,
		})
	);

	// Utilities: a sewer run through four manholes (the tag is a visible attribute), dashed.
	const run = [[8, 8], [8, 22], [8, 38], [26, 43]];
	out.push(poly('UTILITIES', run));
	run.forEach(([x, y], i) => out.push(insert('UTILITIES', 'MANHOLE', x, y, { attributes: [attribute('ID', 'MH-' + (i + 1), x + 1.9, y + 0.1, 0.5), { ...attribute('DEPTH_M', (1.6 + i * 0.25).toFixed(2), x, y, 0.4), invisible: true, display: null, position: null }] })));
	out.push(poly('UTILITIES', [[0, 9.5], [49, 9.5], [49, 0]], false, { color: { type: 'aci', index: 5 }, linetype: 'DASHDOT' }));
	out.push(text('UTILITIES', 28, 10.4, 0.4, 'WATER'));

	// Survey spot levels on the switched-off layer.
	const levels = [[2, 2, 101.2], [32, 2, 101.05], [62, 2, 100.9], [62, 20, 101.3], [62, 36, 101.85], [46, 42.5, 102.1], [24, 42.5, 102.0], [2, 42.5, 101.9], [2, 22, 101.55], [30, 22, 101.4], [50, 30, 101.6], [14, 34, 101.8]];
	for (const [x, y, z] of levels) {
		out.push(ent('SURVEY', { type: 'point', position: P(x, y) }));
		out.push(text('SURVEY', x + 0.5, y + 0.5, 0.4, z.toFixed(2), { halign: 'left', valign: 'baseline' }));
	}

	// Sheet furniture: title, street name, north arrow, scale bar.
	out.push(insert('TEXT', 'NORTH', 58, 39));
	out.push(text('TEXT', 32, -3, 0.9, 'MILL LANE'));
	for (let i = 0; i < 4; i++) out.push(poly('TEXT', [[2 + i * 5, -6.2], [7 + i * 5, -6.2], [7 + i * 5, -5.8], [2 + i * 5, -5.8]], true));
	for (let i = 0; i <= 4; i++) out.push(text('TEXT', 2 + i * 5, -5, 0.45, String(i * 5)));
	out.push(text('TEXT', 23.5, -5, 0.45, 'm', { halign: 'left' }));
	out.push(text('TEXT', 32, -8.5, 0.6, 'SYNTHETIC SAMPLE SITE PLAN - cadkit.org - MIT OR Apache-2.0'));
	return out;
}

/** The format-neutral document (docs/schema/document-v1.schema.json). */
export function buildDocument() {
	return {
		schema_version: 1,
		source: { format: 'dxf', version: 'AC1032', application: null, codepage: null },
		units: { unit: 'meter', meters_per_unit: 1 },
		layers: LAYERS.map(([name, color, linetype, weight, visible]) => ({
			name,
			id: null,
			color,
			linetype,
			lineweight: weight === null ? { type: 'default' } : mm(weight),
			visible,
			frozen: false,
			locked: false,
			plottable: true,
			description: null,
			props: {},
		})),
		linetypes: [
			{ name: 'CONTINUOUS', description: 'Solid line', pattern: [], props: {} },
			{ name: 'DASHED', description: 'Dashed __ __ __', pattern: [1.2, -0.6], props: {} },
			{ name: 'DASHDOT', description: 'Dash dot __ . __ .', pattern: [1.6, -0.5, 0, -0.5], props: {} },
		],
		text_styles: [],
		blocks: BLOCKS.map((blk) => ({ name: blk.name, id: null, base_point: P(0, 0), description: blk.description, entities: blk.entities, is_xref: false, xref_path: null, props: {} })),
		models: [{ name: 'Model', id: null, kind: 'model', is_3d: false, entities: entities(), props: {} }],
		props: {},
		warnings: [],
	};
}

/** Builds the final R2018 DXF text with cadkit's own writer and checks that it reads back complete. */
export function buildSample() {
	const { CadDocument, read } = require('cadkit-wasm/node');
	const source = buildDocument();
	const doc = CadDocument.fromJsonString(JSON.stringify(source));
	let dxf;
	try {
		dxf = doc.toDxf('r2018');
	} finally {
		doc.free();
	}
	const back = read(new TextEncoder().encode(dxf));
	try {
		const info = back.info();
		const want = source.models[0].entities.length;
		if (info.models[0]?.entities !== want || info.blockCount < BLOCKS.length || info.warningCount !== 0) {
			throw new Error(`the written DXF reads back as ${info.models[0]?.entities} entities, ${info.blockCount} blocks, ${info.warningCount} warnings; expected ${want}, ${BLOCKS.length}, 0`);
		}
	} finally {
		back.free();
	}
	return dxf;
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
	const out = buildSample();
	if (process.argv.includes('--check')) {
		const committed = readFileSync(SAMPLE_PATH, 'utf8');
		if (committed !== out) {
			console.error('cadkit-sample-site.dxf is stale: run node scripts/samples/siteplan.mjs');
			process.exit(1);
		}
		console.log('cadkit-sample-site.dxf is up to date');
	} else {
		mkdirSync(dirname(SAMPLE_PATH), { recursive: true });
		writeFileSync(SAMPLE_PATH, out);
		console.log('wrote ' + SAMPLE_PATH + ' (' + Buffer.byteLength(out) + ' bytes)');
	}
}
