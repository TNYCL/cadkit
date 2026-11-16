// Pan and zoom state for the demo stage, independent of the DOM (unit-tested in test/demo-view.test.mjs).
//
// The SVG from cadkit uses drawing units in its viewBox with Y flipped (svg_y = -world_y). The view
// keeps a centre (cx, cy) in SVG user units and a scale s in user units per CSS pixel; the stage
// sets its viewBox from them so that the drawing keeps its aspect ratio at any stage size.

export interface Box {
	x: number;
	y: number;
	w: number;
	h: number;
}

export interface View {
	cx: number;
	cy: number;
	/** SVG user units per CSS pixel. */
	s: number;
	/** Extents of the drawing (the SVG's own viewBox). */
	home: Box;
}

/** Padding around the drawing when fitting, as a fraction of the stage. */
export const FIT_MARGIN = 0.04;
/** Zoom range relative to the fitted scale. */
export const MAX_ZOOM_IN = 1e5;
export const MAX_ZOOM_OUT = 50;

/** Parses an SVG viewBox attribute; falls back to a unit box for missing or invalid values. */
export function parseViewBox(value: string | null): Box {
	const n = (value ?? '').trim().split(/[\s,]+/).map(Number);
	if (n.length === 4 && n.every(Number.isFinite) && n[2]! > 0 && n[3]! > 0) return { x: n[0]!, y: n[1]!, w: n[2]!, h: n[3]! };
	return { x: 0, y: 0, w: 1, h: 1 };
}

/** Scale that fits the home box into a w x h stage. */
export function fitScale(home: Box, w: number, h: number): number {
	const sw = Math.max(1, w) * (1 - 2 * FIT_MARGIN);
	const sh = Math.max(1, h) * (1 - 2 * FIT_MARGIN);
	return Math.max(home.w / sw, home.h / sh);
}

export function fit(home: Box, w: number, h: number): View {
	return { cx: home.x + home.w / 2, cy: home.y + home.h / 2, s: fitScale(home, w, h), home };
}

/** viewBox string for a w x h stage. */
export function viewBoxOf(v: View, w: number, h: number): string {
	const vw = Math.max(1, w) * v.s;
	const vh = Math.max(1, h) * v.s;
	return `${v.cx - vw / 2} ${v.cy - vh / 2} ${vw} ${vh}`;
}

/** Stage pixel (px, py from the stage's top-left) to SVG user units. */
export function toSvg(v: View, w: number, h: number, px: number, py: number): { x: number; y: number } {
	return { x: v.cx + (px - w / 2) * v.s, y: v.cy + (py - h / 2) * v.s };
}

/** Stage pixel to drawing (world) coordinates: Y is flipped back. */
export function toWorld(v: View, w: number, h: number, px: number, py: number): { x: number; y: number } {
	const p = toSvg(v, w, h, px, py);
	return { x: p.x, y: -p.y };
}

/** Pans by a pixel delta (content follows the pointer). */
export function pan(v: View, dx: number, dy: number): View {
	return { ...v, cx: v.cx - dx * v.s, cy: v.cy - dy * v.s };
}

/** Zooms by factor (> 1 zooms in) keeping the point under (px, py) fixed; clamped to the zoom range. */
export function zoomAt(v: View, w: number, h: number, factor: number, px = w / 2, py = h / 2): View {
	const base = fitScale(v.home, w, h);
	const s = Math.min(Math.max(v.s / factor, base / MAX_ZOOM_IN), base * MAX_ZOOM_OUT);
	const p = toSvg(v, w, h, px, py);
	return { ...v, s, cx: p.x - (px - w / 2) * s, cy: p.y - (py - h / 2) * s };
}

/** Decimal places that resolve about one pixel at scale s, for coordinate readouts. */
export function decimalsFor(s: number): number {
	if (!(s > 0) || !Number.isFinite(s)) return 3;
	return Math.max(0, Math.min(8, Math.ceil(-Math.log10(s))));
}

/** A coordinate formatted for the readout at scale s (fixed decimals, no "-0"). */
export function formatCoord(value: number, s: number): string {
	const d = decimalsFor(s);
	const text = value.toFixed(d);
	return /^-0(\.0+)?$/.test(text) ? text.slice(1) : text;
}
