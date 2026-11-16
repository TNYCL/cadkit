// The /demo/ viewer (main thread). All parsing and export happens in the worker
// (src/demo/worker.ts); this module owns the DOM: stage, pan/zoom, panels, status.

import { SAMPLES, sampleById } from './samples.ts';
import * as V from './view.ts';
import { explainError, formatBytes, formatName, formatWithVersion, plural, unitName, unitSymbol, versionLabel } from './text.ts';
import {
	MAX_FILE_BYTES,
	type ExportResult,
	type LayerInfo,
	type OpenResult,
	type PickedEntity,
	type RenderResult,
	type Request,
	type Response,
} from './protocol.ts';

type Phase = 'download' | 'engine' | 'parse' | 'render' | 'export' | 'pick';
const PHASE_TEXT: Record<Phase, string> = {
	download: 'Downloading the sample',
	engine: 'Loading the engine',
	parse: 'Reading the drawing',
	render: 'Rendering the preview',
	export: 'Writing the export',
	pick: 'Picking',
};
const SHEET_BG = { light: '#ffffff', dark: '#0b1324' } as const;
const PICK_TOLERANCE_PX = 6;
const SVG_NS = 'http://www.w3.org/2000/svg';

class DemoError extends Error {
	constructor(
		readonly kind: string,
		message: string
	) {
		super(message);
	}
}

// ---------------------------------------------------------------------------------------------
// Engine: one dedicated worker, restarted after a trap or a cancel.

type Pending = { resolve: (v: unknown) => void; reject: (e: DemoError) => void; onPhase?: (p: Phase) => void };
type RequestBody = Request extends infer R ? (R extends Request ? Omit<R, 'id'> : never) : never;

class Engine {
	private worker: Worker | null = null;
	private pending = new Map<number, Pending>();
	private nextId = 1;
	/** Settles once the worker has instantiated the WebAssembly module (pending until start()). */
	ready: Promise<string> = new Promise<string>(() => {});
	onState: (state: string) => void = () => {};

	start(): void {
		this.onState('loading');
		const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module', name: 'cadkit-wasm' });
		this.worker = worker;
		this.ready = new Promise<string>((resolve, reject) => {
			worker.addEventListener('message', (ev: MessageEvent<Response>) => {
				const m = ev.data;
				if (m.type === 'ready') {
					this.onState('ready');
					resolve(m.version);
				} else if (m.type === 'boot-error') {
					this.onState('failed to start');
					reject(new DemoError('internal', 'The WebAssembly engine could not start: ' + m.error.message));
				} else if (m.type === 'phase') {
					this.pending.get(m.id)?.onPhase?.(m.phase);
				} else if (m.type === 'result') {
					const p = this.pending.get(m.id);
					if (!p) return;
					this.pending.delete(m.id);
					if (m.ok) p.resolve(m.value);
					else p.reject(new DemoError(m.error.kind, m.error.message));
				}
			});
			worker.addEventListener('error', (ev) => {
				ev.preventDefault();
				this.onState('failed to start');
				const err = new DemoError('internal', 'The engine worker failed: ' + (ev.message || 'it could not be loaded'));
				reject(err);
				this.failAll(err);
			});
		});
		this.ready.catch(() => {});
	}

	/** Ends the worker (and the open document); pending calls fail with kind. */
	restart(kind: string, message: string): void {
		this.worker?.terminate();
		this.worker = null;
		this.failAll(new DemoError(kind, message));
		this.start();
	}

	private failAll(err: DemoError): void {
		for (const p of this.pending.values()) p.reject(err);
		this.pending.clear();
	}

	async call<T>(body: RequestBody, onPhase?: (p: Phase) => void, transfer: Transferable[] = []): Promise<T> {
		await this.ready;
		const worker = this.worker;
		if (!worker) throw new DemoError('internal', 'engine not running');
		const id = this.nextId++;
		return new Promise<T>((resolve, reject) => {
			this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, ...(onPhase ? { onPhase } : {}) });
			worker.postMessage({ ...body, id } as Request, transfer);
		});
	}
}

// ---------------------------------------------------------------------------------------------
// Small DOM helpers.

const byId = <T extends HTMLElement = HTMLElement>(id: string): T => {
	const el = document.getElementById(id);
	if (!el) throw new Error('demo: missing #' + id);
	return el as T;
};

function h<K extends keyof HTMLElementTagNameMap>(tag: K, props: Record<string, string> = {}, ...children: (Node | string)[]): HTMLElementTagNameMap[K] {
	const el = document.createElement(tag);
	for (const [k, v] of Object.entries(props)) {
		if (k === 'class') el.className = v;
		else el.setAttribute(k, v);
	}
	el.append(...children);
	return el;
}

function fillDl(dl: HTMLElement, rows: [string, string | Node][]): void {
	dl.replaceChildren(
		...rows.map(([k, v]) => {
			const div = h('div', {}, h('dt', {}, k), h('dd', {}, v));
			return div;
		})
	);
}

const mono = (text: string) => h('code', {}, text);

/** Swatch colour for a DXF/DWG colour index (ACI; an approximation of the 256-colour palette). */
function aciColor(index: number, dark: boolean): string {
	const fixed = ['', '#ff0000', '#ffff00', '#00ff00', '#00ffff', '#0000ff', '#ff00ff', dark ? '#ffffff' : '#000000', '#808080', '#c0c0c0'];
	if (index >= 1 && index <= 9) return fixed[index]!;
	if (index >= 250 && index <= 255) return ['#333333', '#505050', '#696969', '#828282', '#bebebe', '#ffffff'][index - 250]!;
	if (index >= 10 && index <= 249) {
		const k = (index - 10) % 10;
		const hue = Math.floor((index - 10) / 10) * 15;
		const v = [1, 1, 0.65, 0.65, 0.5, 0.5, 0.3, 0.3, 0.15, 0.15][k]!;
		const s = k % 2 === 0 ? 1 : 0.5;
		const f = (n: number) => {
			const kk = (n + hue / 60) % 6;
			const c = v - v * s * Math.max(0, Math.min(kk, 4 - kk, 1));
			return Math.round(c * 255)
				.toString(16)
				.padStart(2, '0');
		};
		return '#' + f(5) + f(3) + f(1);
	}
	return '';
}

function layerSwatch(color: LayerInfo['color'], dark: boolean): string {
	if (color.type === 'rgb' && color.r !== undefined) return `rgb(${color.r}, ${color.g}, ${color.b})`;
	if (color.type === 'aci' && color.index !== undefined) return aciColor(color.index, dark);
	return '';
}

function download(name: string, data: BlobPart, mime: string): void {
	const url = URL.createObjectURL(new Blob([data], { type: mime }));
	const a = h('a', { href: url, download: name });
	a.hidden = true;
	document.body.append(a);
	a.click();
	a.remove();
	setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

// ---------------------------------------------------------------------------------------------

export function mount(): void {
	const root = document.getElementById('ck-viewer');
	if (!root) return;
	const el = {
		root,
		open: byId<HTMLButtonElement>('ck-open'),
		file: byId<HTMLInputElement>('ck-file'),
		sample: byId<HTMLSelectElement>('ck-sample'),
		filename: byId('ck-filename'),
		stage: byId('ck-stage'),
		canvas: byId('ck-canvas'),
		zin: byId<HTMLButtonElement>('ck-zin'),
		zout: byId<HTMLButtonElement>('ck-zout'),
		fit: byId<HTMLButtonElement>('ck-fit'),
		sheet: byId<HTMLButtonElement>('ck-sheet'),
		readout: byId('ck-readout'),
		empty: byId('ck-empty'),
		drop: byId('ck-drop'),
		busy: byId('ck-busy'),
		busyPhase: byId('ck-busy-phase'),
		busyTime: byId('ck-busy-time'),
		cancel: byId<HTMLButtonElement>('ck-cancel'),
		touch: byId('ck-touch'),
		touchStart: byId<HTMLButtonElement>('ck-touch-start'),
		touchDone: byId<HTMLButtonElement>('ck-touch-done'),
		alert: byId('ck-alert'),
		summary: byId('ck-summary'),
		modelWrap: byId('ck-model-wrap'),
		model: byId<HTMLSelectElement>('ck-model'),
		info: byId('ck-info'),
		layerCount: byId('ck-layer-count'),
		layers: byId('ck-layers'),
		layersAll: byId<HTMLButtonElement>('ck-layers-all'),
		layersNone: byId<HTMLButtonElement>('ck-layers-none'),
		selHeading: byId('ck-h-sel'),
		selHint: byId('ck-sel-hint'),
		sel: byId('ck-sel'),
		selClear: byId<HTMLButtonElement>('ck-sel-clear'),
		warnCount: byId('ck-warn-count'),
		warnings: byId('ck-warnings'),
		expSvg: byId<HTMLButtonElement>('ck-exp-svg'),
		expDxf: byId<HTMLButtonElement>('ck-exp-dxf'),
		expJson: byId<HTMLButtonElement>('ck-exp-json'),
		status: byId('ck-status'),
		engineState: byId('ck-engine-state'),
	};

	const engine = new Engine();
	engine.onState = (s) => (el.engineState.textContent = 'engine ' + s);

	// --- state ---
	let doc: OpenResult | null = null;
	let model = 0;
	let readMs = 0;
	let svg: SVGSVGElement | null = null;
	let selRect: SVGRectElement | null = null;
	let layerGroups = new Map<string, SVGGElement>();
	const hidden = new Set<string>();
	let view: V.View | null = null;
	let selection: PickedEntity | null = null;
	let sheet: 'light' | 'dark' = document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
	let busyToken = 0;
	let touchActive = false;
	const coarse = window.matchMedia('(pointer: coarse)');

	const announce = (text: string) => {
		// Re-setting identical text is not announced; clear first.
		el.status.textContent = '';
		requestAnimationFrame(() => (el.status.textContent = text));
	};

	function showAlert(kind: string, detail: string): void {
		const ex = explainError(kind);
		const close = h('button', { type: 'button', class: 'ck-link-btn', 'aria-label': 'Dismiss message' }, 'Dismiss');
		close.addEventListener('click', () => {
			el.alert.hidden = true;
			el.stage.focus({ preventScroll: true });
		});
		el.alert.replaceChildren(
			h('p', { class: 'ck-alert-title' }, ex.title),
			h('p', {}, ex.hint),
			...(detail ? [h('p', { class: 'ck-alert-detail' }, h('code', {}, kind), ' ', detail)] : []),
			close
		);
		el.alert.hidden = false;
	}
	const clearAlert = () => (el.alert.hidden = true);

	function setState(state: 'empty' | 'busy' | 'ready'): void {
		el.root.dataset.state = state;
	}

	function setBusy(phase: Phase | null): number {
		const token = ++busyToken;
		if (!phase) {
			el.busy.hidden = true;
			return token;
		}
		el.busyPhase.textContent = PHASE_TEXT[phase];
		if (el.busy.hidden) {
			const started = performance.now();
			el.busy.hidden = false;
			el.busyTime.textContent = '';
			const tick = () => {
				if (el.busy.hidden) return;
				el.busyTime.textContent = ((performance.now() - started) / 1000).toFixed(1) + ' s';
				setTimeout(tick, 100);
			};
			setTimeout(tick, 100);
			if (el.root.contains(document.activeElement)) el.cancel.focus({ preventScroll: true });
		}
		return token;
	}
	const phase = (p: Phase) => {
		if (!el.busy.hidden) el.busyPhase.textContent = PHASE_TEXT[p];
	};

	function setEnabled(on: boolean): void {
		for (const b of [el.zin, el.zout, el.fit, el.sheet, el.expSvg, el.expDxf, el.expJson, el.layersAll, el.layersNone]) b.disabled = !on;
	}

	// --- stage geometry ---
	const stageSize = () => {
		const r = el.stage.getBoundingClientRect();
		return { w: Math.max(1, r.width), h: Math.max(1, r.height), left: r.left, top: r.top };
	};

	let frame = 0;
	function applyView(): void {
		if (frame) return;
		frame = requestAnimationFrame(() => {
			frame = 0;
			if (!svg || !view) return;
			const { w, h } = stageSize();
			svg.setAttribute('viewBox', V.viewBoxOf(view, w, h));
			updateSelRect();
		});
	}

	function setView(next: V.View): void {
		view = next;
		applyView();
	}

	function fitView(): void {
		if (!view) return;
		const { w, h } = stageSize();
		setView(V.fit(view.home, w, h));
		showCentre();
	}

	function zoomBy(factor: number, px?: number, py?: number): void {
		if (!view) return;
		const { w, h } = stageSize();
		setView(V.zoomAt(view, w, h, factor, px ?? w / 2, py ?? h / 2));
	}

	const units = () => (doc ? unitSymbol(doc.info.units) : '');
	function showCoords(px: number, py: number): void {
		if (!view) return;
		const { w, h } = stageSize();
		const p = V.toWorld(view, w, h, px, py);
		const u = units();
		el.readout.textContent = `X ${V.formatCoord(p.x, view.s)}  Y ${V.formatCoord(p.y, view.s)}${u ? '  ' + u : ''}`;
	}
	function showCentre(): void {
		const { w, h } = stageSize();
		showCoords(w / 2, h / 2);
	}

	// --- rendering ---
	function sanitize(root: Element): void {
		for (const bad of root.querySelectorAll('script, foreignObject, iframe, object, embed, a')) bad.remove();
		for (const n of root.querySelectorAll('[href], [*|href]')) {
			const href = n.getAttribute('href') ?? n.getAttributeNS('http://www.w3.org/1999/xlink', 'href') ?? '';
			if (!href.startsWith('#') && !href.startsWith('data:image/')) n.remove();
		}
	}

	async function render(fit: boolean): Promise<void> {
		if (!doc) return;
		const info = doc.info;
		el.canvas.replaceChildren();
		svg = null;
		selRect = null;
		layerGroups = new Map();
		if (!(info.models[model]?.entities ?? 0)) {
			el.canvas.append(h('p', { class: 'ck-canvas-note' }, 'This model has no entities to draw.'));
			buildLayers();
			return;
		}
		let result: RenderResult;
		try {
			result = await engine.call<RenderResult>({ type: 'render', model, background: SHEET_BG[sheet] }, phase);
		} catch (e) {
			const err = e as DemoError;
			if (err.kind === 'trap') throw err;
			el.canvas.append(
				h(
					'p',
					{ class: 'ck-canvas-note' },
					err.kind === 'limit_exceeded'
						? 'No preview: this model is too large to render as SVG in the browser. Document details, layers, warnings and exports are still available.'
						: 'No preview: ' + err.message
				)
			);
			buildLayers();
			return;
		}
		const parsed = new DOMParser().parseFromString(result.svg, 'image/svg+xml');
		const node = parsed.documentElement;
		if (node.localName !== 'svg' || parsed.getElementsByTagName('parsererror').length) {
			throw new DemoError('internal', 'the renderer returned SVG the browser could not parse');
		}
		sanitize(node);
		const home = V.parseViewBox(node.getAttribute('viewBox'));
		// The background rectangle is drawn by the stage instead, so panning past the extents
		// shows the same sheet colour.
		const first = node.firstElementChild;
		if (first?.localName === 'rect' && first.getAttribute('fill') === SHEET_BG[sheet]) first.remove();
		node.removeAttribute('width');
		node.removeAttribute('height');
		node.setAttribute('preserveAspectRatio', 'xMidYMid meet');
		node.setAttribute('aria-hidden', 'true');
		node.setAttribute('focusable', 'false');
		// A 3D model is shown as a top view, where faces lie on top of each other (roof over
		// walls over ground). viewer.css draws their fills translucent with full-colour outlines
		// so every face stays visible; the SVG export keeps the engine's own fills.
		if (info.models[model]?.is3d) node.setAttribute('data-faces', 'translucent');
		const imported = document.importNode(node, true) as unknown as SVGSVGElement;
		el.canvas.append(imported);
		svg = imported;
		for (const g of imported.querySelectorAll<SVGGElement>(':scope > g[data-layer]')) {
			const name = g.getAttribute('data-layer') ?? '';
			layerGroups.set(name, g);
			if (hidden.has(name)) g.style.display = 'none';
		}
		const { w: sw, h: sh } = stageSize();
		const keep = !fit && view && view.home.x === home.x && view.home.y === home.y && view.home.w === home.w;
		view = keep && view ? { ...view, home } : V.fit(home, sw, sh);
		applyView();
		buildLayers();
		if (selection) drawSelection();
	}

	// --- panels ---
	function fillDocument(): void {
		if (!doc) return;
		const i = doc.info;
		const total = i.models.reduce((n, m) => n + m.entities, 0);
		const m = i.models[model];
		const layerNames = doc.layers.map((l) => l.name);
		const listed = layerNames.slice(0, 24).join(', ') + (layerNames.length > 24 ? `, and ${layerNames.length - 24} more` : '');
		el.summary.textContent =
			`${doc.name}: ${formatWithVersion(i.format, i.version)}, units ${unitName(i.units)}, ${plural(total, 'entity', 'entities')} in ${plural(i.models.length, 'model')}. ` +
			(m ? `Showing ${m.name || 'model ' + (model + 1)} (${plural(m.entities, 'entity', 'entities')}${m.is3d ? ', 3D, top view' : ''}). ` : '') +
			(layerNames.length ? `${plural(layerNames.length, 'layer')}: ${listed}. ` : 'No layers. ') +
			(i.warningCount ? `${plural(i.warningCount, 'warning')}.` : 'No warnings.');

		const rows: [string, string | Node][] = [
			['File', doc.name],
			['Size', formatBytes(doc.bytes)],
			['Format', formatName(i.format)],
			['Version', versionLabel(i.format, i.version)],
		];
		if (i.application) rows.push(['Application', i.application]);
		rows.push(
			['Units', unitName(i.units) + (i.metersPerUnit ? ` (${i.metersPerUnit} m per unit)` : '')],
			['Entities', total.toLocaleString('en-US')],
			['Layers', i.layerCount.toLocaleString('en-US')],
			['Blocks', i.blockCount.toLocaleString('en-US')],
			['Models', i.models.length.toLocaleString('en-US')]
		);
		if (doc.kinds) {
			const kinds = Object.entries(doc.kinds).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
			rows.push(['Kinds', h('span', { class: 'ck-kinds' }, ...kinds.map(([k, n]) => h('span', {}, mono(k), ' ' + n.toLocaleString('en-US'))))]);
		}
		rows.push(['Read in', `${Math.round(readMs)} ms in this browser`]);
		fillDl(el.info, rows);

		el.model.replaceChildren(
			...i.models.map((mm, idx) => {
				const o = h('option', { value: String(idx) }, `${mm.name || 'Model ' + (idx + 1)} (${mm.entities.toLocaleString('en-US')})`);
				return o;
			})
		);
		el.model.value = String(model);
		el.modelWrap.hidden = i.models.length < 2;
	}

	function buildLayers(): void {
		if (!doc) {
			el.layers.replaceChildren();
			el.layerCount.textContent = '';
			return;
		}
		const names = doc.layers.map((l) => l.name);
		for (const n of layerGroups.keys()) if (!names.includes(n)) names.push(n);
		const meta = new Map(doc.layers.map((l) => [l.name, l]));
		el.layerCount.textContent = `(${names.length})`;
		if (!names.length) {
			el.layers.replaceChildren(h('li', { class: 'ck-hint' }, 'This drawing has no layers.'));
			return;
		}
		el.layers.replaceChildren(
			...names.map((name, idx) => {
				const g = layerGroups.get(name);
				const l = meta.get(name);
				const id = 'ck-layer-' + idx;
				const cb = h('input', { type: 'checkbox', id });
				cb.checked = Boolean(g) && !hidden.has(name);
				cb.disabled = !g;
				cb.addEventListener('change', () => toggleLayer(name, cb.checked));
				const sw = h('span', { class: 'ck-swatch', 'aria-hidden': 'true' });
				const colour = l ? layerSwatch(l.color, sheet === 'dark') : '';
				if (colour) sw.style.background = colour;
				else sw.classList.add('ck-swatch--none');
				const note = !g
					? l && (l.frozen || !l.visible)
						? 'off in file'
						: 'nothing drawn'
					: l && (l.frozen || !l.visible)
						? 'off in file'
						: '';
				const label = h('label', { for: id }, sw, h('span', { class: 'ck-lname' }, name || '(unnamed)'));
				if (note) label.append(h('span', { class: 'ck-lnote' }, note));
				return h('li', {}, cb, label);
			})
		);
	}

	function toggleLayer(name: string, on: boolean): void {
		if (on) hidden.delete(name);
		else hidden.add(name);
		const g = layerGroups.get(name);
		if (g) g.style.display = on ? '' : 'none';
		if (!on && selection?.layer === name) clearSelection(false);
	}

	function setAllLayers(on: boolean): void {
		for (const name of layerGroups.keys()) toggleLayer(name, on);
		for (const cb of el.layers.querySelectorAll<HTMLInputElement>('input[type=checkbox]:not(:disabled)')) cb.checked = on;
		announce(on ? 'All layers shown.' : 'All layers hidden.');
	}

	function fillWarnings(): void {
		if (!doc) {
			el.warnings.replaceChildren();
			el.warnCount.textContent = '';
			return;
		}
		const total = doc.info.warningCount;
		el.warnCount.textContent = `(${total.toLocaleString('en-US')})`;
		if (!total) {
			el.warnings.replaceChildren(h('p', { class: 'ck-hint' }, 'None for this file.'));
			return;
		}
		const byCode = new Map<string, string[]>();
		for (const w of doc.warnings) {
			const list = byCode.get(w.code) ?? [];
			list.push(w.offset === null ? w.message : `${w.message} (byte ${w.offset.toLocaleString('en-US')})`);
			byCode.set(w.code, list);
		}
		const codes = Object.entries(doc.warningCodes).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
		el.warnings.replaceChildren(
			...codes.map(([code, count]) => {
				const msgs = byCode.get(code) ?? [];
				const d = h('details', { class: 'ck-warn' }, h('summary', {}, h('code', {}, code), h('span', { class: 'ck-count' }, '× ' + count.toLocaleString('en-US'))));
				const ul = h('ul', {}, ...msgs.slice(0, 50).map((m) => h('li', {}, m)));
				if (count > Math.min(msgs.length, 50)) ul.append(h('li', { class: 'ck-hint' }, `${(count - Math.min(msgs.length, 50)).toLocaleString('en-US')} more not listed`));
				d.append(ul);
				return d;
			})
		);
	}

	// --- selection ---
	function updateSelRect(): void {
		if (!selRect || !selection || !view) return;
		const pad = 4 * view.s;
		const [x0, y0] = selection.bbox.min;
		const [x1, y1] = selection.bbox.max;
		selRect.setAttribute('x', String(x0 - pad));
		selRect.setAttribute('y', String(-y1 - pad));
		selRect.setAttribute('width', String(Math.max(0, x1 - x0) + 2 * pad));
		selRect.setAttribute('height', String(Math.max(0, y1 - y0) + 2 * pad));
	}

	function drawSelection(): void {
		selRect?.remove();
		selRect = null;
		if (!svg || !selection) return;
		const r = document.createElementNS(SVG_NS, 'rect');
		r.setAttribute('class', 'ck-selrect');
		r.setAttribute('vector-effect', 'non-scaling-stroke');
		svg.append(r);
		selRect = r;
		updateSelRect();
	}

	function clearSelection(speak = true): void {
		selection = null;
		selRect?.remove();
		selRect = null;
		el.sel.replaceChildren();
		el.selHint.hidden = false;
		el.selClear.hidden = true;
		if (speak) announce('Selection cleared.');
	}

	function showSelection(hit: PickedEntity): void {
		selection = hit;
		const fmt = (n: number) => (view ? V.formatCoord(n, view.s) : String(n));
		const rows: [string, string | Node][] = [
			['Kind', mono(hit.kind)],
			['Layer', hit.layer || '(unnamed)'],
			['Index', String(hit.index)],
		];
		if (hit.id !== null) rows.push(['Handle / id', mono(String(hit.id))]);
		if (hit.color) rows.push(['Colour', hit.color]);
		if (hit.block) rows.push(['Block', hit.block]);
		if (hit.text) rows.push(['Text', hit.text]);
		const [x0, y0] = hit.bbox.min;
		const [x1, y1] = hit.bbox.max;
		const u = units();
		rows.push(['Bounds', mono(`(${fmt(x0)}, ${fmt(y0)}) to (${fmt(x1)}, ${fmt(y1)})${u ? ' ' + u : ''}`)]);
		for (const a of hit.attributes) rows.push(['Attribute ' + a.tag, a.value]);
		if (hit.truncated) rows.push(['Note', 'Some metadata was shortened to the preview limits.']);
		fillDl(el.sel, rows);
		el.selHint.hidden = true;
		el.selClear.hidden = false;
		drawSelection();
		announce(`Selected ${hit.kind} on layer ${hit.layer || 'unnamed'}.`);
	}

	async function pickAt(px: number, py: number): Promise<void> {
		if (!doc || !view || !svg) return;
		const { w, h } = stageSize();
		const p = V.toWorld(view, w, h, px, py);
		const excluded = [...hidden];
		try {
			const hit = await engine.call<PickedEntity | null>({
				type: 'pick',
				model,
				x: p.x,
				y: p.y,
				tolerance: PICK_TOLERANCE_PX * view.s,
				excludedLayers: excluded,
			});
			if (hit) showSelection(hit);
			else {
				clearSelection(false);
				announce('Nothing to pick there.');
			}
		} catch (e) {
			handleError(e);
		}
	}

	// --- opening ---
	function resetDocument(): void {
		doc = null;
		svg = null;
		view = null;
		selRect = null;
		selection = null;
		layerGroups = new Map();
		hidden.clear();
		el.canvas.replaceChildren();
		el.readout.textContent = '';
		el.filename.replaceChildren(h('span', { class: 'ck-muted' }, 'No file open'));
		el.summary.textContent = 'No drawing open. Open a file or a sample to see its format, units, layers and entities here.';
		el.info.replaceChildren();
		el.modelWrap.hidden = true;
		buildLayers();
		fillWarnings();
		clearSelection(false);
		setEnabled(false);
		setTouch(false);
		el.touch.hidden = true;
		setState('empty');
	}

	function handleError(e: unknown): void {
		const err = e instanceof DemoError ? e : new DemoError('internal', e instanceof Error ? e.message : String(e));
		if (err.kind === 'trap') {
			engine.restart('trap', err.message);
			resetDocument();
		}
		if (err.kind === 'cancelled') return;
		showAlert(err.kind, err.message);
		announce(explainError(err.kind).title + '.');
	}

	async function openBuffer(name: string, buffer: ArrayBuffer, sampleId: string | null): Promise<void> {
		clearAlert();
		if (buffer.byteLength > MAX_FILE_BYTES) {
			showAlert('too_large', `${name} is ${formatBytes(buffer.byteLength)}; the demo opens files up to ${formatBytes(MAX_FILE_BYTES)}.`);
			return;
		}
		const token = setBusy('engine');
		try {
			await engine.ready;
			phase('parse');
			const t0 = performance.now();
			const result = await engine.call<OpenResult>({ type: 'open', name, buffer }, phase, [buffer]);
			readMs = performance.now() - t0;
			if (token !== busyToken) return;
			doc = result;
			model = result.model;
			hidden.clear();
			clearSelection(false);
			await render(true);
			fillDocument();
			fillWarnings();
			setEnabled(true);
			setState('ready');
			el.filename.replaceChildren(h('span', { class: 'ck-fname' }, name), h('span', { class: 'ck-fmt' }, formatWithVersion(result.info.format, result.info.version)));
			el.sample.value = sampleId ?? '';
			const url = new URL(location.href);
			if (sampleId) url.searchParams.set('sample', sampleId);
			else url.searchParams.delete('sample');
			history.replaceState(null, '', url);
			setBusy(null);
			el.touch.hidden = !coarse.matches;
			el.stage.focus({ preventScroll: true });
			const i = result.info;
			announce(
				`Opened ${name}: ${formatWithVersion(i.format, i.version)}, ${plural(i.models.reduce((n, m) => n + m.entities, 0), 'entity', 'entities')}, ${plural(i.layerCount, 'layer')}` +
					(i.warningCount ? `, ${plural(i.warningCount, 'warning')} (see Warnings).` : '.')
			);
		} catch (e) {
			if (token === busyToken) setBusy(null);
			handleError(e);
			if (!doc) setState('empty');
		}
	}

	async function openFile(f: File): Promise<void> {
		if (f.size > MAX_FILE_BYTES) {
			showAlert('too_large', `${f.name} is ${formatBytes(f.size)}; the demo opens files up to ${formatBytes(MAX_FILE_BYTES)}.`);
			return;
		}
		let buf: ArrayBuffer;
		try {
			buf = await f.arrayBuffer();
		} catch (e) {
			showAlert('io', e instanceof Error ? e.message : String(e));
			return;
		}
		await openBuffer(f.name, buf, null);
	}

	async function openSample(id: string): Promise<void> {
		const s = sampleById(id);
		if (!s) {
			showAlert('unknown_sample', `there is no sample "${id}". Available: ${SAMPLES.map((x) => x.id).join(', ')}.`);
			return;
		}
		clearAlert();
		const token = setBusy('download');
		let buf: ArrayBuffer;
		try {
			const res = await fetch(s.path);
			if (!res.ok) throw new DemoError('network', `HTTP ${res.status} for ${s.path}`);
			buf = await res.arrayBuffer();
		} catch (e) {
			if (token === busyToken) setBusy(null);
			handleError(e instanceof DemoError ? e : new DemoError('network', e instanceof Error ? e.message : String(e)));
			return;
		}
		if (token !== busyToken) return;
		await openBuffer(s.path.split('/').pop() ?? s.id, buf, s.id);
	}

	// --- exports ---
	const stem = () => (doc?.name ?? 'drawing').replace(/\.[^.]+$/, '') || 'drawing';

	function exportSvg(): void {
		if (!svg || !view) {
			showAlert('internal', 'there is no preview to export for this model.');
			return;
		}
		const copy = svg.cloneNode(true) as SVGSVGElement;
		for (const g of copy.querySelectorAll<SVGGElement>('g[data-layer]')) if (g.style.display === 'none') g.remove();
		copy.querySelector('.ck-selrect')?.remove();
		const hb = view.home;
		copy.setAttribute('viewBox', `${hb.x} ${hb.y} ${hb.w} ${hb.h}`);
		copy.setAttribute('width', '1600');
		copy.setAttribute('height', String(Math.round((1600 * hb.h) / hb.w)));
		copy.removeAttribute('aria-hidden');
		copy.removeAttribute('focusable');
		copy.removeAttribute('preserveAspectRatio');
		const bg = document.createElementNS(SVG_NS, 'rect');
		for (const [k, v] of Object.entries({ x: hb.x, y: hb.y, width: hb.w, height: hb.h, fill: SHEET_BG[sheet] })) bg.setAttribute(k, String(v));
		copy.insertBefore(bg, copy.firstChild);
		const text = '<?xml version="1.0" encoding="UTF-8"?>\n' + new XMLSerializer().serializeToString(copy);
		download(stem() + '.svg', text, 'image/svg+xml');
		announce('SVG export saved.');
	}

	async function exportDoc(format: 'dxf' | 'json'): Promise<void> {
		const token = setBusy('export');
		try {
			const r = await engine.call<ExportResult>({ type: 'export', format }, phase);
			if (token !== busyToken) return;
			download(`${stem()}.${r.extension}`, r.data, r.mime);
			announce(`${format.toUpperCase()} export saved (${formatBytes(new Blob([r.data]).size)}).`);
		} catch (e) {
			handleError(e);
		} finally {
			if (token === busyToken) setBusy(null);
			el.root.querySelector<HTMLButtonElement>(format === 'dxf' ? '#ck-exp-dxf' : '#ck-exp-json')?.focus({ preventScroll: true });
		}
	}

	// --- touch gate (coarse pointers): the page keeps scrolling until the user opts in ---
	function setTouch(on: boolean): void {
		touchActive = on;
		el.root.dataset.touch = on ? 'active' : '';
		el.touch.hidden = on || !doc || !coarse.matches;
		el.touchDone.hidden = !on || !coarse.matches;
	}
	el.touchStart.addEventListener('click', () => {
		setTouch(true);
		el.stage.focus({ preventScroll: true });
		announce('Touch gestures now pan and zoom the drawing. Press Done to scroll the page again.');
	});
	el.touchDone.addEventListener('click', () => {
		setTouch(false);
		el.touchStart.focus({ preventScroll: true });
	});

	// --- pointer input ---
	const pointers = new Map<number, { x: number; y: number; sx: number; sy: number; moved: boolean }>();
	let pinch: { d: number; mx: number; my: number } | null = null;
	const pinchState = () => {
		const [a, b] = [...pointers.values()];
		if (!a || !b) return null;
		return { d: Math.hypot(a.x - b.x, a.y - b.y), mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2 };
	};
	const acceptsPointer = (e: PointerEvent) => Boolean(view) && (e.pointerType !== 'touch' || touchActive);

	el.stage.addEventListener('pointerdown', (e) => {
		if (!acceptsPointer(e) || (e.pointerType === 'mouse' && e.button !== 0)) return;
		el.stage.setPointerCapture(e.pointerId);
		pointers.set(e.pointerId, { x: e.clientX, y: e.clientY, sx: e.clientX, sy: e.clientY, moved: false });
		if (pointers.size === 2) pinch = pinchState();
		el.root.dataset.drag = 'true';
	});
	el.stage.addEventListener('pointermove', (e) => {
		const { left, top } = stageSize();
		if (view && e.pointerType === 'mouse') showCoords(e.clientX - left, e.clientY - top);
		const p = pointers.get(e.pointerId);
		if (!p || !view) return;
		if (Math.hypot(e.clientX - p.sx, e.clientY - p.sy) > 4) p.moved = true;
		if (pointers.size === 1) setView(V.pan(view, e.clientX - p.x, e.clientY - p.y));
		p.x = e.clientX;
		p.y = e.clientY;
		if (pointers.size === 2 && pinch) {
			p.moved = true;
			const now = pinchState();
			if (now && pinch.d > 0 && now.d > 0) {
				zoomBy(now.d / pinch.d, now.mx - left, now.my - top);
				if (view) setView(V.pan(view, now.mx - pinch.mx, now.my - pinch.my));
				pinch = now;
			}
		}
	});
	const endPointer = (e: PointerEvent) => {
		const p = pointers.get(e.pointerId);
		const click = p && !p.moved && pointers.size === 1 && e.type === 'pointerup';
		pointers.delete(e.pointerId);
		pinch = pointers.size === 2 ? pinchState() : null;
		if (!pointers.size) delete el.root.dataset.drag;
		if (click) {
			const { left, top } = stageSize();
			void pickAt(e.clientX - left, e.clientY - top);
		}
	};
	el.stage.addEventListener('pointerup', endPointer);
	el.stage.addEventListener('pointercancel', endPointer);
	el.stage.addEventListener('pointerleave', (e) => {
		if (e.pointerType === 'mouse' && !pointers.size) showCentre();
	});
	el.stage.addEventListener(
		'wheel',
		(e) => {
			if (!view) return;
			e.preventDefault();
			const { left, top } = stageSize();
			let dy = e.deltaY;
			if (e.deltaMode === 1) dy *= 16;
			else if (e.deltaMode === 2) dy *= 400;
			zoomBy(Math.exp(-Math.max(-400, Math.min(400, dy)) * 0.0015), e.clientX - left, e.clientY - top);
			showCoords(e.clientX - left, e.clientY - top);
		},
		{ passive: false }
	);
	el.stage.addEventListener('dblclick', (e) => {
		if (!view) return;
		const { left, top } = stageSize();
		zoomBy(2, e.clientX - left, e.clientY - top);
	});

	// --- keyboard ---
	el.stage.addEventListener('keydown', (e) => {
		if (!view || e.ctrlKey || e.metaKey || e.altKey) return;
		const { w, h } = stageSize();
		const step = (e.shiftKey ? 0.4 : 0.1) * Math.min(w, h);
		const actions: Record<string, () => void> = {
			ArrowLeft: () => setView(V.pan(view!, step, 0)),
			ArrowRight: () => setView(V.pan(view!, -step, 0)),
			ArrowUp: () => setView(V.pan(view!, 0, step)),
			ArrowDown: () => setView(V.pan(view!, 0, -step)),
			'+': () => zoomBy(1.25),
			'=': () => zoomBy(1.25),
			'-': () => zoomBy(0.8),
			_: () => zoomBy(0.8),
			'0': () => fitView(),
			Enter: () => void pickAt(w / 2, h / 2),
			' ': () => void pickAt(w / 2, h / 2),
			Escape: () => clearSelection(),
		};
		const fn = actions[e.key];
		if (!fn) return;
		e.preventDefault();
		fn();
		if (e.key !== 'Enter' && e.key !== ' ' && e.key !== 'Escape') showCentre();
	});
	el.stage.addEventListener('focus', () => {
		if (view) showCentre();
	});

	new ResizeObserver(() => applyView()).observe(el.stage);

	// --- controls ---
	el.zin.addEventListener('click', () => zoomBy(1.4));
	el.zout.addEventListener('click', () => zoomBy(1 / 1.4));
	el.fit.addEventListener('click', fitView);
	const syncSheetButton = () => {
		el.root.dataset.sheet = sheet;
		el.sheet.setAttribute('aria-pressed', String(sheet === 'dark'));
	};
	syncSheetButton();
	el.sheet.addEventListener('click', async () => {
		sheet = sheet === 'dark' ? 'light' : 'dark';
		syncSheetButton();
		if (!doc) return;
		const token = setBusy('render');
		try {
			await render(false);
		} catch (e) {
			handleError(e);
		} finally {
			if (token === busyToken) setBusy(null);
			el.sheet.focus({ preventScroll: true });
		}
		announce(sheet === 'dark' ? 'Dark sheet.' : 'Light sheet.');
	});
	el.model.addEventListener('change', async () => {
		model = Number(el.model.value) || 0;
		clearSelection(false);
		const token = setBusy('render');
		try {
			await render(true);
			fillDocument();
		} catch (e) {
			handleError(e);
		} finally {
			if (token === busyToken) setBusy(null);
		}
		const m = doc?.info.models[model];
		if (m) announce(`Showing ${m.name}, ${plural(m.entities, 'entity', 'entities')}.`);
	});
	el.layersAll.addEventListener('click', () => setAllLayers(true));
	el.layersNone.addEventListener('click', () => setAllLayers(false));
	el.selClear.addEventListener('click', () => {
		clearSelection();
		el.selHeading.focus();
	});
	el.expSvg.addEventListener('click', exportSvg);
	el.expDxf.addEventListener('click', () => void exportDoc('dxf'));
	el.expJson.addEventListener('click', () => void exportDoc('json'));

	el.cancel.addEventListener('click', () => {
		busyToken++;
		el.busy.hidden = true;
		engine.restart('cancelled', 'cancelled by the user');
		resetDocument();
		showAlert('cancelled', '');
		announce('Cancelled. The engine was restarted.');
		el.open.focus({ preventScroll: true });
	});

	el.open.addEventListener('click', () => el.file.click());
	el.file.addEventListener('change', () => {
		const f = el.file.files?.[0];
		el.file.value = '';
		if (f) void openFile(f);
	});
	el.sample.addEventListener('change', () => {
		if (el.sample.value) void openSample(el.sample.value);
	});
	for (const b of el.root.querySelectorAll<HTMLButtonElement>('.ck-sample-btn')) {
		b.addEventListener('click', () => void openSample(b.dataset.sample ?? ''));
	}

	// Drag and drop anywhere on the viewer; dropping elsewhere on the page must not navigate away.
	let depth = 0;
	el.root.addEventListener('dragenter', (e) => {
		if (!e.dataTransfer?.types.includes('Files')) return;
		e.preventDefault();
		depth++;
		el.drop.hidden = false;
	});
	el.root.addEventListener('dragover', (e) => {
		if (!e.dataTransfer?.types.includes('Files')) return;
		e.preventDefault();
		e.dataTransfer.dropEffect = 'copy';
	});
	el.root.addEventListener('dragleave', () => {
		depth = Math.max(0, depth - 1);
		if (!depth) el.drop.hidden = true;
	});
	el.root.addEventListener('drop', (e) => {
		e.preventDefault();
		depth = 0;
		el.drop.hidden = true;
		const f = e.dataTransfer?.files?.[0];
		if (f) void openFile(f);
	});
	window.addEventListener('dragover', (e) => e.preventDefault());
	window.addEventListener('drop', (e) => e.preventDefault());

	// --- start ---
	setEnabled(false);
	engine.start();
	engine.ready.catch((e: unknown) => handleError(e));
	const wanted = new URL(location.href).searchParams.get('sample');
	if (wanted) void openSample(wanted);
}
