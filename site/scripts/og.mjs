// Open Graph card (1200 x 630) drawn as a drawing sheet: blueprint ground, grid, the page title
// and description, and a title block. All text is outlined with fontkit, so rendering needs no
// installed fonts and is identical on every build machine.

import { createRequire } from 'node:module';
import { markElements, opticalOffsetX } from '../src/lib/brand-mark.mjs';
import { textPath, wrap, measure } from './lib/text-path.mjs';

const require = createRequire(import.meta.url);
const { Resvg } = require('@resvg/resvg-js');

const W = 1200;
const H = 630;
const C = {
	bg: '#0b1324',
	minor: '#122037',
	major: '#1b2a47',
	border: '#22314f',
	text: '#e8edf5',
	muted: '#9aa8bf',
	accent: '#5ce1e6',
	redline: '#ff7a66',
	surface: '#101b31',
};

const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

/**
 * @param {{title: string, description: string, path: string, version: string, date: string}} page
 * @returns {Buffer} PNG bytes
 */
export function renderOgCard(page) {
	const TITLE = { family: 'sans', size: 66, width: 115, weight: 600, tracking: -0.015 };
	const DESC = { family: 'sans', size: 28, width: 100, weight: 400 };
	const LABEL = { family: 'mono', size: 14, weight: 500, tracking: 0.08 };
	const VALUE = { family: 'mono', size: 21, weight: 500 };
	const WORD = { family: 'sans', size: 30, width: 115, weight: 500, tracking: -0.01 };

	const x0 = 72;
	const maxW = W - 2 * x0;
	let body = '';

	// Grid: 12 px minor, 60 px major.
	body += `<rect width="${W}" height="${H}" fill="${C.bg}"/>`;
	body += '<g shape-rendering="crispEdges">';
	for (let x = 0; x <= W; x += 12) body += `<path d="M${x + 0.5} 0V${H}" stroke="${x % 60 === 0 ? C.major : C.minor}"/>`;
	for (let y = 0; y <= H; y += 12) body += `<path d="M0 ${y + 0.5}H${W}" stroke="${y % 60 === 0 ? C.major : C.minor}"/>`;
	body += '</g>';

	// Brand.
	body += `<g transform="translate(${x0 - 4 + opticalOffsetX() * 1.5} 52) scale(1.5)">${markElements({ arc: C.text, grip: C.accent })}</g>`;
	body += `<path d="${textPath('cadkit', x0 + 56, 87, WORD)}" fill="${C.text}"/>`;
	const url = 'cadkit.org' + (page.path === '/' ? '' : page.path);
	const urlOpts = { family: 'mono', size: 18, weight: 400 };
	const urlText = measure(url, urlOpts) > 520 ? url.slice(0, 40) + '\u2026' : url;
	body += `<path d="${textPath(urlText, W - x0 - measure(urlText, urlOpts), 84, urlOpts)}" fill="${C.muted}"/>`;

	// Title (up to 3 lines) and description (up to 2 lines).
	const titleLines = wrap(page.title, maxW, 3, TITLE);
	let y = titleLines.length === 1 ? 300 : titleLines.length === 2 ? 262 : 222;
	for (const line of titleLines) {
		body += `<path d="${textPath(line, x0, y, TITLE)}" fill="${C.text}"/>`;
		y += 74;
	}
	y += 8;
	for (const line of wrap(page.description, maxW, 2, DESC)) {
		body += `<path d="${textPath(line, x0, y, DESC)}" fill="${C.muted}"/>`;
		y += 40;
	}

	// Dimension line above the title block.
	const dy = 506;
	body += `<g stroke="${C.redline}" stroke-width="1.5" fill="none"><path d="M${x0} ${dy - 10}V${dy + 10}M${W - x0} ${dy - 10}V${dy + 10}M${x0} ${dy}H${W - x0}M${x0 - 6} ${dy + 6}l12 -12M${W - x0 - 6} ${dy + 6}l12 -12"/></g>`;

	// Title block.
	const ty = 530;
	const th = 64;
	const cells = [
		['PROJECT', 'cadkit', 230],
		['REV', page.version, 170],
		['LICENSE', 'MIT OR Apache-2.0', 330],
		['DATE', page.date, 0],
	];
	body += `<rect x="${x0 + 0.5}" y="${ty + 0.5}" width="${maxW - 1}" height="${th}" fill="${C.surface}" stroke="${C.text}" stroke-width="1"/>`;
	let cx = x0;
	cells.forEach(([label, value, width], i) => {
		const w = width || x0 + maxW - cx;
		if (i > 0) body += `<path d="M${cx + 0.5} ${ty}V${ty + th}" stroke="${C.border}"/>`;
		body += `<path d="${textPath(label, cx + 16, ty + 24, LABEL)}" fill="${C.muted}"/>`;
		body += `<path d="${textPath(value, cx + 16, ty + 51, VALUE)}" fill="${C.text}"/>`;
		cx += w;
	});
	// Grips at the sheet corners.
	for (const [gx, gy] of [
		[x0, ty],
		[W - x0, ty + th],
	])
		body += `<rect x="${gx - 4}" y="${gy - 4}" width="8" height="8" fill="${C.accent}"/>`;

	const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}"><title>${esc(page.title)}</title>${body}</svg>`;
	return new Resvg(svg, { font: { loadSystemFonts: false } }).render().asPng();
}
