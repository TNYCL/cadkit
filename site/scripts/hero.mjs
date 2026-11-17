#!/usr/bin/env node
// Renders the landing page drawings at build time with cadkit-wasm (Node build) and records the
// real document facts. Outputs (git-ignored):
//   src/generated/hero-plan.json    the sample plan restyled for the theme, facts, snippet output
//   src/generated/plan.svg          the SVG the "same task" snippets write (widthPx 1200, defaults)
//   src/generated/hero-district.svg axonometric drawing of the CityGML district sample
//
// Runs automatically before dev, build and check (npm pre-scripts).

import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { collectPolygons, axonometric } from './lib/axonometric.mjs';

const require = createRequire(import.meta.url);
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const samplePath = join(root, 'public', 'samples', 'cadkit-sample-plan.dxf');
const districtPath = join(root, 'public', 'samples', 'cadkit-sample-district.gml');
const outDir = join(root, 'src', 'generated');
const outPath = join(outDir, 'hero-plan.json');

const { read } = require('cadkit-wasm/node');
const wasmVersion = require('cadkit-wasm/package.json').version;

const bytes = readFileSync(samplePath);
const doc = read(new Uint8Array(bytes));
let info, layers, entities, raw, snippetSvg;
try {
	info = doc.info();
	layers = doc.layers();
	entities = doc.toJSON().models[0]?.entities ?? [];
	raw = doc.toSvg({ model: 0, widthPx: 1200, strokePx: 1, background: null, text: true });
	// Exactly what the JavaScript snippet on the landing page writes to plan.svg.
	snippetSvg = doc.toSvg({ widthPx: 1200 });
} finally {
	doc.free();
}

const kinds = {};
for (const e of entities) kinds[e.kind.type] = (kinds[e.kind.type] ?? 0) + 1;
const drawnLayers = [...new Set(entities.map((e) => e.layer))];

// Restyle: drop per-entity colours (the theme colours each layer via CSS), keep geometry, and
// normalise every stroked shape to pathLength 1 for the line-draw.
let svg = raw
	.replace(/<\?xml[^>]*>\s*/, '')
	.replace(/^<svg ([^>]*)>/, (_, attrs) => {
		const viewBox = /viewBox="([^"]+)"/.exec(attrs)?.[1];
		if (!viewBox) throw new Error('cadkit SVG has no viewBox');
		return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}" class="ck-plan" role="img" aria-labelledby="ck-plan-title"><title id="ck-plan-title">Synthetic apartment floor plan (12 m by 9 m) rendered by cadkit-wasm</title>`;
	})
	.replace(/ (stroke|fill)="#[0-9a-fA-F]{3,8}"/g, '')
	// Stroke widths are set in user units by CSS: a non-scaling stroke would also make the
	// dash pattern used for the reveal non-scaling, which breaks pathLength.
	.replace(/ vector-effect="non-scaling-stroke"/g, '')
	.replace(/ stroke-width="[^"]*"/g, '')
	.replace(/<(path|line|circle|polyline|polygon|ellipse)\b/g, '<$1 pathLength="1"');

if (!svg.includes('data-layer="WALLS"')) throw new Error('expected per-layer groups in the cadkit SVG');

// District: the polygons of the neutral document model, projected (scripts/lib/axonometric.mjs).
const districtBytes = readFileSync(districtPath);
const gml = read(new Uint8Array(districtBytes));
let districtInfo, districtEntities, districtWarnings;
try {
	districtInfo = gml.info();
	districtEntities = gml.toJSON().models[0]?.entities ?? [];
	districtWarnings = gml.warnings().map((w) => w.code);
} finally {
	gml.free();
}
const polygons = collectPolygons(districtEntities);
const axo = axonometric(polygons);
if (axo.faces.length === 0) throw new Error('district sample produced no visible faces');
const districtSvg =
	`<svg xmlns="http://www.w3.org/2000/svg" viewBox="${axo.viewBox.join(' ')}" class="ck-iso" role="img" aria-labelledby="ck-iso-title">` +
	`<title id="ck-iso-title">Axonometric view of a synthetic CityGML 2.0 district of ${districtInfo.models[0]?.entities ?? 0} buildings, from the polygons cadkit-wasm read</title>` +
	`<path class="ck-iso-site" d="${axo.ground}"/>` +
	axo.faces.map((f) => `<path class="ck-iso-${f.kind}" pathLength="1" d="${f.d}"/>`).join('') +
	'</svg>\n';

const facts = {
	file: 'cadkit-sample-plan.dxf',
	href: '/samples/cadkit-sample-plan.dxf',
	bytes: bytes.length,
	sha256: createHash('sha256').update(bytes).digest('hex'),
	format: info.format,
	version: info.version,
	units: info.units,
	entities: info.models[0]?.entities ?? 0,
	layers: info.layerCount,
	drawnLayers: drawnLayers.length,
	layerNames: layers.map((l) => l.name),
	kinds,
	warnings: info.warningCount,
	renderer: `cadkit-wasm ${wasmVersion}`,
	cadkitWasmVersion: wasmVersion,
	snippetSvgBytes: Buffer.byteLength(snippetSvg),
	svgBytes: Buffer.byteLength(svg),
	svg,
	district: {
		file: 'cadkit-sample-district.gml',
		href: '/samples/cadkit-sample-district.gml',
		bytes: districtBytes.length,
		format: districtInfo.format,
		version: districtInfo.version,
		buildings: districtInfo.models[0]?.entities ?? 0,
		polygons: polygons.length,
		visibleFaces: axo.faces.length,
		layers: districtInfo.layerCount,
		warnings: districtInfo.warningCount,
		warningCodes: districtWarnings,
		svgBytes: Buffer.byteLength(districtSvg),
	},
};

mkdirSync(outDir, { recursive: true });
writeFileSync(outPath, JSON.stringify(facts, null, '\t') + '\n');
writeFileSync(join(outDir, 'plan.svg'), snippetSvg);
writeFileSync(join(outDir, 'hero-district.svg'), districtSvg);
console.log(
	`hero: ${facts.format} ${facts.version}, ${facts.entities} entities, ${facts.layers} layers, svg ${facts.svgBytes} B; ` +
		`district: ${facts.district.buildings} buildings, ${facts.district.visibleFaces} faces, svg ${facts.district.svgBytes} B (${facts.renderer})`
);
