// The 2D sheet: the SVG that cadkit renders, prepared for the page. It removes anything active
// from the markup, composes paper-space layouts from their viewports, draws point markers at a
// constant pixel size, restyles colours for the dark sheet and carries the overlays (selection
// highlight, measure path). viewer.ts owns the view (pan and zoom) and the input.

import { displayColor, parseHex, SHEET_BG, type Sheet } from './colors.ts';
import { s, SVG_NS } from './dom.ts';
import { DemoError } from './engine.ts';
import type { RenderResult } from './protocol.ts';
import { planLayout, pointCross, type LayoutPlan, type Pt } from './scene2d.ts';
import { parseViewBox, toLocal, unionBox, type Box, type Offset } from './view.ts';

export interface Loaded {
	/** Extents to fit (sheet units, relative to `origin`): the drawing, or the sheet content of a layout. Raster image frames are left out. */
	home: Box;
	/** `home` with the raster image frames of the sheet itself, for when they are shown. */
	homeWithImages: Box;
	/** Local origin of the sheet: absolute SVG coordinates = sheet coordinates + origin. */
	origin: Offset;
	/** Local origin of the model shown through a layout's viewports. */
	sourceOrigin: Offset;
	/** Set when the model is a paper-space layout shown through its viewports (absolute drawing units). */
	layout: LayoutPlan | null;
	/** Raster image frames drawn (the sheet's own and those seen through viewports). */
	imageFrames: number;
	/** The model has viewports but none of them shows the model. */
	noWindows: boolean;
	/** Nothing at all was drawn. */
	empty: boolean;
	/** SVG elements in the page for this model. */
	nodes: number;
	/** Point markers redrawn at a constant size. */
	points: number;
}

interface Painted {
	el: Element;
	stroke: string | null;
	fill: string | null;
}

const SOURCE_ID = 'ck-source';
const XLINK = 'http://www.w3.org/1999/xlink';

function parseSvg(text: string): SVGSVGElement {
	const parsed = new DOMParser().parseFromString(text, 'image/svg+xml');
	const node = parsed.documentElement;
	if (node.localName !== 'svg' || parsed.getElementsByTagName('parsererror').length) {
		throw new DemoError('internal', 'the renderer returned SVG the browser could not parse');
	}
	// The markup comes from a file the visitor opened: keep only what draws.
	for (const bad of node.querySelectorAll('script, foreignObject, iframe, object, embed, a, style, animate, set, animateTransform, animateMotion')) bad.remove();
	for (const el of [node, ...node.querySelectorAll('*')]) {
		for (const attr of [...el.attributes]) if (/^on/i.test(attr.name)) el.removeAttribute(attr.name);
		const href = el.getAttribute('href') ?? el.getAttributeNS(XLINK, 'href');
		if (href !== null && !href.startsWith('#') && !href.startsWith('data:image/')) el.remove();
	}
	return node as unknown as SVGSVGElement;
}

/** Removes the engine's background rectangle: the stage paints the sheet colour itself. */
function dropBackground(node: SVGSVGElement): void {
	const first = node.firstElementChild;
	if (first?.localName === 'rect' && first.getAttribute('fill') === SHEET_BG.light && !first.hasAttribute('data-layer')) first.remove();
}

/** Redraws the engine's point crosses as dots of constant pixel size; returns how many. */
function fixPoints(root: Element, unit: number): number {
	let n = 0;
	for (const path of root.querySelectorAll('path')) {
		const c = pointCross(path.getAttribute('d') ?? '');
		if (!c) continue;
		// A very short segment with round caps: a dot whose diameter is the stroke width in pixels.
		path.setAttribute('d', `M${c.x} ${c.y}h${unit}`);
		path.setAttribute('class', 'ck-pt');
		path.removeAttribute('stroke-dasharray');
		n++;
	}
	return n;
}

export class Sheet2d {
	svg: SVGSVGElement | null = null;
	layout: LayoutPlan | null = null;
	private groups = new Map<string, SVGGElement[]>();
	private painted: Painted[] = [];
	private sheet: Sheet = 'light';
	private unit = 1e-6;
	private highlight: SVGGElement | null = null;
	private box: SVGRectElement | null = null;
	private measure: SVGGElement | null = null;
	private origin: Offset = { x: 0, y: 0 };
	private sourceOrigin: Offset = { x: 0, y: 0 };
	private images: SVGGElement[] = [];

	constructor(private readonly host: HTMLElement) {}

	clear(): void {
		this.host.replaceChildren();
		this.svg = null;
		this.layout = null;
		this.groups = new Map();
		this.painted = [];
		this.highlight = null;
		this.box = null;
		this.measure = null;
		this.origin = { x: 0, y: 0 };
		this.sourceOrigin = { x: 0, y: 0 };
		this.images = [];
	}

	/** Names of the layers that have something drawn. */
	layerNames(): string[] {
		return [...this.groups.keys()];
	}

	/**
	 * Builds the sheet from a render result and shows it; throws DemoError on unusable SVG. The
	 * SVG is relative to `result.origin`, the source of a layout to its own origin; viewports
	 * come in drawing units and are placed by subtracting those origins.
	 */
	load(result: RenderResult, sheet: Sheet, translucentFaces: boolean, showImages: boolean): Loaded {
		this.clear();
		this.sheet = sheet;
		const origin = result.origin;
		const sourceOrigin = result.layout?.sourceOrigin ?? { x: 0, y: 0 };
		const node = parseSvg(result.svg);
		dropBackground(node);
		const ownEmpty = !node.querySelector('g[data-layer]');
		// cadkit-wasm 0.5 leaves raster image frames out of the viewBox (see origin.ts).
		let home = parseViewBox(node.getAttribute('viewBox'));
		let homeWithImages = result.frames ? unionBox(home, result.frames) : home;
		let layout: LayoutPlan | null = null;
		let noWindows = false;
		if (result.layout) {
			const abs = { ...home, x: home.x + origin.x, y: home.y + origin.y };
			layout = planLayout(result.layout.viewports, ownEmpty ? null : abs);
			noWindows = !layout;
		}
		if (layout && result.layout) {
			const source = parseSvg(result.layout.sourceSvg);
			dropBackground(source);
			// Both documents number their hatch patterns from 1: give the source its own ids.
			for (const el of source.querySelectorAll('[id]')) el.setAttribute('id', 'ck-src-' + el.getAttribute('id'));
			for (const el of source.querySelectorAll('[fill^="url(#"], [stroke^="url(#"]')) {
				for (const a of ['fill', 'stroke']) {
					const v = el.getAttribute(a);
					if (v?.startsWith('url(#')) el.setAttribute(a, v.replace('url(#', 'url(#ck-src-'));
				}
			}
			const doc = node.ownerDocument;
			let defs = node.querySelector(':scope > defs');
			if (!defs) {
				defs = doc.createElementNS(SVG_NS, 'defs');
				node.insertBefore(defs, node.firstChild);
			}
			const group = doc.createElementNS(SVG_NS, 'g');
			group.setAttribute('id', SOURCE_ID);
			for (const child of [...source.children]) {
				if (child.localName === 'defs') for (const d of [...child.children]) defs.append(doc.importNode(d, true));
				else group.append(doc.importNode(child, true));
			}
			defs.append(group);
			// Windows below the sheet's own entities (title block, notes).
			const before = node.querySelector(':scope > g[data-layer]');
			for (const f of layout.frames) {
				const win = doc.createElementNS(SVG_NS, 'svg');
				const x = f.x - origin.x;
				const y = f.y - origin.y;
				const vb = toLocal(f.viewBox, sourceOrigin);
				for (const [k, v] of Object.entries({ x, y, width: f.w, height: f.h, viewBox: `${vb.x} ${vb.y} ${vb.w} ${vb.h}`, preserveAspectRatio: 'none', class: 'ck-vp' })) {
					win.setAttribute(k, String(v));
				}
				const use = doc.createElementNS(SVG_NS, 'use');
				use.setAttribute('href', '#' + SOURCE_ID);
				win.append(use);
				const frame = doc.createElementNS(SVG_NS, 'rect');
				for (const [k, v] of Object.entries({ x, y, width: f.w, height: f.h, class: 'ck-vp-frame', 'vector-effect': 'non-scaling-stroke' })) frame.setAttribute(k, String(v));
				node.insertBefore(win, before);
				node.insertBefore(frame, before);
			}
			home = toLocal(layout.home, origin);
			homeWithImages = result.frames ? unionBox(home, result.frames) : home;
		}
		node.removeAttribute('width');
		node.removeAttribute('height');
		node.setAttribute('preserveAspectRatio', 'xMidYMid meet');
		node.setAttribute('aria-hidden', 'true');
		node.setAttribute('focusable', 'false');
		// A 3D model seen from above has faces on top of each other (roof over walls over ground):
		// viewer.css draws their fills translucent so every face stays visible.
		if (translucentFaces) node.setAttribute('data-faces', 'translucent');

		const svg = document.importNode(node, true) as unknown as SVGSVGElement;
		this.unit = Math.max(home.w, home.h, 1e-9) * 1e-7;
		const points = fixPoints(svg, this.unit);
		for (const g of svg.querySelectorAll<SVGGElement>('g[data-layer]')) {
			const name = g.getAttribute('data-layer') ?? '';
			const list = this.groups.get(name) ?? [];
			list.push(g);
			this.groups.set(name, list);
		}
		for (const el of svg.querySelectorAll('[stroke], [fill]')) {
			const stroke = el.getAttribute('stroke');
			const fill = el.getAttribute('fill');
			const p: Painted = { el, stroke: parseHex(stroke) ? stroke : null, fill: parseHex(fill) ? fill : null };
			if (p.stroke || p.fill) this.painted.push(p);
		}
		// Raster references without their image are dashed frames, usually larger than the drawing.
		this.images = [...svg.querySelectorAll<SVGGElement>('g[data-kind="images"]')];
		const imageFrames = this.images.reduce((n, g) => n + g.querySelectorAll('path').length, 0);
		this.host.append(svg);
		this.svg = svg;
		this.layout = layout;
		this.origin = origin;
		this.sourceOrigin = sourceOrigin;
		this.setImagesShown(showImages);
		if (sheet !== 'light') this.paint();
		return { home, homeWithImages, origin, sourceOrigin, layout, imageFrames, noWindows, empty: ownEmpty && !layout, nodes: svg.querySelectorAll('*').length, points };
	}

	/** Shows or hides the raster image frames. */
	setImagesShown(on: boolean): void {
		for (const g of this.images) {
			g.style.display = on ? '' : 'none';
			if (on) g.removeAttribute('data-ck-hidden');
			else g.setAttribute('data-ck-hidden', '');
		}
	}

	private paint(): void {
		const cache = new Map<string, string>();
		const shown = (c: string) => {
			let v = cache.get(c);
			if (v === undefined) {
				v = displayColor(c, this.sheet);
				cache.set(c, v);
			}
			return v;
		};
		for (const p of this.painted) {
			if (p.stroke) p.el.setAttribute('stroke', shown(p.stroke));
			if (p.fill) p.el.setAttribute('fill', shown(p.fill));
		}
	}

	/** Restyles the drawing for a sheet colour; no new render is needed. */
	setSheet(sheet: Sheet): void {
		if (sheet === this.sheet) return;
		this.sheet = sheet;
		this.paint();
	}

	setLayerHidden(name: string, hidden: boolean): void {
		for (const g of this.groups.get(name) ?? []) {
			g.style.display = hidden ? 'none' : '';
			if (hidden) g.setAttribute('data-ck-hidden', '');
			else g.removeAttribute('data-ck-hidden');
		}
	}

	setViewBox(value: string): void {
		this.svg?.setAttribute('viewBox', value);
	}

	/**
	 * Shows the selected entity as the engine drew it alone (SVG text), in the highlight colour.
	 * `inSource` puts it into the model shown through a layout's viewports. null clears it.
	 */
	setHighlight(svgText: string | null, inSource = false): boolean {
		this.highlight?.remove();
		this.highlight = null;
		if (!this.svg || !svgText) return false;
		let node: SVGSVGElement;
		try {
			node = parseSvg(svgText);
		} catch {
			return false;
		}
		// Like the engine's layer groups: shapes are outlines unless they carry their own fill.
		const g = s('g', { class: 'ck-hl', fill: 'none', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' });
		for (const layer of node.querySelectorAll('g[data-layer]')) for (const child of [...layer.children]) g.append(document.importNode(child, true));
		if (!g.childElementCount) return false;
		fixPoints(g, this.unit);
		const target = inSource ? this.svg.querySelector('#' + SOURCE_ID) : this.svg;
		(target ?? this.svg).append(g);
		this.highlight = g;
		return true;
	}

	/** Fallback highlight: a rectangle around a box in absolute SVG units (padded by the caller). null clears it. */
	setBox(abs: Box | null, inSource = false): void {
		if (!abs || !this.svg) {
			this.box?.remove();
			this.box = null;
			return;
		}
		const box = toLocal(abs, inSource ? this.sourceOrigin : this.origin);
		if (!this.box) {
			this.box = s('rect', { class: 'ck-selrect', 'vector-effect': 'non-scaling-stroke' });
			const target = inSource ? this.svg.querySelector('#' + SOURCE_ID) : this.svg;
			(target ?? this.svg).append(this.box);
		}
		for (const [k, v] of Object.entries({ x: box.x, y: box.y, width: box.w, height: box.h })) this.box.setAttribute(k, String(v));
	}

	/** Draws the measure path through the points (absolute drawing coordinates, Y up); [] clears it. */
	setMeasure(pts: Pt[], closed: boolean, cursor: Pt | null): void {
		this.measure?.remove();
		this.measure = null;
		if (!this.svg || !pts.length) return;
		const g = s('g', { class: 'ck-measure' });
		const all = cursor && !closed ? [...pts, cursor] : pts;
		const o = this.origin;
		const d = all.map((p, i) => `${i ? 'L' : 'M'}${p.x - o.x} ${-p.y - o.y}`).join('') + (closed && pts.length > 2 ? 'Z' : '');
		g.append(s('path', { class: 'ck-m-line', d, 'vector-effect': 'non-scaling-stroke' }));
		for (const p of pts) g.append(s('path', { class: 'ck-m-pt', d: `M${p.x - o.x} ${-p.y - o.y}h${this.unit}`, 'vector-effect': 'non-scaling-stroke' }));
		this.svg.append(g);
		this.measure = g;
	}

	/**
	 * The sheet as shown (hidden layers and overlays removed) as a standalone SVG document. Its
	 * coordinates are those of the page: relative to the local origin, which the file states in
	 * a comment and in its <desc>, because absolute georeferenced numbers would break in every
	 * viewer that uses float32 (all browsers).
	 */
	exportSvg(home: Box): string | null {
		if (!this.svg) return null;
		const copy = this.svg.cloneNode(true) as SVGSVGElement;
		for (const el of copy.querySelectorAll('[data-ck-hidden], .ck-hl, .ck-selrect, .ck-measure')) el.remove();
		copy.setAttribute('viewBox', `${home.x} ${home.y} ${home.w} ${home.h}`);
		copy.setAttribute('width', '1600');
		copy.setAttribute('height', String(Math.max(1, Math.round((1600 * home.h) / home.w))));
		for (const a of ['aria-hidden', 'focusable', 'preserveAspectRatio', 'data-faces']) copy.removeAttribute(a);
		// Styles the page applies with CSS, written out so the file looks the same on its own.
		for (const el of copy.querySelectorAll('.ck-pt')) {
			el.setAttribute('stroke-width', '4');
			el.setAttribute('stroke-linecap', 'round');
			el.removeAttribute('class');
		}
		for (const el of copy.querySelectorAll('.ck-vp-frame')) {
			el.setAttribute('fill', 'none');
			el.setAttribute('stroke', this.sheet === 'dark' ? '#8b98ad' : '#7a8496');
			el.removeAttribute('class');
		}
		copy.insertBefore(s('rect', { x: home.x, y: home.y, width: home.w, height: home.h, fill: SHEET_BG[this.sheet] }), copy.firstChild);
		const { x, y } = this.origin;
		let note = '';
		if (x || y) {
			note = `Coordinates are relative to a local origin: drawing X = x + ${x}, drawing Y = -(y + ${y}), in drawing units.`;
			const desc = document.createElementNS(SVG_NS, 'desc');
			desc.textContent = note;
			copy.insertBefore(desc, copy.firstChild);
			copy.setAttribute('data-origin', `${x} ${y}`);
		}
		return '<?xml version="1.0" encoding="UTF-8"?>\n' + (note ? `<!-- ${note} -->\n` : '') + new XMLSerializer().serializeToString(copy);
	}
}
