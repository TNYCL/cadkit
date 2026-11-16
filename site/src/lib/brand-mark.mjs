// Geometry of the cadkit mark: the "C" drawn as a CAD arc entity in its selected state.
// A 270 degree monoline arc opening to the right, square grips at both ends and at the arc
// midpoint, and a centre mark. Shared by the inline header logo (Logo.astro), the brand asset
// script (scripts/brand.mjs) and the Open Graph cards (scripts/og.mjs).
//
// The centre mark is drawn the way drafting software marks the centre of an arc: a centre
// point with four short, thin dashes held away from it by an open gap. The gap, the dashes
// being thinner than the arc and the arc colour (not the grip accent) keep it from reading as a
// plus sign. At favicon size only the centre point remains.
//
// Coordinates use a 32 x 32 design grid, y down. The arc ends sit on whole units (23, 9) and
// (23, 23), so the end grips stay pixel-aligned at 16, 32 and 64 px.

export const GRID = 32;
const CX = 16;
const CY = 16;
/** Radius that puts the arc ends at 45 degrees on whole grid units: 7 * sqrt(2). */
export const RADIUS = 7 * Math.SQRT2;

const n = (v) => String(Math.round(v * 1000) / 1000);

/**
 * @typedef {object} Weight
 * @property {number} stroke arc stroke width
 * @property {number} grip grip square side
 * @property {number} dot radius of the centre point
 * @property {null | {from: number, to: number, width: number}} dashes centre-mark dashes
 *   (distance from the centre where each dash starts and ends, and its thickness), or none
 */

/**
 * Weights for each use, in grid units:
 * - "ui": 20-64 px inline use (header, footer, lockups, OG cards);
 * - "tile": the navy tile at 128 px and above (avatar, README icon, apple-touch icon);
 * - "icon": favicons (crisp at 16 px): heavier arc, centre point only.
 * @type {Record<'ui' | 'tile' | 'icon', Weight>}
 */
export const WEIGHTS = {
	ui: { stroke: 2.25, grip: 4.5, dot: 1.25, dashes: { from: 3, to: 6, width: 1 } },
	tile: { stroke: 3, grip: 5.5, dot: 1.5, dashes: { from: 3.25, to: 6, width: 1.25 } },
	icon: { stroke: 3, grip: 5.5, dot: 1.75, dashes: null },
};

/**
 * The centre mark (centre point and dashes) as SVG elements.
 * @param {string} paint
 * @param {Weight} w
 */
export function centreMarkElements(paint, w) {
	let out = `<circle cx="${CX}" cy="${CY}" r="${n(w.dot)}" fill="${paint}"/>`;
	if (w.dashes) {
		const { from, to, width } = w.dashes;
		const len = to - from;
		const h = width / 2;
		const rects = [
			[CX - to, CY - h, len, width],
			[CX + from, CY - h, len, width],
			[CX - h, CY - to, width, len],
			[CX - h, CY + from, width, len],
		];
		out += rects
			.map(([x, y, rw, rh]) => `<rect x="${n(x)}" y="${n(y)}" width="${n(rw)}" height="${n(rh)}" fill="${paint}"/>`)
			.join('');
	}
	return out;
}

/**
 * Returns the mark as SVG elements (no <svg> wrapper).
 * @param {{arc: string, grip: string, weight?: 'ui' | 'tile' | 'icon'}} paint colours (any SVG
 *   paint, e.g. "currentColor", "#5ce1e6" or "var(--ck-accent)"). The centre mark uses the arc
 *   colour.
 */
export function markElements({ arc, grip, weight = 'ui' }) {
	const w = WEIGHTS[weight];
	const ex = CX + 7;
	const top = CY - 7;
	const bottom = CY + 7;
	const mid = CX - RADIUS;
	const h = w.grip / 2;
	const square = (x, y) =>
		`<rect x="${n(x - h)}" y="${n(y - h)}" width="${n(w.grip)}" height="${n(w.grip)}" fill="${grip}"/>`;
	return [
		`<path d="M${ex} ${top}A${n(RADIUS)} ${n(RADIUS)} 0 1 0 ${ex} ${bottom}" fill="none" stroke="${arc}" stroke-width="${w.stroke}" stroke-linecap="butt"/>`,
		centreMarkElements(arc, w),
		square(ex, top),
		square(mid, CY),
		square(ex, bottom),
	].join('');
}

/**
 * Horizontal offset that centres the mark's visual bounds in the 32-unit grid (the arc opens to
 * the right, so its bounds are not symmetric).
 * @param {'ui' | 'tile' | 'icon'} weight
 */
export function opticalOffsetX(weight = 'ui') {
	const h = WEIGHTS[weight].grip / 2;
	const left = CX - RADIUS - h;
	const right = CX + 7 + h;
	return GRID / 2 - (left + right) / 2;
}
