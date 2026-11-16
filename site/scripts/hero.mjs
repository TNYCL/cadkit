#!/usr/bin/env node
// Renders the landing page hero at build time: reads public/samples/cadkit-sample-plan.dxf with
// cadkit-wasm (Node build), exports SVG with doc.toSvg, restyles it for the site theme and
// records the real document facts. Output: src/generated/hero-plan.json (git-ignored).
//
// Runs automatically before dev, build and check (npm pre-scripts).

import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const samplePath = join(root, 'public', 'samples', 'cadkit-sample-plan.dxf');
const outPath = join(root, 'src', 'generated', 'hero-plan.json');

const { read } = require('cadkit-wasm/node');
const wasmVersion = require('cadkit-wasm/package.json').version;

const bytes = readFileSync(samplePath);
const doc = read(new Uint8Array(bytes));
let info, layers, entities, raw;
try {
	info = doc.info();
	layers = doc.layers();
	entities = doc.toJSON().models[0]?.entities ?? [];
	raw = doc.toSvg({ model: 0, widthPx: 1200, strokePx: 1, background: null, text: true });
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
	svgBytes: Buffer.byteLength(svg),
	svg,
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, JSON.stringify(facts, null, '\t') + '\n');
console.log(
	`hero: ${facts.format} ${facts.version}, ${facts.entities} entities, ${facts.layers} layers, svg ${facts.svgBytes} B (${facts.renderer})`
);
