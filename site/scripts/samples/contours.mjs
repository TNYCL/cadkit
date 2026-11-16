#!/usr/bin/env node
// Generates the large synthetic sample of the demo: public/samples/generated/cadkit-sample-contours.dxf.
//
// The drawing is our own work (MIT OR Apache-2.0): contour lines of an invented terrain (a sum
// of smooth hills on a slope), traced with marching squares and written as one LINE per segment,
// about forty thousand of them, plus a frame, a coordinate grid and spot heights. It is not a
// real place. Its purpose is size: it shows how long cadkit takes to read and render tens of
// thousands of entities in a browser.
//
// The file is about 2 MB, so it is not committed: scripts/prebuild.d/10-samples.mjs builds it
// into the git-ignored folder public/samples/generated/. To keep the download small it is written
// directly as minimal ASCII DXF (R12 entities, coordinates to 0.1 m) and then read with
// cadkit-wasm to check it. The script is deterministic: running it twice gives identical bytes.
//
// Usage: node scripts/samples/contours.mjs [--check]
//   --check  exit 1 if the file on disk differs from a fresh build.

import { createRequire } from 'node:module';
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
export const SAMPLE_PATH = join(here, '..', '..', 'public', 'samples', 'generated', 'cadkit-sample-contours.dxf');

/** Map extents in metres, grid spacing of the height samples and contour interval. */
const W = 2400;
const H = 1600;
const STEP = 8;
const INTERVAL = 1;
const INDEX_EVERY = 5;

/** Hills (positive) and hollows (negative): centre, height and radius in metres. */
const HILLS = [
	[520, 1080, 46, 260],
	[1180, 520, 62, 330],
	[1820, 1150, 38, 220],
	[2080, 380, 28, 180],
	[300, 330, 24, 200],
	[1450, 1320, -22, 190],
	[820, 760, -16, 150],
	[1650, 800, 18, 120],
];

/** Terrain height in metres at a map point. */
export function height(x, y) {
	let z = 100 + 0.012 * x + 0.006 * y + 2.2 * Math.sin(x / 150) * Math.cos(y / 190);
	for (const [cx, cy, a, s] of HILLS) z += a * Math.exp(-((x - cx) ** 2 + (y - cy) ** 2) / (2 * s * s));
	return z;
}

const f1 = (n) => {
	const v = Math.round(n * 10) / 10;
	return Object.is(v, -0) ? '0' : String(v);
};

/** Contour segments as [x1, y1, x2, y2, level], traced cell by cell (marching squares). */
export function segments() {
	const nx = W / STEP + 1;
	const ny = H / STEP + 1;
	const z = new Float64Array(nx * ny);
	let lo = Infinity;
	let hi = -Infinity;
	for (let j = 0; j < ny; j++)
		for (let i = 0; i < nx; i++) {
			const v = height(i * STEP, j * STEP);
			z[j * nx + i] = v;
			lo = Math.min(lo, v);
			hi = Math.max(hi, v);
		}
	const out = [];
	for (let j = 0; j < ny - 1; j++) {
		for (let i = 0; i < nx - 1; i++) {
			const a = z[j * nx + i]; // south-west
			const b = z[j * nx + i + 1]; // south-east
			const c = z[(j + 1) * nx + i + 1]; // north-east
			const d = z[(j + 1) * nx + i]; // north-west
			const cellLo = Math.min(a, b, c, d);
			const cellHi = Math.max(a, b, c, d);
			const x = i * STEP;
			const y = j * STEP;
			for (let level = Math.ceil(cellLo / INTERVAL) * INTERVAL; level < cellHi; level += INTERVAL) {
				// Crossing point on each cell edge, by linear interpolation.
				const t = (p, q) => (level - p) / (q - p);
				const S = () => [x + STEP * t(a, b), y];
				const E = () => [x + STEP, y + STEP * t(b, c)];
				const N = () => [x + STEP * t(d, c), y + STEP];
				const Wd = () => [x, y + STEP * t(a, d)];
				const code = (a >= level ? 1 : 0) | (b >= level ? 2 : 0) | (c >= level ? 4 : 0) | (d >= level ? 8 : 0);
				const pairs = [];
				switch (code) {
					case 1:
					case 14:
						pairs.push([Wd(), S()]);
						break;
					case 2:
					case 13:
						pairs.push([S(), E()]);
						break;
					case 3:
					case 12:
						pairs.push([Wd(), E()]);
						break;
					case 4:
					case 11:
						pairs.push([E(), N()]);
						break;
					case 6:
					case 9:
						pairs.push([S(), N()]);
						break;
					case 7:
					case 8:
						pairs.push([Wd(), N()]);
						break;
					case 5:
					case 10: {
						// Saddle: the cell centre decides which corners are joined.
						const high = (a + b + c + d) / 4 >= level;
						if ((code === 5) === high) pairs.push([Wd(), N()], [S(), E()]);
						else pairs.push([Wd(), S()], [E(), N()]);
						break;
					}
					default:
						break;
				}
				for (const [p, q] of pairs) out.push([p[0], p[1], q[0], q[1], level]);
			}
		}
	}
	return { list: out, lo, hi };
}

/** The whole drawing as minimal ASCII DXF. */
export function buildSample() {
	const out = [];
	const pair = (code, value) => out.push(String(code), String(value));
	const line = (layer, x1, y1, x2, y2) => {
		const [a, b, c, d] = [f1(x1), f1(y1), f1(x2), f1(y2)];
		if (a === c && b === d) return false; // shorter than the 0.1 m rounding
		pair(0, 'LINE');
		pair(8, layer);
		pair(10, a);
		pair(20, b);
		pair(11, c);
		pair(21, d);
		return true;
	};
	const text = (layer, x, y, h, value, centred = true) => {
		pair(0, 'TEXT');
		pair(8, layer);
		pair(10, f1(x));
		pair(20, f1(y));
		pair(40, h);
		pair(1, value);
		if (centred) {
			pair(72, 1);
			pair(11, f1(x));
			pair(21, f1(y));
			pair(73, 2);
		}
	};

	const { list, lo, hi } = segments();
	let lines = 0;
	for (const [x1, y1, x2, y2, level] of list) {
		const index = Math.round(level / INTERVAL) % INDEX_EVERY === 0;
		if (line(index ? 'CONTOUR-INDEX' : 'CONTOUR', x1, y1, x2, y2)) lines++;
	}
	// Coordinate grid every 400 m with labels, frame, spot heights on the hilltops, title.
	for (let x = 0; x <= W; x += 400) {
		if (x > 0 && x < W) line('GRID', x, 0, x, H);
		text('GRID', x, -28, 18, String(x));
	}
	for (let y = 0; y <= H; y += 400) {
		if (y > 0 && y < H) line('GRID', 0, y, W, y);
		text('GRID', -46, y, 18, String(y));
	}
	line('FRAME', 0, 0, W, 0);
	line('FRAME', W, 0, W, H);
	line('FRAME', W, H, 0, H);
	line('FRAME', 0, H, 0, 0);
	for (const [cx, cy, a] of HILLS) {
		if (a < 0) continue;
		pair(0, 'POINT');
		pair(8, 'SPOT');
		pair(10, f1(cx));
		pair(20, f1(cy));
		text('SPOT', cx + 14, cy + 10, 16, height(cx, cy).toFixed(1), false);
	}
	text('FRAME', W / 2, -80, 26, `SYNTHETIC CONTOUR MAP - ${INTERVAL} m interval, ${Math.floor(lo)} to ${Math.ceil(hi)} m - cadkit.org - MIT OR Apache-2.0`);

	const layers = [
		['CONTOUR', 33],
		['CONTOUR-INDEX', 32],
		['GRID', 9],
		['FRAME', 7],
		['SPOT', 1],
	];
	const head = ['0', 'SECTION', '2', 'HEADER', '9', '$ACADVER', '1', 'AC1009', '9', '$INSUNITS', '70', '6', '0', 'ENDSEC'];
	const tables = ['0', 'SECTION', '2', 'TABLES', '0', 'TABLE', '2', 'LAYER', '70', String(layers.length)];
	for (const [name, color] of layers) tables.push('0', 'LAYER', '2', name, '70', '0', '62', String(color), '6', 'CONTINUOUS');
	tables.push('0', 'ENDTAB', '0', 'ENDSEC');
	const dxf = [...head, ...tables, '0', 'SECTION', '2', 'ENTITIES', ...out, '0', 'ENDSEC', '0', 'EOF'].join('\n') + '\n';

	// The file must open with cadkit, complete and without warnings.
	const { read } = require('cadkit-wasm/node');
	const doc = read(new TextEncoder().encode(dxf));
	try {
		const info = doc.info();
		const got = info.models[0]?.entities ?? 0;
		if (got < lines || info.warningCount !== 0 || info.units !== 'meter') {
			throw new Error(`the contour map reads back as ${got} entities (wrote ${lines} lines), ${info.warningCount} warnings, units ${info.units}`);
		}
	} finally {
		doc.free();
	}
	return dxf;
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
	const text = buildSample();
	if (process.argv.includes('--check')) {
		if (!existsSync(SAMPLE_PATH) || readFileSync(SAMPLE_PATH, 'utf8') !== text) {
			console.error('cadkit-sample-contours.dxf is missing or stale: run node scripts/samples/contours.mjs');
			process.exit(1);
		}
		console.log('cadkit-sample-contours.dxf is up to date');
	} else {
		mkdirSync(dirname(SAMPLE_PATH), { recursive: true });
		writeFileSync(SAMPLE_PATH, text);
		console.log('wrote ' + SAMPLE_PATH + ' (' + Buffer.byteLength(text) + ' bytes)');
	}
}
