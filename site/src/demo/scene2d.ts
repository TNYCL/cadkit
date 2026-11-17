// Geometry helpers for the 2D sheet that need no DOM (unit-tested in test/demo-scene.test.mjs):
// point markers, paper-space layouts and the measure tool.

import type { Box } from './view.ts';

// ---------------------------------------------------------------------------------------------
// Point markers.

const CROSS = /^M(-?[\d.]+(?:e-?\d+)?) (-?[\d.]+(?:e-?\d+)?)L(-?[\d.]+(?:e-?\d+)?) (-?[\d.]+(?:e-?\d+)?)M(-?[\d.]+(?:e-?\d+)?) (-?[\d.]+(?:e-?\d+)?)L(-?[\d.]+(?:e-?\d+)?) (-?[\d.]+(?:e-?\d+)?)$/;

/**
 * cadkit-wasm 0.5 draws a POINT as one path of two crossing segments whose length follows the
 * drawing extents (0.4 % of the view), so on a wide drawing a point becomes a cross many
 * units long. This recognises exactly that shape (a horizontal and a vertical segment of equal
 * length crossing at their midpoints) and returns its centre in SVG units, so the page can draw
 * a marker of constant pixel size instead. Any other path returns null.
 */
export function pointCross(d: string): { x: number; y: number } | null {
	const m = CROSS.exec(d);
	if (!m) return null;
	const [x1, y1, x2, y2, x3, y3, x4, y4] = m.slice(1).map(Number) as [number, number, number, number, number, number, number, number];
	if (y1 !== y2 || x3 !== x4) return null;
	const lh = Math.abs(x2 - x1);
	const lv = Math.abs(y4 - y3);
	if (!(lh > 0) || !(lv > 0)) return null;
	// Coordinates are rounded to a few decimals, so compare with a tolerance of 1 % of the length.
	const eps = 0.01 * Math.max(lh, lv);
	if (Math.abs(lh - lv) > eps) return null;
	if (Math.abs((x1 + x2) / 2 - x3) > eps || Math.abs((y3 + y4) / 2 - y1) > eps) return null;
	return { x: x3, y: y1 };
}

// ---------------------------------------------------------------------------------------------
// Paper-space layouts.

/** A viewport entity of the neutral document, in drawing units (paper centre and size, model view). */
export interface Viewport {
	/** Centre on the sheet. */
	cx: number;
	cy: number;
	/** Size on the sheet. */
	w: number;
	h: number;
	/** Centre of the model area it shows. */
	vcx: number;
	vcy: number;
	/** Height of the model area it shows (the width follows the viewport's aspect ratio). */
	vh: number;
}

/** A viewport placed on the sheet: SVG rectangle (Y flipped) and the model viewBox it shows. */
export interface Frame {
	x: number;
	y: number;
	w: number;
	h: number;
	/** viewBox of the model-space SVG shown inside the rectangle. */
	viewBox: Box;
	/** Model units per paper unit. */
	scale: number;
}

export interface LayoutPlan {
	/** Extents of the sheet content in SVG units: every frame, plus the sheet's own entities. */
	home: Box;
	frames: Frame[];
}

const usable = (v: Viewport) => [v.cx, v.cy, v.w, v.h, v.vcx, v.vcy, v.vh].every(Number.isFinite) && v.w > 0 && v.h > 0 && v.vh > 0;

/**
 * True for the viewport that stands for the sheet itself: in DWG and DXF the first viewport of
 * a layout shows paper space at 1:1 around its own centre. It is not a window into the model.
 */
export function isSheetViewport(v: Viewport): boolean {
	const eps = 1e-6 * Math.max(1, Math.abs(v.h));
	return Math.abs(v.vh - v.h) <= eps && Math.abs(v.vcx - v.cx) <= eps && Math.abs(v.vcy - v.cy) <= eps;
}

/**
 * Places the model viewports of a layout. `own` is the bounding box of the sheet's own
 * entities (title block, notes) in SVG units when it has any. Returns null when the layout has
 * no viewport that shows the model.
 */
export function planLayout(viewports: Viewport[], own: Box | null): LayoutPlan | null {
	const list = viewports.filter(usable);
	const windows = list.filter((v, i) => !(i === 0 && isSheetViewport(v)));
	if (!windows.length) return null;
	const frames = windows.map((v): Frame => {
		const scale = v.vh / v.h;
		const vw = v.w * scale;
		return {
			x: v.cx - v.w / 2,
			y: -(v.cy + v.h / 2),
			w: v.w,
			h: v.h,
			viewBox: { x: v.vcx - vw / 2, y: -(v.vcy + v.vh / 2), w: vw, h: v.vh },
			scale,
		};
	});
	let x0 = Infinity;
	let y0 = Infinity;
	let x1 = -Infinity;
	let y1 = -Infinity;
	for (const b of own ? [...frames, own] : frames) {
		x0 = Math.min(x0, b.x);
		y0 = Math.min(y0, b.y);
		x1 = Math.max(x1, b.x + b.w);
		y1 = Math.max(y1, b.y + b.h);
	}
	return { home: { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, frames };
}

/**
 * The model point under a sheet point (both in drawing coordinates, Y up), or null when the
 * point is outside every frame. Later frames lie on top, so they are tested first.
 */
export function paperToModel(plan: LayoutPlan, x: number, y: number): { x: number; y: number; scale: number } | null {
	const sy = -y;
	for (let i = plan.frames.length - 1; i >= 0; i--) {
		const f = plan.frames[i]!;
		if (x < f.x || x > f.x + f.w || sy < f.y || sy > f.y + f.h) continue;
		const mx = f.viewBox.x + ((x - f.x) / f.w) * f.viewBox.w;
		const my = f.viewBox.y + ((sy - f.y) / f.h) * f.viewBox.h;
		return { x: mx, y: -my, scale: f.scale };
	}
	return null;
}

// ---------------------------------------------------------------------------------------------
// Measure tool.

export interface Pt {
	x: number;
	y: number;
}

/** Length of the open path through the points, in drawing units. */
export function pathLength(pts: Pt[]): number {
	let n = 0;
	for (let i = 1; i < pts.length; i++) n += Math.hypot(pts[i]!.x - pts[i - 1]!.x, pts[i]!.y - pts[i - 1]!.y);
	return n;
}

/**
 * Area enclosed by the points taken as a closed polygon (shoelace), in square drawing units.
 * Computed relative to the first point: georeferenced coordinates are millions of units from
 * zero, where the products of the plain formula lose the digits the area needs.
 */
export function polygonArea(pts: Pt[]): number {
	if (pts.length < 3) return 0;
	const o = pts[0]!;
	let a = 0;
	for (let i = 0; i < pts.length; i++) {
		const p = pts[i]!;
		const q = pts[(i + 1) % pts.length]!;
		a += (p.x - o.x) * (q.y - o.y) - (q.x - o.x) * (p.y - o.y);
	}
	return Math.abs(a) / 2;
}

/** A length or area with a sensible number of significant digits ("12.35", "0.0042", "1,250"). */
export function formatMeasure(value: number): string {
	if (!Number.isFinite(value)) return '-';
	const abs = Math.abs(value);
	if (abs === 0) return '0';
	if (abs >= 1000) return value.toLocaleString('en-US', { maximumFractionDigits: 1 });
	if (abs >= 1) return value.toFixed(abs >= 100 ? 2 : 3).replace(/\.?0+$/, '');
	return value.toPrecision(3).replace(/\.?0+$/, '');
}
