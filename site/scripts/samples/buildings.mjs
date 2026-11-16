#!/usr/bin/env node
// Generates the synthetic CityGML sample used by the demo: public/samples/cadkit-sample-buildings.gml.
//
// The model is our own work (MIT OR Apache-2.0): a fictional gable-roof house and a flat-roof
// garage, each an LoD2 solid whose faces are the polygons of its wall, roof and ground surfaces
// (shared by local xlink). Coordinates are round numbers in ETRS89 / UTM zone 32N (EPSG:25832)
// with DHHN2016 heights (EPSG:7837), given explicitly as a compound CRS. The buildings are not
// real. The script writes CityGML 2.0 XML in memory, reads it with cadkit-wasm's native CityGML
// reader, validates it (structure and solid geometry) and writes the final file with cadkit's own
// CityGML writer. Running it twice gives identical bytes.
//
// Usage: node scripts/samples/buildings.mjs [--check]
//   --check  exit 1 if the committed file differs from a fresh build (used by npm test).

import { createRequire } from 'node:module';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
export const SAMPLE_PATH = join(here, '..', '..', 'public', 'samples', 'cadkit-sample-buildings.gml');

/** Compound CRS: ETRS89 / UTM zone 32N + DHHN2016 height. */
export const SRS = 'urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837';
/** Local origin of the site (easting, northing, ground height in metres). */
const ORIGIN = [500000, 5600000, 50];

/**
 * A building as a closed polyhedron: named vertices (local metres) and faces as vertex-name
 * loops, counter-clockwise seen from outside, each tagged with its CityGML boundary surface.
 */
export function gableHouse(x0, y0, w, d, eaves, ridge) {
	const v = {
		A: [x0, y0, 0], B: [x0 + w, y0, 0], C: [x0 + w, y0 + d, 0], D: [x0, y0 + d, 0],
		Ae: [x0, y0, eaves], Be: [x0 + w, y0, eaves], Ce: [x0 + w, y0 + d, eaves], De: [x0, y0 + d, eaves],
		R1: [x0, y0 + d / 2, ridge], R2: [x0 + w, y0 + d / 2, ridge],
	};
	const faces = [
		['GroundSurface', 'ground', ['A', 'D', 'C', 'B']],
		['WallSurface', 'wall-s', ['A', 'B', 'Be', 'Ae']],
		['WallSurface', 'wall-e', ['B', 'C', 'Ce', 'R2', 'Be']],
		['WallSurface', 'wall-n', ['C', 'D', 'De', 'Ce']],
		['WallSurface', 'wall-w', ['D', 'A', 'Ae', 'R1', 'De']],
		['RoofSurface', 'roof-s', ['Ae', 'Be', 'R2', 'R1']],
		['RoofSurface', 'roof-n', ['Ce', 'De', 'R1', 'R2']],
	];
	return { v, faces };
}

export function flatBox(x0, y0, w, d, h) {
	const v = {
		A: [x0, y0, 0], B: [x0 + w, y0, 0], C: [x0 + w, y0 + d, 0], D: [x0, y0 + d, 0],
		At: [x0, y0, h], Bt: [x0 + w, y0, h], Ct: [x0 + w, y0 + d, h], Dt: [x0, y0 + d, h],
	};
	const faces = [
		['GroundSurface', 'ground', ['A', 'D', 'C', 'B']],
		['WallSurface', 'wall-s', ['A', 'B', 'Bt', 'At']],
		['WallSurface', 'wall-e', ['B', 'C', 'Ct', 'Bt']],
		['WallSurface', 'wall-n', ['C', 'D', 'Dt', 'Ct']],
		['WallSurface', 'wall-w', ['D', 'A', 'At', 'Dt']],
		['RoofSurface', 'roof', ['At', 'Bt', 'Ct', 'Dt']],
	];
	return { v, faces };
}

const BUILDINGS = [
	{
		id: 'ck-house',
		name: 'House',
		description: 'Synthetic gable-roof house, 10 m x 8 m, eaves at 6 m, ridge at 9 m. Fictional; not a real building.',
		height: 9,
		storeys: 2,
		shape: gableHouse(0, 0, 10, 8, 6, 9),
	},
	{
		id: 'ck-garage',
		name: 'Garage',
		description: 'Synthetic flat-roof garage, 4 m x 6 m, 3 m high. Fictional; not a real building.',
		height: 3,
		storeys: 1,
		shape: flatBox(12, 1, 4, 6, 3),
	},
];

const pos = ([x, y, z]) => `${ORIGIN[0] + x} ${ORIGIN[1] + y} ${ORIGIN[2] + z}`;

export function buildingXml(b) {
	const { v, faces } = b.shape;
	const polyId = (f) => `${b.id}-${f[1]}-poly`;
	const members = faces.map((f) => `<gml:surfaceMember xlink:href="#${polyId(f)}"/>`).join('\n');
	const bounded = faces
		.map((f) => {
			const ring = [...f[2], f[2][0]].map((k) => pos(v[k])).join(' ');
			return (
				`<bldg:boundedBy><bldg:${f[0]} gml:id="${b.id}-${f[1]}"><bldg:lod2MultiSurface>` +
				`<gml:MultiSurface srsName="${SRS}" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="${polyId(f)}">` +
				`<gml:exterior><gml:LinearRing><gml:posList srsDimension="3">${ring}</gml:posList></gml:LinearRing></gml:exterior>` +
				`</gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:${f[0]}></bldg:boundedBy>`
			);
		})
		.join('\n');
	return `<core:cityObjectMember>
<bldg:Building gml:id="${b.id}">
<gml:description>${b.description}</gml:description>
<gml:name>${b.name}</gml:name>
<gen:stringAttribute name="source"><gen:value>cadkit.org synthetic sample, MIT OR Apache-2.0</gen:value></gen:stringAttribute>
<bldg:measuredHeight uom="m">${b.height}</bldg:measuredHeight>
<bldg:storeysAboveGround>${b.storeys}</bldg:storeysAboveGround>
<bldg:lod2Solid><gml:Solid srsName="${SRS}" srsDimension="3"><gml:exterior><gml:CompositeSurface>
${members}
</gml:CompositeSurface></gml:exterior></gml:Solid></bldg:lod2Solid>
${bounded}
</bldg:Building>
</core:cityObjectMember>`;
}

/**
 * A CityGML 2.0 city model of the given buildings ({id, name, description, height, storeys,
 * shape}), with an envelope computed from their vertices.
 */
export function cityModelXml(buildings, name, description) {
	const all = buildings.flatMap((b) => Object.values(b.shape.v));
	const lo = [0, 1, 2].map((i) => Math.min(...all.map((p) => p[i])));
	const hi = [0, 1, 2].map((i) => Math.max(...all.map((p) => p[i])));
	return `<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:bldg="http://www.opengis.net/citygml/building/2.0" xmlns:gen="http://www.opengis.net/citygml/generics/2.0" xmlns:gml="http://www.opengis.net/gml" xmlns:xlink="http://www.w3.org/1999/xlink">
<gml:description>${description}</gml:description>
<gml:name>${name}</gml:name>
<gml:boundedBy><gml:Envelope srsName="${SRS}" srsDimension="3"><gml:lowerCorner>${pos(lo)}</gml:lowerCorner><gml:upperCorner>${pos(hi)}</gml:upperCorner></gml:Envelope></gml:boundedBy>
${buildings.map(buildingXml).join('\n')}
</core:CityModel>
`;
}

export function buildSourceXml() {
	return cityModelXml(BUILDINGS, 'cadkit sample buildings', 'Synthetic CityGML 2.0 sample by cadkit.org (MIT OR Apache-2.0). Fictional buildings at round coordinates.');
}

/** Reads CityGML XML with cadkit's native reader, validates it and writes it with cadkit's writer. */
export function writeValidated(xml, polygons) {
	const { readCityGml } = require('cadkit-wasm/node');
	const native = readCityGml(new TextEncoder().encode(xml));
	try {
		const report = native.validate({ geometry: true });
		if (report.polygons !== polygons) throw new Error(`unexpected polygon count ${report.polygons}, expected ${polygons}`);
		return Buffer.from(native.toGml({ geometry: true })).toString('utf8');
	} finally {
		native.free();
	}
}

/** Reads the XML with cadkit's native CityGML reader, validates it and writes it with cadkit's writer. */
export function buildSample() {
	return writeValidated(
		buildSourceXml(),
		BUILDINGS.reduce((n, b) => n + b.shape.faces.length, 0)
	);
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
	const text = buildSample();
	if (process.argv.includes('--check')) {
		const committed = readFileSync(SAMPLE_PATH, 'utf8');
		if (committed !== text) {
			console.error('cadkit-sample-buildings.gml is stale: run node scripts/samples/buildings.mjs');
			process.exit(1);
		}
		console.log('cadkit-sample-buildings.gml is up to date');
	} else {
		mkdirSync(dirname(SAMPLE_PATH), { recursive: true });
		writeFileSync(SAMPLE_PATH, text);
		console.log('wrote ' + SAMPLE_PATH + ' (' + Buffer.byteLength(text) + ' bytes)');
	}
}
