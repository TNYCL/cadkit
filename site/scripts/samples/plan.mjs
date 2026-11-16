#!/usr/bin/env node
// Generates the synthetic floor plan used by the landing page hero and the demo:
// public/samples/cadkit-sample-plan.dxf.
//
// The drawing is our own work (MIT OR Apache-2.0): a fictional 12 m x 9 m apartment with no
// real address, owner or project. The script is deterministic: it writes a minimal R12 DXF in
// memory, reads it with cadkit-wasm, sets metric units on the neutral document and writes the
// final file with cadkit's own DXF writer (R2018). Running it twice gives identical bytes.
//
// Usage: node scripts/samples/plan.mjs [--check]
//   --check  exit 1 if the committed file differs from a fresh build (used by npm test).

import { createRequire } from 'node:module';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
export const SAMPLE_PATH = join(here, '..', '..', 'public', 'samples', 'cadkit-sample-plan.dxf');

// Exterior wall thickness in metres (interior walls are 0.12 m, given by their face coordinates).
const EXT = 0.3;

/** Layer table: name, ACI colour. */
const LAYERS = [
	['WALLS', 7],
	['DOORS', 4],
	['WINDOWS', 5],
	['DIMENSIONS', 1],
	['TEXT', 7],
	['GRID', 8],
];

const fmt = (n) => {
	const v = Math.round(n * 10000) / 10000;
	return Object.is(v, -0) ? '0' : String(v);
};

/** Collects DXF entity records as [code, value] pairs. */
class Sink {
	constructor() {
		this.out = [];
	}
	pair(code, value) {
		this.out.push(String(code), typeof value === 'number' ? fmt(value) : String(value));
	}
	line(layer, x1, y1, x2, y2) {
		this.pair(0, 'LINE');
		this.pair(8, layer);
		this.pair(10, x1);
		this.pair(20, y1);
		this.pair(30, 0);
		this.pair(11, x2);
		this.pair(21, y2);
		this.pair(31, 0);
	}
	/** Closed polyline (R12 POLYLINE / VERTEX / SEQEND). */
	poly(layer, pts, closed = true) {
		this.pair(0, 'POLYLINE');
		this.pair(8, layer);
		this.pair(66, 1);
		this.pair(10, 0);
		this.pair(20, 0);
		this.pair(30, 0);
		this.pair(70, closed ? 1 : 0);
		for (const [x, y] of pts) {
			this.pair(0, 'VERTEX');
			this.pair(8, layer);
			this.pair(10, x);
			this.pair(20, y);
			this.pair(30, 0);
		}
		this.pair(0, 'SEQEND');
		this.pair(8, layer);
	}
	rect(layer, x0, y0, x1, y1) {
		this.poly(layer, [
			[x0, y0],
			[x1, y0],
			[x1, y1],
			[x0, y1],
		]);
	}
	/** Arc, angles in degrees counter-clockwise (DXF convention). */
	arc(layer, cx, cy, r, a0, a1) {
		this.pair(0, 'ARC');
		this.pair(8, layer);
		this.pair(10, cx);
		this.pair(20, cy);
		this.pair(30, 0);
		this.pair(40, r);
		this.pair(50, a0);
		this.pair(51, a1);
	}
	circle(layer, cx, cy, r) {
		this.pair(0, 'CIRCLE');
		this.pair(8, layer);
		this.pair(10, cx);
		this.pair(20, cy);
		this.pair(30, 0);
		this.pair(40, r);
	}
	/** Text centred on (x, y) (horizontal 1 = centre, vertical 2 = middle). */
	text(layer, x, y, h, value, rotation = 0) {
		this.pair(0, 'TEXT');
		this.pair(8, layer);
		this.pair(10, x);
		this.pair(20, y);
		this.pair(30, 0);
		this.pair(40, h);
		this.pair(1, value);
		if (rotation) this.pair(50, rotation);
		this.pair(72, 1);
		this.pair(11, x);
		this.pair(21, y);
		this.pair(31, 0);
		this.pair(73, 2);
	}
}

/**
 * A straight wall band with openings. For a horizontal wall, the band spans x in [from, to]
 * and y in [a, b]; for a vertical wall the roles swap. Each opening is {at, width, kind, side,
 * hinge} where kind is 'window', 'door' or 'gap', side (+1 / -1) is the swing direction of a
 * door and hinge ('start' / 'end') the jamb it turns on.
 */
function wall(s, dir, from, to, a, b, openings = []) {
	const toXY = (u, v) => (dir === 'h' ? [u, v] : [v, u]);
	const rect = (u0, u1) => {
		const [x0, y0] = toXY(u0, a);
		const [x1, y1] = toXY(u1, b);
		s.rect('WALLS', Math.min(x0, x1), Math.min(y0, y1), Math.max(x0, x1), Math.max(y0, y1));
	};
	const seg = (layer, u0, v0, u1, v1) => {
		const [x0, y0] = toXY(u0, v0);
		const [x1, y1] = toXY(u1, v1);
		s.line(layer, x0, y0, x1, y1);
	};
	const sorted = [...openings].sort((p, q) => p.at - q.at);
	let cursor = from;
	for (const o of sorted) {
		const u0 = o.at;
		const u1 = o.at + o.width;
		if (u0 > cursor) rect(cursor, u0);
		cursor = u1;
		if (o.kind === 'window') {
			// Sill lines at both faces plus the glazing line in the middle, and the jambs.
			const mid = (a + b) / 2;
			seg('WINDOWS', u0, a, u1, a);
			seg('WINDOWS', u0, b, u1, b);
			seg('WINDOWS', u0, mid - 0.02, u1, mid - 0.02);
			seg('WINDOWS', u0, mid + 0.02, u1, mid + 0.02);
			seg('WINDOWS', u0, a, u0, b);
			seg('WINDOWS', u1, a, u1, b);
		} else if (o.kind === 'door') {
			// Leaf drawn open at 90 degrees from the hinge jamb, with its swing arc.
			const w = o.width;
			const hingeU = o.hinge === 'end' ? u1 : u0;
			const freeU = o.hinge === 'end' ? u0 : u1;
			const face = o.side > 0 ? b : a;
			const leafEnd = face + o.side * w;
			seg('DOORS', hingeU, face, hingeU, leafEnd);
			const [cx, cy] = toXY(hingeU, face);
			const [fx, fy] = toXY(freeU, face);
			const [lx, ly] = toXY(hingeU, leafEnd);
			const ang = (x, y) => (Math.atan2(y - cy, x - cx) * 180) / Math.PI;
			let a0 = ang(fx, fy);
			let a1 = ang(lx, ly);
			// The swing spans 90 degrees; choose the counter-clockwise order that keeps it that way.
			let sweep = (a1 - a0 + 360) % 360;
			if (sweep > 180) [a0, a1] = [a1, a0];
			s.arc('DOORS', cx, cy, w, (a0 + 360) % 360, (a1 + 360) % 360);
		}
	}
	if (cursor < to) rect(cursor, to);
}

/** Linear dimension drawn as plain geometry: extension lines, dimension line, ticks, value. */
function dimension(s, dir, u0, u1, base, offset, value) {
	const toXY = (u, v) => (dir === 'h' ? [u, v] : [v, u]);
	const line = (p, q) => s.line('DIMENSIONS', ...toXY(...p), ...toXY(...q));
	const v = base + offset;
	const sign = Math.sign(offset);
	line([u0, base + sign * 0.15], [u0, v + sign * 0.12]);
	line([u1, base + sign * 0.15], [u1, v + sign * 0.12]);
	line([u0 - 0.12, v], [u1 + 0.12, v]);
	for (const u of [u0, u1]) line([u - 0.09, v - 0.09], [u + 0.09, v + 0.09]);
	const [tx, ty] = toXY((u0 + u1) / 2, v + sign * 0.2);
	s.text('DIMENSIONS', tx, ty, 0.16, value, dir === 'h' ? 0 : 90);
}

export function buildR12() {
	const s = new Sink();
	const W = 12;
	const H = 9;

	// Exterior walls (faces at 0 and 12 / 0 and 9).
	wall(s, 'h', 0, W, 0, EXT, [
		{ at: 1.2, width: 2.6, kind: 'window' },
		{ at: 8.0, width: 2.4, kind: 'window' },
	]);
	wall(s, 'h', 0, W, H - EXT, H, [
		{ at: 1.2, width: 1.4, kind: 'window' },
		{ at: 4.6, width: 1.0, kind: 'door', side: -1, hinge: 'start' },
		{ at: 7.2, width: 0.8, kind: 'window' },
		{ at: 9.6, width: 1.4, kind: 'window' },
	]);
	wall(s, 'v', EXT, H - EXT, 0, EXT, [
		{ at: 1.6, width: 2.2, kind: 'window' },
		{ at: 6.4, width: 1.4, kind: 'window' },
	]);
	wall(s, 'v', EXT, H - EXT, W - EXT, W, [
		{ at: 1.8, width: 1.8, kind: 'window' },
		{ at: 7.0, width: 1.2, kind: 'window' },
	]);

	// Interior walls.
	// Living | kitchen + entry (y 5.04 to 5.16).
	wall(s, 'h', EXT, 6.54, 5.04, 5.16, [
		{ at: 1.2, width: 1.2, kind: 'gap' },
		{ at: 4.6, width: 0.9, kind: 'door', side: -1, hinge: 'end' },
	]);
	// Kitchen | entry (x 3.54 to 3.66).
	wall(s, 'v', 5.16, H - EXT, 3.54, 3.66, [{ at: 6.8, width: 0.8, kind: 'door', side: -1, hinge: 'start' }]);
	// Left zone | right zone (x 6.54 to 6.66); the corridor opens onto the entry.
	wall(s, 'v', EXT, H - EXT, 6.54, 6.66, [{ at: 5.16, width: 1.2, kind: 'gap' }]);
	// Bedroom | corridor (y 5.04 to 5.16).
	wall(s, 'h', 6.66, W - EXT, 5.04, 5.16, [{ at: 7.0, width: 0.9, kind: 'door', side: -1, hinge: 'start' }]);
	// Corridor | bath + study (y 6.36 to 6.48).
	wall(s, 'h', 6.66, W - EXT, 6.36, 6.48, [
		{ at: 7.2, width: 0.8, kind: 'door', side: 1, hinge: 'start' },
		{ at: 9.4, width: 0.9, kind: 'door', side: 1, hinge: 'end' },
	]);
	// Bath | study (x 8.7 to 8.82).
	wall(s, 'v', 6.48, H - EXT, 8.7, 8.82);

	// Room labels: name and net floor area from the interior faces.
	const rooms = [
		['LIVING', 0.3, 0.3, 6.54, 5.04],
		['KITCHEN', 0.3, 5.16, 3.54, 8.7],
		['ENTRY', 3.66, 5.16, 6.54, 8.7],
		['BEDROOM', 6.66, 0.3, 11.7, 5.04],
		['CORRIDOR', 6.66, 5.16, 11.7, 6.36],
		['BATH', 6.66, 6.48, 8.7, 8.7],
		['STUDY', 8.82, 6.48, 11.7, 8.7],
	];
	for (const [name, x0, y0, x1, y1] of rooms) {
		const cx = (x0 + x1) / 2;
		const cy = (y0 + y1) / 2;
		const area = ((x1 - x0) * (y1 - y0)).toFixed(2);
		if (name === 'CORRIDOR') {
			s.text('TEXT', cx + 1.6, cy, 0.2, name);
			continue;
		}
		s.text('TEXT', cx, cy + 0.17, 0.24, name);
		s.text('TEXT', cx, cy - 0.19, 0.17, area + ' m²');
	}

	// Structural grid on the wall centre lines, with labelled bubbles.
	const gx = [
		['A', EXT / 2],
		['B', 6.6],
		['C', W - EXT / 2],
	];
	const gy = [
		['1', EXT / 2],
		['2', 5.1],
		['3', H - EXT / 2],
	];
	const R = 0.3;
	for (const [label, x] of gx) {
		s.line('GRID', x, -2.3, x, H + 0.9);
		s.circle('GRID', x, H + 0.9 + R, R);
		s.text('GRID', x, H + 0.9 + R, 0.26, label);
	}
	for (const [label, y] of gy) {
		s.line('GRID', -2.3, y, W + 0.9, y);
		s.circle('GRID', W + 0.9 + R, y, R);
		s.text('GRID', W + 0.9 + R, y, 0.26, label);
	}

	// Dimensions: grid chain and overall size, below and left of the plan.
	dimension(s, 'h', gx[0][1], gx[1][1], 0, -0.8, (gx[1][1] - gx[0][1]).toFixed(2));
	dimension(s, 'h', gx[1][1], gx[2][1], 0, -0.8, (gx[2][1] - gx[1][1]).toFixed(2));
	dimension(s, 'h', 0, W, 0, -1.6, W.toFixed(2));
	dimension(s, 'v', gy[0][1], gy[1][1], 0, -0.8, (gy[1][1] - gy[0][1]).toFixed(2));
	dimension(s, 'v', gy[1][1], gy[2][1], 0, -0.8, (gy[2][1] - gy[1][1]).toFixed(2));
	dimension(s, 'v', 0, H, 0, -1.6, H.toFixed(2));

	s.text('TEXT', W / 2, -2.9, 0.2, 'SYNTHETIC SAMPLE PLAN - cadkit.org - MIT OR Apache-2.0');

	const head = ['0', 'SECTION', '2', 'HEADER', '9', '$ACADVER', '1', 'AC1009', '0', 'ENDSEC'];
	const tables = ['0', 'SECTION', '2', 'TABLES', '0', 'TABLE', '2', 'LAYER', '70', String(LAYERS.length)];
	for (const [name, color] of LAYERS) {
		tables.push('0', 'LAYER', '2', name, '70', '0', '62', String(color), '6', 'CONTINUOUS');
	}
	tables.push('0', 'ENDTAB', '0', 'ENDSEC');
	const body = ['0', 'SECTION', '2', 'ENTITIES', ...s.out, '0', 'ENDSEC', '0', 'EOF'];
	return [...head, ...tables, ...body].join('\n') + '\n';
}

/** Builds the final R2018 DXF bytes with cadkit's own writer. */
export function buildSample() {
	const { read, CadDocument } = require('cadkit-wasm/node');
	const src = read(new TextEncoder().encode(buildR12()), { codepage: 'utf-8' });
	const model = src.toJSON();
	src.free();
	model.units = { ...model.units, unit: 'meter', meters_per_unit: 1 };
	const doc = CadDocument.fromJsonString(JSON.stringify(model));
	try {
		return doc.toDxf('r2018');
	} finally {
		doc.free();
	}
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
	const text = buildSample();
	if (process.argv.includes('--check')) {
		const committed = readFileSync(SAMPLE_PATH, 'utf8');
		if (committed !== text) {
			console.error('cadkit-sample-plan.dxf is stale: run node scripts/samples/plan.mjs');
			process.exit(1);
		}
		console.log('cadkit-sample-plan.dxf is up to date');
	} else {
		mkdirSync(dirname(SAMPLE_PATH), { recursive: true });
		writeFileSync(SAMPLE_PATH, text);
		console.log('wrote ' + SAMPLE_PATH + ' (' + Buffer.byteLength(text) + ' bytes)');
	}
}
