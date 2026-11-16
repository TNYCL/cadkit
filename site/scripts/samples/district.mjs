#!/usr/bin/env node
// Generates the synthetic CityGML district used by the demo: public/samples/cadkit-sample-district.gml.
//
// The model is our own work (MIT OR Apache-2.0): 28 fictional buildings on four rows of plots
// along two streets, with gable roofs in both directions and flat roofs, one to four storeys.
// Each building is an LoD2 solid whose faces are the polygons of its wall, roof and ground
// surfaces, like the two-building sample (scripts/samples/buildings.mjs, whose helpers this
// script uses), in the same compound CRS. Nothing here is a real place. Sizes come from a fixed
// seed, so running the script twice gives identical bytes.
//
// Usage: node scripts/samples/district.mjs [--check]
//   --check  exit 1 if the committed file differs from a fresh build (used by npm test).

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cityModelXml, flatBox, gableHouse, writeValidated } from './buildings.mjs';

const here = dirname(fileURLToPath(import.meta.url));
export const SAMPLE_PATH = join(here, '..', '..', 'public', 'samples', 'cadkit-sample-district.gml');

function lcg(seed) {
	let s = seed >>> 0;
	return () => {
		s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
		return s / 4294967296;
	};
}

/** A gable house with its ridge along Y: the X-ridge house turned by a quarter turn (winding kept). */
function gableHouseY(x0, y0, w, d, eaves, ridge) {
	const { v, faces } = gableHouse(0, 0, d, w, eaves, ridge);
	const turned = Object.fromEntries(Object.entries(v).map(([k, [x, y, z]]) => [k, [x0 + w - y, y0 + x, z]]));
	return { v: turned, faces };
}

export function buildings() {
	const rnd = lcg(2026);
	const half = (lo, hi) => lo + Math.round(rnd() * (hi - lo) * 2) / 2; // steps of 0.5 m
	const out = [];
	// Four rows of seven plots, 18 m x 20 m each; rows 0/1 and 2/3 face a street between them.
	const rowY = [0, 34, 60, 94];
	for (let row = 0; row < 4; row++) {
		for (let col = 0; col < 7; col++) {
			const n = out.length + 1;
			const w = half(9, 14);
			const d = half(8, 12);
			const x0 = col * 18 + half(1, 18 - w - 1);
			const y0 = rowY[row] + half(1.5, 20 - d - 1.5);
			const storeys = 1 + Math.floor(rnd() * 3.4);
			const eaves = storeys * 3;
			const kind = rnd();
			let shape;
			let roof;
			if (kind < 0.42) {
				shape = gableHouse(x0, y0, w, d, eaves, eaves + half(2, 3.5));
				roof = 'gable roof, ridge east-west';
			} else if (kind < 0.78) {
				shape = gableHouseY(x0, y0, w, d, eaves, eaves + half(2, 3.5));
				roof = 'gable roof, ridge north-south';
			} else {
				shape = flatBox(x0, y0, w, d, eaves + 0.5);
				roof = 'flat roof';
			}
			const height = Math.max(...Object.values(shape.v).map((p) => p[2]));
			const id = 'ck-d' + String(n).padStart(2, '0');
			out.push({
				id,
				name: 'Building ' + String(n).padStart(2, '0'),
				description: `Synthetic building, ${w} m x ${d} m, ${roof}, ${height} m high. Fictional; not a real building.`,
				height,
				storeys,
				shape,
			});
		}
	}
	return out;
}

export function buildSample() {
	const list = buildings();
	const xml = cityModelXml(list, 'cadkit sample district', 'Synthetic CityGML 2.0 sample by cadkit.org (MIT OR Apache-2.0). 28 fictional buildings at round coordinates.');
	return writeValidated(
		xml,
		list.reduce((n, b) => n + b.shape.faces.length, 0)
	);
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
	const text = buildSample();
	if (process.argv.includes('--check')) {
		const committed = readFileSync(SAMPLE_PATH, 'utf8');
		if (committed !== text) {
			console.error('cadkit-sample-district.gml is stale: run node scripts/samples/district.mjs');
			process.exit(1);
		}
		console.log('cadkit-sample-district.gml is up to date');
	} else {
		mkdirSync(dirname(SAMPLE_PATH), { recursive: true });
		writeFileSync(SAMPLE_PATH, text);
		console.log('wrote ' + SAMPLE_PATH + ' (' + Buffer.byteLength(text) + ' bytes)');
	}
}
