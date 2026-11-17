// Local origin for the SVG the engine returns (unit-tested in test/demo-origin.test.mjs).
//
// Browsers process SVG coordinates as 32-bit floats, which resolve about 7 significant digits,
// and cadkit writes absolute drawing coordinates. A plan 16 units wide at x = 512,345 has only
// a few float32 steps per unit, so its lines snap together. The worker therefore rewrites the
// numbers relative to an integer origin and tells the page, which adds it back wherever sheet
// and drawing coordinates meet.
//
// The rewrite knows exactly the markup the engine emits (catalogued over the bundled samples
// and several hundred real drawings):
//   svg viewBox, the background rect x/y, path d with absolute M, L, A and Z only, text x/y and
//   rotate(angle cx cy), tspan x/y (dy is relative), and hatch patterns in defs whose lines are
//   pattern-local; colours, widths, dash arrays and font sizes carry no position.
// Anything else (another element, attribute, path command or number syntax) leaves the SVG
// untouched and says why: a drawing is either rewritten completely or not at all.
// Keep this file free of non-erasable TypeScript syntax: Node imports it directly.

import { MAX_ZOOM_IN, type Box, type Offset } from './view.ts';

/** A local origin in SVG user units (drawing X, and drawing Y negated): absolute = local + origin. */
export type Origin = Offset;

export const NO_ORIGIN: Origin = { x: 0, y: 0 };

export interface LocalSvg {
	/** The SVG, rewritten relative to `origin` (or unchanged when `origin` is zero). */
	svg: string;
	origin: Origin;
	/**
	 * Extents of the raster image frames in the coordinates of `svg`, or null when there are
	 * none. cadkit-wasm 0.5 leaves the frames out of the viewBox, so a fit to the viewBox ignores
	 * them; the page adds this box when it shows them.
	 */
	frames: Box | null;
	/** Raster image frames (paths inside `<g data-kind="images">`). */
	imageFrames: number;
	/** Set when a rewrite was needed but the markup was not what the engine emits. */
	problem: string | null;
}

/** Widest stage the decision assumes (CSS pixels times device pixel ratio). */
export const REF_STAGE_PX = 4096;
/** Largest float32 rounding error, in pixels at maximum zoom, that needs no rewrite. */
export const MAX_ERROR_PX = 0.1;

const NUM = /^-?\d+(?:\.\d+)?$/;
const F32_HALF_ULP = 2 ** -24;

/** Parses "x y w h" (the engine writes four plain numbers); null otherwise. */
function viewBoxOf(value: string): Box | null {
	const n = value.split(' ');
	if (n.length !== 4 || !n.every((t) => NUM.test(t))) return null;
	const [x, y, w, h] = n.map(Number) as [number, number, number, number];
	return w > 0 && h > 0 ? { x, y, w, h } : null;
}

/**
 * The origin the sheet with this viewBox should be drawn from, or zero when its coordinates are
 * fine as they are. Coordinates need moving when float32 rounding at their magnitude exceeds
 * MAX_ERROR_PX at the deepest zoom (MAX_ZOOM_IN over the fitted view on a REF_STAGE_PX stage)
 * and moving shrinks them at least fourfold; below that the error comes from the drawing's own
 * size, which no origin removes. The origin is a multiple of a power of ten no larger than the
 * extent (an integer), so the local numbers stay below twice the extent.
 *
 * With MAX_ZOOM_IN = 1e5 the first test never passes: a pixel at that zoom is 2.4e-9 of the
 * extent, finer than float32 resolves even at the centre of a drawing around zero (half the
 * extent away, 3e-8 of it). The fourfold test decides alone: a sheet whose farthest viewBox
 * corner is more than four times as far from zero as it would be from the origin moves (a 5 m
 * detail at x = 6,000,000 does, and so does a 100-unit plan at x = 500, which is harmless:
 * the rewrite is exact); a sheet around zero or a wide map near it stays.
 */
export function chooseOrigin(vb: Box): Origin {
	const extent = Math.max(vb.w, vb.h);
	if (!(extent > 0) || !Number.isFinite(extent) || !Number.isFinite(vb.x) || !Number.isFinite(vb.y)) return NO_ORIGIN;
	const grid = Math.max(1, 10 ** Math.floor(Math.log10(extent)));
	const o = { x: Math.floor(vb.x / grid) * grid, y: Math.floor(vb.y / grid) * grid };
	const reach = (ox: number, oy: number) => Math.max(Math.abs(vb.x - ox), Math.abs(vb.x + vb.w - ox), Math.abs(vb.y - oy), Math.abs(vb.y + vb.h - oy));
	const before = reach(0, 0);
	const after = reach(o.x, o.y);
	const pixel = extent / (REF_STAGE_PX * MAX_ZOOM_IN);
	if ((before * F32_HALF_ULP) / pixel <= MAX_ERROR_PX || before <= 4 * after) return NO_ORIGIN;
	return o;
}

class Unexpected extends Error {}
const unexpected = (what: string): never => {
	throw new Unexpected(what);
};

/** Decimals a moved number may keep: toFixed takes at most 100, and the engine writes few. */
const MAX_DECIMALS = 20;

/**
 * One number moved by an integer offset, keeping the decimals it was written with. A number
 * the plain notation cannot carry after the move (too many decimals, or 1e21 and more, which
 * toFixed writes with an exponent) is markup the rewrite does not expect.
 */
function shift(token: string, by: number): string {
	if (!by) return token;
	const dot = token.indexOf('.');
	const decimals = dot < 0 ? 0 : token.length - dot - 1;
	if (decimals > MAX_DECIMALS) unexpected('number with ' + decimals + ' decimals');
	let s = (Number(token) - by).toFixed(decimals);
	if (!NUM.test(s)) unexpected('number ' + s);
	if (decimals) s = s.replace(/\.?0+$/, '');
	return s === '-0' ? '0' : s;
}

/** Attributes per element: `keep` carry no position; the others are rewritten by the scanner. */
const KEEP: Record<string, ReadonlySet<string>> = {
	svg: new Set(['xmlns', 'width', 'height']),
	rect: new Set(['width', 'height', 'fill']),
	defs: new Set(),
	pattern: new Set(['id', 'width', 'height']),
	line: new Set(['x1', 'y1', 'x2', 'y2', 'stroke', 'stroke-width', 'vector-effect']),
	g: new Set(['data-layer', 'data-kind', 'fill', 'stroke-width', 'stroke-linecap', 'stroke-linejoin']),
	path: new Set(['fill', 'fill-rule', 'stroke', 'vector-effect', 'stroke-width', 'stroke-dasharray']),
	text: new Set(['font-size', 'text-anchor', 'dominant-baseline', 'fill', 'textLength', 'lengthAdjust']),
	tspan: new Set(['dy']),
};
/** Where each element may appear. */
const PARENT: Record<string, string> = { svg: '', rect: 'svg', defs: 'svg', pattern: 'defs', line: 'pattern', g: 'svg', path: 'g', text: 'g', tspan: 'text' };

const TAG = /<(\/?)([a-zA-Z]+)((?:\s+[a-zA-Z][\w:-]*="[^"<]*")*)\s*(\/?)>|[^<]+/y;
const ATTR = /\s+([a-zA-Z][\w:-]*)="([^"<]*)"/g;
const ROTATE3 = /^rotate\((-?\d+(?:\.\d+)?) (-?\d+(?:\.\d+)?) (-?\d+(?:\.\d+)?)\)$/;
const ROTATE1 = /^rotate\((-?\d+(?:\.\d+)?)\)$/;
const PATH_PART = /([MLAZ])([^MLAZ]*)/y;

/** Extents of points, in the coordinates of the rewritten SVG. */
class Extents {
	x0 = Infinity;
	y0 = Infinity;
	x1 = -Infinity;
	y1 = -Infinity;
	add(x: number, y: number): void {
		if (x < this.x0) this.x0 = x;
		if (x > this.x1) this.x1 = x;
		if (y < this.y0) this.y0 = y;
		if (y > this.y1) this.y1 = y;
	}
	box(): Box | null {
		if (!(this.x1 >= this.x0) || !(this.y1 >= this.y0)) return null;
		return { x: this.x0, y: this.y0, w: this.x1 - this.x0, h: this.y1 - this.y0 };
	}
}

/** Rewrites one path `d`; `ext`, when given, collects its end points (after the move). */
function pathData(d: string, o: Origin, ext: Extents | null): string {
	let out = '';
	let pos = 0;
	while (pos < d.length) {
		PATH_PART.lastIndex = pos;
		const m = PATH_PART.exec(d);
		if (!m) unexpected('path data');
		pos = PATH_PART.lastIndex;
		const cmd = m![1]!;
		const rest = m![2]!.trim();
		const nums = rest ? rest.split(' ') : [];
		if (!nums.every((t) => NUM.test(t))) unexpected('path data');
		if (cmd === 'Z') {
			if (nums.length) unexpected('path data');
			out += 'Z';
			continue;
		}
		const step = cmd === 'A' ? 7 : 2;
		if (!nums.length || nums.length % step) unexpected('path data');
		const parts: string[] = [];
		for (let i = 0; i < nums.length; i += step) {
			const xs = shift(nums[i + step - 2]!, o.x);
			const ys = shift(nums[i + step - 1]!, o.y);
			if (cmd === 'A') {
				const flags = [nums[i + 3], nums[i + 4]];
				if (!flags.every((f) => f === '0' || f === '1')) unexpected('arc flags');
				parts.push(nums[i]!, nums[i + 1]!, nums[i + 2]!, flags[0]!, flags[1]!, xs, ys);
			} else parts.push(xs, ys);
			if (ext) ext.add(Number(xs), Number(ys));
		}
		out += cmd + parts.join(' ');
	}
	return out;
}

const mod = (a: number, b: number) => ((a % b) + b) % b;

/**
 * Moves an engine SVG to a local origin. `origin` null chooses one from the viewBox
 * (chooseOrigin); an explicit origin is applied as given (the one-entity highlight must use the
 * sheet's). The viewBox and every position move; hatch patterns get an x/y offset so their
 * lines keep their phase against the absolute grid the engine drew them on.
 */
export function localizeSvg(svg: string, origin: Origin | null = null): LocalSvg {
	const untouched = (problem: string | null): LocalSvg => ({ svg, origin: NO_ORIGIN, frames: null, imageFrames: 0, problem });
	const root = /^<svg\b[^>]*\sviewBox="([^"]*)"/.exec(svg);
	const vb = root ? viewBoxOf(root[1]!) : null;
	if (!vb) return untouched(origin ? 'no viewBox' : null);
	const o = origin ?? chooseOrigin(vb);
	const images = svg.includes('<g data-kind="images"');
	const moving = o.x !== 0 || o.y !== 0;
	if (!moving && !images) return untouched(null);

	const out: string[] = [];
	const stack: string[] = [];
	const ext = images ? new Extents() : null;
	let inImages = -1;
	let imageFrames = 0;
	try {
		let pos = 0;
		while (pos < svg.length) {
			TAG.lastIndex = pos;
			const m = TAG.exec(svg);
			if (!m) unexpected('markup at ' + pos);
			pos = TAG.lastIndex;
			const [whole, close, tag, attrs, selfClose] = m!;
			if (tag === undefined) {
				// Character data: only text and tspan contents, or whitespace between elements.
				const top = stack[stack.length - 1];
				if (top !== 'text' && top !== 'tspan' && whole.trim()) unexpected('text outside text');
				out.push(whole);
				continue;
			}
			if (close) {
				if (attrs || selfClose || stack.pop() !== tag) unexpected('</' + tag + '>');
				if (inImages === stack.length) inImages = -1;
				out.push(whole);
				continue;
			}
			const keep = KEEP[tag!];
			if (!keep || PARENT[tag!] !== (stack[stack.length - 1] ?? '')) unexpected('<' + tag + '>');
			const a: [string, string][] = [];
			const seen = new Set<string>();
			for (const [, name, value] of attrs!.matchAll(ATTR)) {
				if (seen.has(name!)) unexpected('repeated ' + name);
				seen.add(name!);
				a.push([name!, value!]);
			}
			const moved = a.map(([name, value]): [string, string] => {
				if (keep.has(name)) {
					if (tag === 'g' && name === 'data-kind' && value !== 'images') unexpected('data-kind ' + value);
					return [name, value];
				}
				const xy = (axis: 'x' | 'y'): string => {
					if (!NUM.test(value)) unexpected(tag + ' ' + name);
					return shift(value, axis === 'x' ? o.x : o.y);
				};
				if (tag === 'svg' && name === 'viewBox') {
					const [x, y, w, h] = value.split(' ');
					if (!viewBoxOf(value)) unexpected('viewBox');
					return [name, `${shift(x!, o.x)} ${shift(y!, o.y)} ${w} ${h}`];
				}
				if ((tag === 'rect' || tag === 'text' || tag === 'tspan') && (name === 'x' || name === 'y')) return [name, xy(name)];
				if (tag === 'path' && name === 'd') return [name, pathData(value, o, ext && inImages >= 0 ? ext : null)];
				if (tag === 'text' && name === 'transform') {
					const r = ROTATE3.exec(value);
					if (!r) unexpected('text transform');
					return [name, `rotate(${r![1]} ${shift(r![2]!, o.x)} ${shift(r![3]!, o.y)})`];
				}
				if (tag === 'pattern' && name === 'patternUnits') {
					if (value !== 'userSpaceOnUse') unexpected('patternUnits ' + value);
					return [name, value];
				}
				if (tag === 'pattern' && name === 'patternTransform') {
					if (!ROTATE1.test(value)) unexpected('patternTransform');
					return [name, value];
				}
				return unexpected(tag + ' ' + name);
			});
			if (tag === 'pattern' && moving) {
				// The tiles sat on the absolute grid through (0, 0); keep them there: put a tile corner
				// at the old absolute origin, which is -origin now (in pattern space, before the rotation).
				const w = Number(a.find(([n]) => n === 'width')?.[1]);
				const h = Number(a.find(([n]) => n === 'height')?.[1]);
				const rot = ROTATE1.exec(a.find(([n]) => n === 'patternTransform')?.[1] ?? 'rotate(0)');
				if (!(w > 0) || !(h > 0) || !rot) unexpected('pattern size');
				const t = (-Number(rot![1]) * Math.PI) / 180;
				const ux = Math.cos(t) * -o.x - Math.sin(t) * -o.y;
				const uy = Math.sin(t) * -o.x + Math.cos(t) * -o.y;
				moved.push(['x', String(Number(mod(ux, w).toFixed(9)))], ['y', String(Number(mod(uy, h).toFixed(9)))]);
			}
			if (tag === 'g' && a.some(([n, v]) => n === 'data-kind' && v === 'images')) inImages = stack.length;
			if (tag === 'path' && inImages >= 0) imageFrames++;
			out.push('<' + tag + moved.map(([n, v]) => ` ${n}="${v}"`).join('') + (selfClose ? '/>' : '>'));
			if (!selfClose) stack.push(tag!);
		}
		if (stack.length) unexpected('unclosed <' + stack[stack.length - 1] + '>');
	} catch (e) {
		if (e instanceof Unexpected) return untouched(moving ? 'unexpected ' + e.message : null);
		throw e;
	}
	return { svg: moving ? out.join('') : svg, origin: o, frames: ext?.box() ?? null, imageFrames, problem: null };
}
