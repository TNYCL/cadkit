// Outlines text to SVG path data with fontkit, so static assets (logo lockups, Open Graph cards)
// render identically everywhere without depending on installed fonts. Uses the same variable
// font files the site serves (@fontsource-variable), instanced at the requested axes.

import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const fontkit = createRequire(import.meta.url)('fontkit');
const siteRoot = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

const FILES = {
	sans: join(siteRoot, 'node_modules/@fontsource-variable/archivo/files/archivo-latin-wdth-normal.woff2'),
	mono: join(siteRoot, 'node_modules/@fontsource-variable/jetbrains-mono/files/jetbrains-mono-latin-wght-normal.woff2'),
};

const cache = new Map();

/** @param {'sans' | 'mono'} family @param {Record<string, number>} axes */
function face(family, axes) {
	const key = family + JSON.stringify(axes);
	let f = cache.get(key);
	if (!f) {
		f = fontkit.openSync(FILES[family]).getVariation(axes);
		cache.set(key, f);
	}
	return f;
}

const n = (v) => String(Math.round(v * 100) / 100);

/**
 * @typedef {object} TextOptions
 * @property {'sans' | 'mono'} [family]
 * @property {number} size font size in output units
 * @property {number} [weight] wght axis
 * @property {number} [width] wdth axis (sans only)
 * @property {number} [tracking] letter spacing in em (e.g. -0.01)
 */

/** Advance width of `text` in output units. @param {string} text @param {TextOptions} o */
export function measure(text, o) {
	const f = face(o.family ?? 'sans', axes(o));
	const run = f.layout(text);
	const scale = o.size / f.unitsPerEm;
	const tracking = (o.tracking ?? 0) * o.size;
	let w = 0;
	for (const p of run.positions) w += p.xAdvance * scale + tracking;
	return w - (run.positions.length ? tracking : 0);
}

/** Font metrics in output units. @param {TextOptions} o */
export function metrics(o) {
	const f = face(o.family ?? 'sans', axes(o));
	const s = o.size / f.unitsPerEm;
	return { capHeight: f.capHeight * s, xHeight: f.xHeight * s, ascent: f.ascent * s, descent: f.descent * s };
}

/**
 * SVG path data for `text` with its baseline starting at (x, y), y down.
 * @param {string} text @param {number} x @param {number} y @param {TextOptions} o
 */
export function textPath(text, x, y, o) {
	const f = face(o.family ?? 'sans', axes(o));
	const run = f.layout(text);
	const s = o.size / f.unitsPerEm;
	const tracking = (o.tracking ?? 0) * o.size;
	let pen = x;
	let d = '';
	for (let i = 0; i < run.glyphs.length; i++) {
		const g = run.glyphs[i];
		const p = run.positions[i];
		const ox = pen + p.xOffset * s;
		const oy = y - p.yOffset * s;
		for (const c of g.path.commands) {
			const a = c.args;
			const pt = (j) => `${n(ox + a[j] * s)} ${n(oy - a[j + 1] * s)}`;
			if (c.command === 'moveTo') d += 'M' + pt(0);
			else if (c.command === 'lineTo') d += 'L' + pt(0);
			else if (c.command === 'quadraticCurveTo') d += 'Q' + pt(0) + ' ' + pt(2);
			else if (c.command === 'bezierCurveTo') d += 'C' + pt(0) + ' ' + pt(2) + ' ' + pt(4);
			else if (c.command === 'closePath') d += 'Z';
		}
		pen += p.xAdvance * s + tracking;
	}
	return d;
}

/**
 * Greedy word wrap to `maxWidth`; returns at most `maxLines` lines, ellipsising the last one.
 * @param {string} text @param {number} maxWidth @param {number} maxLines @param {TextOptions} o
 */
export function wrap(text, maxWidth, maxLines, o) {
	const words = text.split(/\s+/).filter(Boolean);
	const lines = [];
	let line = '';
	for (const word of words) {
		const next = line ? line + ' ' + word : word;
		if (measure(next, o) <= maxWidth || !line) line = next;
		else {
			lines.push(line);
			line = word;
		}
	}
	if (line) lines.push(line);
	if (lines.length <= maxLines) return lines;
	const kept = lines.slice(0, maxLines);
	let last = kept[maxLines - 1] + '\u2026';
	while (measure(last, o) > maxWidth && last.length > 1) last = last.slice(0, -2).trimEnd() + '\u2026';
	kept[maxLines - 1] = last;
	return kept;
}

/** @param {TextOptions} o */
function axes(o) {
	const a = { wght: o.weight ?? 400 };
	if ((o.family ?? 'sans') === 'sans') a.wdth = o.width ?? 100;
	return a;
}
