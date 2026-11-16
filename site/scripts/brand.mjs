#!/usr/bin/env node
// Generates the cadkit logo set from src/lib/brand-mark.mjs and the Archivo wordmark:
//
//   public/brand/cadkit-mark.svg         two-tone mark for light backgrounds
//   public/brand/cadkit-mark-dark.svg    two-tone mark for dark backgrounds
//   public/brand/cadkit-mark-mono.svg    single colour (black; recolour freely)
//   public/brand/cadkit-lockup*.svg      mark + outlined "cadkit" wordmark (same three variants)
//   public/brand/cadkit-tile.svg         navy tile master (avatar, favicon)
//   public/brand/cadkit-avatar-512.png   GitHub organisation avatar
//   public/brand/cadkit-icon-256.png     README image
//   public/brand/cadkit-favicon-32.png
//   public/favicon.svg, public/favicon.ico (16 + 32), public/apple-touch-icon.png (180)
//
// Usage: node scripts/brand.mjs [--check]   (--check: exit 1 when a committed SVG is stale)

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { markElements, opticalOffsetX, GRID } from '../src/lib/brand-mark.mjs';
import { textPath, measure, metrics } from './lib/text-path.mjs';

const require = createRequire(import.meta.url);
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const pub = join(root, 'public');
const brand = join(pub, 'brand');

const INK = { text: '#14171c', accent: '#1747b8' };
const BLUEPRINT = { bg: '#0b1324', text: '#e8edf5', accent: '#5ce1e6' };

const svg = (w, h, body, title) =>
	`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" role="img" aria-label="${title}"><title>${title}</title>${body}</svg>\n`;

function mark(paint, weight = 'ui') {
	return svg(GRID, GRID, `<g transform="translate(${opticalOffsetX(weight)} 0)">${markElements({ ...paint, weight })}</g>`, 'cadkit');
}

// Wordmark: lowercase, Archivo wdth 115, weight 500, tracking -0.01em, outlined to paths.
const WORD = { family: 'sans', size: 23, width: 115, weight: 500, tracking: -0.01 };
function lockup(paint, textColor) {
	const gap = 9;
	const m = metrics(WORD);
	const baseline = 16 + m.capHeight / 2;
	const x = GRID + gap;
	const width = Math.ceil(x + measure('cadkit', WORD) + 1);
	const body =
		`<g transform="translate(${opticalOffsetX('ui')} 0)">${markElements(paint)}</g>` +
		`<path d="${textPath('cadkit', x, baseline, WORD)}" fill="${textColor}"/>`;
	return svg(width, GRID, body, 'cadkit');
}

function tile(size, { radius = 0, scale = 0.62 } = {}) {
	const inner = GRID * scale;
	const offset = (GRID - inner) / 2;
	const k = scale;
	const body =
		`<rect width="${GRID}" height="${GRID}" rx="${radius}" fill="${BLUEPRINT.bg}"/>` +
		`<g transform="translate(${offset + opticalOffsetX('tile') * k} ${offset}) scale(${k})">${markElements({ arc: BLUEPRINT.accent, grip: BLUEPRINT.accent, weight: 'tile' })}</g>`;
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${GRID} ${GRID}" width="${size}" height="${size}">${body}</svg>\n`;
}

function favicon() {
	// The tab icon keeps the navy tile so it reads on light and dark browser chrome alike. The
	// "icon" weight drops the centre-mark dashes, which would blur into a smudge at 16 px.
	const body =
		`<rect width="32" height="32" rx="6" fill="${BLUEPRINT.bg}"/>` +
		`<g transform="translate(${3.2 + opticalOffsetX('icon') * 0.8} 3.2) scale(0.8)">${markElements({ arc: BLUEPRINT.accent, grip: BLUEPRINT.accent, weight: 'icon' })}</g>`;
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">${body}</svg>\n`;
}

const svgs = {
	[join(brand, 'cadkit-mark.svg')]: mark({ arc: INK.text, grip: INK.accent }),
	[join(brand, 'cadkit-mark-dark.svg')]: mark({ arc: BLUEPRINT.text, grip: BLUEPRINT.accent }),
	[join(brand, 'cadkit-mark-mono.svg')]: mark({ arc: '#000000', grip: '#000000' }),
	[join(brand, 'cadkit-lockup.svg')]: lockup({ arc: INK.text, grip: INK.accent }, INK.text),
	[join(brand, 'cadkit-lockup-dark.svg')]: lockup({ arc: BLUEPRINT.text, grip: BLUEPRINT.accent }, BLUEPRINT.text),
	[join(brand, 'cadkit-lockup-mono.svg')]: lockup({ arc: '#000000', grip: '#000000' }, '#000000'),
	[join(brand, 'cadkit-tile.svg')]: tile(512),
	[join(pub, 'favicon.svg')]: favicon(),
};

if (process.argv.includes('--check')) {
	const stale = Object.entries(svgs).filter(([p, c]) => !existsSync(p) || readFileSync(p, 'utf8') !== c);
	if (stale.length) {
		console.error('stale brand assets (run node scripts/brand.mjs):\n' + stale.map(([p]) => '  ' + p).join('\n'));
		process.exit(1);
	}
	console.log('brand SVGs are up to date');
	process.exit(0);
}

const { Resvg } = require('@resvg/resvg-js');
const png = (svgText, size) =>
	new Resvg(svgText, { fitTo: { mode: 'width', value: size }, font: { loadSystemFonts: false } }).render().asPng();

/** ICO container with embedded PNG images (supported by every current browser and OS). */
function ico(images) {
	const header = Buffer.alloc(6 + 16 * images.length);
	header.writeUInt16LE(0, 0);
	header.writeUInt16LE(1, 2);
	header.writeUInt16LE(images.length, 4);
	let offset = header.length;
	images.forEach(({ size, data }, i) => {
		const e = 6 + 16 * i;
		header.writeUInt8(size >= 256 ? 0 : size, e);
		header.writeUInt8(size >= 256 ? 0 : size, e + 1);
		header.writeUInt8(0, e + 2);
		header.writeUInt8(0, e + 3);
		header.writeUInt16LE(1, e + 4);
		header.writeUInt16LE(32, e + 6);
		header.writeUInt32LE(data.length, e + 8);
		header.writeUInt32LE(offset, e + 12);
		offset += data.length;
	});
	return Buffer.concat([header, ...images.map((i) => i.data)]);
}

mkdirSync(brand, { recursive: true });
for (const [p, c] of Object.entries(svgs)) writeFileSync(p, c);

const fav = svgs[join(pub, 'favicon.svg')];
const outputs = {
	[join(brand, 'cadkit-avatar-512.png')]: png(tile(512), 512),
	[join(brand, 'cadkit-icon-256.png')]: png(tile(256, { radius: 6 }), 256),
	[join(brand, 'cadkit-favicon-32.png')]: png(fav, 32),
	[join(pub, 'apple-touch-icon.png')]: png(tile(180, { scale: 0.6 }), 180),
	[join(pub, 'favicon.ico')]: ico([
		{ size: 16, data: png(fav, 16) },
		{ size: 32, data: png(fav, 32) },
	]),
};
for (const [p, data] of Object.entries(outputs)) writeFileSync(p, data);
console.log('wrote ' + (Object.keys(svgs).length + Object.keys(outputs).length) + ' brand files');
