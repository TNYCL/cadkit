// The /demo/ viewer (main thread). All parsing and export happens in the worker
// (src/demo/worker.ts); this module owns the DOM: the stage with its 2D sheet (sheet2d.ts) and
// 3D view (gl3d.ts), pan/zoom and picking, the tabbed details panel and the status line.

import { layerSwatch, SHEET_BG, type Sheet } from './colors.ts';
import { byId, download, fillDl, h, mono } from './dom.ts';
import { DemoError, Engine } from './engine.ts';
import { Scene3d } from './gl3d.ts';
import { SAMPLES, sampleById } from './samples.ts';
import { formatMeasure, paperToModel, pathLength, polygonArea, type Pt } from './scene2d.ts';
import { Sheet2d, type Loaded } from './sheet2d.ts';
import * as V from './view.ts';
import {
	cleanMessage,
	explainError,
	extensionHint,
	formatBytes,
	formatMs,
	formatName,
	formatWithVersion,
	plural,
	sourceTypeName,
	unitName,
	unitSymbol,
	versionLabel,
} from './text.ts';
import {
	LEGACY_DXF_VERSIONS,
	MAX_FILE_BYTES,
	type CallRecord,
	type CheckResult,
	type DxfVersion,
	type EntityResult,
	type ExportResult,
	type GmlPreflight,
	type GmlValidation,
	type MeshResult,
	type OpenResult,
	type Phase as WorkerPhase,
	type PickedEntity,
	type RenderResult,
	type RoundTrip,
	type TransformResult,
} from './protocol.ts';

type Phase = WorkerPhase | 'download' | 'engine';
const PHASE_TEXT: Record<Phase, string> = {
	download: 'Downloading the sample',
	engine: 'Loading the engine',
	parse: 'Reading the drawing',
	render: 'Rendering the sheet',
	export: 'Writing the export',
	pick: 'Picking',
	mesh: 'Building the 3D view',
	check: 'Running the check',
};
const PICK_TOLERANCE_PX = 6;
/** The busy overlay appears only when work takes longer than this (no flash on fast files). */
const BUSY_DELAY_MS = 180;
/** Sample opened when the page is loaded without ?sample=. */
const DEFAULT_SAMPLE = 'plan-dxf';
const MAX_CALLS_SHOWN = 300;
const TAB_IDS = ['file', 'layers', 'entity', 'export', 'checks', 'calls'] as const;
type TabId = (typeof TAB_IDS)[number];

interface Selection {
	/** Model the entity is in (model space for a pick through a layout viewport). */
	model: number;
	/** Index chain from the model's entity list down through groups. */
	path: number[];
	/** 2D pick result; null for a 3D pick. */
	hit: PickedEntity | null;
	/** Face of the 3D mesh; -1 for a 2D pick. */
	face: number;
	/** The pick went through a layout viewport into model space. */
	inSource: boolean;
	detail: EntityResult | null;
}

/** Roving-tabindex tabs (WAI-ARIA): arrow keys move, Home and End jump. */
function wireTabs(list: HTMLElement, select: (id: string, focus: boolean) => void, key: string): void {
	const tabs = () => [...list.querySelectorAll<HTMLButtonElement>('[role=tab]')];
	list.addEventListener('click', (e) => {
		const tab = (e.target as Element).closest<HTMLButtonElement>('[role=tab]');
		if (tab) select(tab.dataset[key] ?? '', false);
	});
	list.addEventListener('keydown', (e) => {
		const all = tabs();
		const i = all.findIndex((t) => t.getAttribute('aria-selected') === 'true');
		const to = e.key === 'ArrowRight' ? i + 1 : e.key === 'ArrowLeft' ? i - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? all.length - 1 : -1;
		if (to < 0 && e.key !== 'ArrowLeft') return;
		e.preventDefault();
		const next = all[(to + all.length) % all.length];
		if (next) select(next.dataset[key] ?? '', true);
	});
}

function markTab(list: HTMLElement, panels: HTMLElement[], key: string, id: string, focus: boolean): void {
	for (const tab of list.querySelectorAll<HTMLButtonElement>('[role=tab]')) {
		const on = tab.dataset[key] === id;
		tab.setAttribute('aria-selected', String(on));
		tab.tabIndex = on ? 0 : -1;
		if (on && focus) tab.focus({ preventScroll: true });
		// Keep the selected tab visible in a tab row that scrolls sideways, without moving the page.
		if (on && tab.offsetLeft < list.scrollLeft) list.scrollLeft = tab.offsetLeft;
		else if (on && tab.offsetLeft + tab.offsetWidth > list.scrollLeft + list.clientWidth) list.scrollLeft = tab.offsetLeft + tab.offsetWidth - list.clientWidth;
	}
	for (const p of panels) p.hidden = p.dataset[key] !== id && p.id !== 'ck-p-' + id;
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
		panelToggle: byId<HTMLButtonElement>('ck-panel-toggle'),
		stage: byId('ck-stage'),
		canvas: byId('ck-canvas'),
		gl: byId<HTMLCanvasElement>('ck-gl'),
		zin: byId<HTMLButtonElement>('ck-zin'),
		zout: byId<HTMLButtonElement>('ck-zout'),
		fit: byId<HTMLButtonElement>('ck-fit'),
		three: byId<HTMLButtonElement>('ck-3d'),
		measureToggle: byId<HTMLButtonElement>('ck-measure-toggle'),
		sheet: byId<HTMLButtonElement>('ck-sheet'),
		full: byId<HTMLButtonElement>('ck-full'),
		readout: byId('ck-readout'),
		notice: byId('ck-notice'),
		noticeText: byId('ck-notice-text'),
		noticeMore: byId<HTMLButtonElement>('ck-notice-more'),
		noticeClose: byId<HTMLButtonElement>('ck-notice-close'),
		mbox: byId('ck-mbox'),
		mboxText: byId('ck-mbox-text'),
		mClose: byId<HTMLButtonElement>('ck-m-close'),
		mUndo: byId<HTMLButtonElement>('ck-m-undo'),
		mClear: byId<HTMLButtonElement>('ck-m-clear'),
		selchip: byId('ck-selchip'),
		selchipText: byId('ck-selchip-text'),
		selchipMore: byId<HTMLButtonElement>('ck-selchip-more'),
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
		panel: byId('ck-panel'),
		tabs: root.querySelector<HTMLElement>('.ck-tabs')!,
		summary: byId('ck-summary'),
		modelWrap: byId('ck-model-wrap'),
		model: byId<HTMLSelectElement>('ck-model'),
		info: byId('ck-info'),
		notDrawn: byId('ck-notdrawn'),
		notDrawnCount: byId('ck-notdrawn-count'),
		notDrawnList: byId('ck-notdrawn-list'),
		warnCount: byId('ck-warn-count'),
		warnings: byId('ck-warnings'),
		layerCount: byId('ck-layer-count'),
		layerFilter: byId<HTMLInputElement>('ck-layer-filter'),
		layers: byId('ck-layers'),
		layersHint: byId('ck-layers-hint'),
		layersAll: byId<HTMLButtonElement>('ck-layers-all'),
		layersNone: byId<HTMLButtonElement>('ck-layers-none'),
		selHeading: byId('ck-h-sel'),
		selHint: byId('ck-sel-hint'),
		sel: byId('ck-sel'),
		selZoom: byId<HTMLButtonElement>('ck-sel-zoom'),
		selClear: byId<HTMLButtonElement>('ck-sel-clear'),
		selProps: byId('ck-sel-props'),
		selPropsCount: byId('ck-sel-props-count'),
		selPropsList: byId('ck-sel-props-list'),
		selJsonWrap: byId('ck-sel-json-wrap'),
		selJson: byId('ck-sel-json'),
		selJsonNote: byId('ck-sel-json-note'),
		selCopy: byId<HTMLButtonElement>('ck-sel-copy'),
		expSvg: byId<HTMLButtonElement>('ck-exp-svg'),
		expDxf: byId<HTMLButtonElement>('ck-exp-dxf'),
		dxfVersion: byId<HTMLSelectElement>('ck-dxf-version'),
		dxfCodepage: byId<HTMLSelectElement>('ck-dxf-codepage'),
		expJson: byId<HTMLButtonElement>('ck-exp-json'),
		expGml: byId<HTMLButtonElement>('ck-exp-gml'),
		expResult: byId('ck-exp-result'),
		unit: byId<HTMLSelectElement>('ck-unit'),
		unitApply: byId<HTMLButtonElement>('ck-unit-apply'),
		unitNote: byId('ck-unit-note'),
		explode: byId<HTMLButtonElement>('ck-explode'),
		reset: byId<HTMLButtonElement>('ck-reset'),
		changeResult: byId('ck-change-result'),
		chkInspect: byId<HTMLButtonElement>('ck-chk-inspect'),
		chkInspectOut: byId('ck-chk-inspect-out'),
		chkLimit: byId<HTMLInputElement>('ck-chk-limit'),
		chkLimitRun: byId<HTMLButtonElement>('ck-chk-limit-run'),
		chkLimitOut: byId('ck-chk-limit-out'),
		chkCut: byId<HTMLInputElement>('ck-chk-cut'),
		chkCutValue: byId('ck-chk-cut-value'),
		chkCutRun: byId<HTMLButtonElement>('ck-chk-cut-run'),
		chkCutOut: byId('ck-chk-cut-out'),
		chkGml: byId('ck-chk-gml'),
		gmlValidate: byId<HTMLButtonElement>('ck-gml-validate'),
		gmlValidateOut: byId('ck-gml-validate-out'),
		gmlProfile: byId<HTMLSelectElement>('ck-gml-profile'),
		gmlPreflight: byId<HTMLButtonElement>('ck-gml-preflight'),
		gmlPreflightOut: byId('ck-gml-preflight-out'),
		calls: byId('ck-calls'),
		callsCount: byId('ck-calls-count'),
		callsClear: byId<HTMLButtonElement>('ck-calls-clear'),
		status: byId('ck-status'),
		engineState: byId('ck-engine-state'),
	};
	const tabPanels = TAB_IDS.map((id) => byId('ck-p-' + id));

	const engine = new Engine();
	engine.onState = (s) => (el.engineState.textContent = 'engine ' + s);
	const sheet2d = new Sheet2d(el.canvas);
	const can3d = Scene3d.supported();
	let scene: Scene3d | null = null;

	// --- state ---
	let doc: OpenResult | null = null;
	let model = 0;
	/** Bumped whenever the document changes; results of older requests are dropped. */
	let generation = 0;
	/** Bumped when another file is opened (or the viewer is emptied): panel results of the old file are dropped. */
	let fileToken = 0;
	let loaded: Loaded | null = null;
	/** Model the viewports of the current layout look at. */
	let layoutSource = 0;
	let view: V.View | null = null;
	/** The view is the fitted one (no pan or zoom since): a resize fits again. */
	let fitted = true;
	let mode: '2d' | '3d' = '2d';
	let mesh: MeshResult | null = null;
	let meshKey = '';
	const hidden = new Set<string>();
	let selection: Selection | null = null;
	let sheet: Sheet = document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
	let measuring = false;
	let mPts: Pt[] = [];
	let mClosed = false;
	let mCursor: Pt | null = null;
	let busyToken = 0;
	let busyTimer = 0;
	let touchActive = false;
	const timings = { read: Number.NaN, render: Number.NaN, svgBytes: 0 };
	const callLog: CallRecord[] = [];
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
			...(detail ? [h('p', { class: 'ck-alert-detail' }, h('code', {}, kind), ' ', cleanMessage(kind, detail))] : []),
			close
		);
		el.alert.hidden = false;
	}
	const clearAlert = () => (el.alert.hidden = true);

	function setState(state: 'empty' | 'ready'): void {
		el.root.dataset.state = state;
	}

	/** Starts (or, with null, ends) a busy period; the overlay shows after BUSY_DELAY_MS. */
	function setBusy(phase: Phase | null): number {
		const token = ++busyToken;
		clearTimeout(busyTimer);
		if (!phase) {
			el.busy.hidden = true;
			return token;
		}
		el.busyPhase.textContent = PHASE_TEXT[phase];
		if (el.busy.hidden) {
			const started = performance.now();
			busyTimer = window.setTimeout(() => {
				if (token !== busyToken) return;
				const hadFocus = el.root.contains(document.activeElement);
				el.busy.hidden = false;
				el.busyTime.textContent = '';
				const tick = () => {
					if (el.busy.hidden) return;
					el.busyTime.textContent = ((performance.now() - started) / 1000).toFixed(1) + ' s';
					setTimeout(tick, 100);
				};
				tick();
				if (hadFocus) el.cancel.focus({ preventScroll: true });
			}, BUSY_DELAY_MS);
		}
		return token;
	}
	const phase = (p: Phase) => {
		el.busyPhase.textContent = PHASE_TEXT[p];
	};

	function setEnabled(on: boolean): void {
		const always = [el.zin, el.zout, el.fit, el.sheet, el.expSvg, el.expDxf, el.dxfVersion, el.expJson, el.layersAll, el.layersNone, el.unit, el.unitApply, el.explode, el.reset];
		const checks = [el.chkInspect, el.chkLimit, el.chkLimitRun, el.chkCut, el.chkCutRun];
		for (const b of [...always, ...checks]) b.disabled = !on;
		el.measureToggle.disabled = !on || mode === '3d';
		el.dxfCodepage.disabled = !on || !LEGACY_DXF_VERSIONS.includes(el.dxfVersion.value as DxfVersion);
		// A document that names no unit has nothing to convert from.
		const unitless = !doc || doc.info.units === 'unitless';
		el.unit.disabled = el.unitApply.disabled = !on || unitless;
		el.unitNote.hidden = !on || !unitless;
		const gml = on && doc?.info.format === 'city_gml';
		el.expGml.disabled = !gml;
		el.chkGml.hidden = !gml;
	}

	// --- tabs ---
	function selectTab(id: string, focus = false): void {
		if (!(TAB_IDS as readonly string[]).includes(id)) return;
		markTab(el.tabs, tabPanels, 'tab', id, focus);
	}
	wireTabs(el.tabs, selectTab, 'tab');
	/** Opens a tab and brings the panel into view (it is below the drawing on narrow screens). */
	function revealTab(id: TabId): void {
		if (el.root.dataset.panel === 'closed') setPanel(true);
		selectTab(id);
		if (el.panel.getBoundingClientRect().top > window.innerHeight - 80) el.panel.scrollIntoView({ block: 'start', behavior: 'smooth' });
	}

	// --- code examples under the viewer ---
	const codeTasks = document.querySelector<HTMLElement>('.ck-code-tasks');
	const codePanels = [...document.querySelectorAll<HTMLElement>('.ck-code-task')];
	function selectTask(id: string, focus = false): void {
		if (!codeTasks || !codePanels.some((p) => p.dataset.task === id)) return;
		markTab(codeTasks, codePanels, 'task', id, focus);
	}
	if (codeTasks) {
		wireTabs(codeTasks, selectTask, 'task');
		selectTask(codePanels[0]?.dataset.task ?? '');
	}

	// --- stage geometry ---
	const stageSize = () => {
		const r = el.stage.getBoundingClientRect();
		return { w: Math.max(1, r.width), h: Math.max(1, r.height), left: r.left, top: r.top };
	};
	/** The tool column and the note strip cover parts of the stage; fitting keeps the drawing clear of them. */
	const insets = (): V.Insets => {
		const { w, h } = stageSize();
		const top = el.notice.hidden ? 0 : Math.min(el.notice.offsetHeight + 16, h / 3);
		return { left: 0, top, right: w > 200 ? 56 : 0, bottom: 0 };
	};

	let frame = 0;
	function applyView(): void {
		if (frame) return;
		frame = requestAnimationFrame(() => {
			frame = 0;
			if (!view) return;
			const { w, h } = stageSize();
			sheet2d.setViewBox(V.viewBoxOf(view, w, h));
			updateSelBox();
		});
	}

	function setView(next: V.View, moved = true): void {
		view = next;
		if (moved) fitted = false;
		applyView();
	}

	function fitView(): void {
		if (mode === '3d') {
			scene?.home();
			return;
		}
		if (!view) return;
		const { w, h } = stageSize();
		setView(V.fitBox(view.home, view.home, w, h, insets()), false);
		fitted = true;
		showCentre();
	}

	function zoomBy(factor: number, px?: number, py?: number): void {
		if (mode === '3d') {
			scene?.zoom(factor);
			return;
		}
		if (!view) return;
		const { w, h } = stageSize();
		setView(V.zoomAt(view, w, h, factor, px ?? w / 2, py ?? h / 2));
	}

	const units = () => (doc ? unitSymbol(doc.info.units) : '');
	function showCoords(px: number, py: number): void {
		if (mode === '3d') {
			el.readout.textContent = '3D   drag to turn   Shift-drag to pan';
			return;
		}
		if (!view) {
			el.readout.textContent = '';
			return;
		}
		const { w, h } = stageSize();
		const p = V.toWorld(view, w, h, px, py);
		const u = units();
		el.readout.textContent = `${loaded?.layout ? 'Sheet  ' : ''}X ${V.formatCoord(p.x, view.s)}  Y ${V.formatCoord(p.y, view.s)}${u ? '  ' + u : ''}`;
	}
	function showCentre(): void {
		const { w, h } = stageSize();
		showCoords(w / 2, h / 2);
	}

	// --- rendering ---
	const canvasNote = (text: string) => el.canvas.append(h('p', { class: 'ck-canvas-note' }, text));

	/** Renders the current model into the 2D sheet. `start` is a part of the drawing to show first. */
	async function render(fit: boolean, start: V.Box | null = null): Promise<void> {
		if (!doc) return;
		const info = doc.info;
		const previous = view;
		sheet2d.clear();
		loaded = null;
		view = null;
		if (!(info.models[model]?.entities ?? 0)) {
			canvasNote('This model has no entities to draw.');
			buildLayers();
			showCentre();
			return;
		}
		let result: RenderResult;
		try {
			result = await engine.call<RenderResult>({ type: 'render', model }, phase);
		} catch (e) {
			const err = e as DemoError;
			if (err.kind === 'trap' || err.kind === 'cancelled') throw err;
			canvasNote(
				err.kind === 'limit_exceeded'
					? 'No 2D sheet: this model is too large to render as SVG in the browser. Document details, layers, warnings, checks and exports are still available.'
					: 'No 2D sheet: ' + err.message
			);
			buildLayers();
			return;
		}
		const m = info.models[model];
		loaded = sheet2d.load(result, sheet, Boolean(m?.is3d) && !result.layout);
		layoutSource = result.layout?.source ?? 0;
		timings.svgBytes = result.svg.length + (result.layout?.sourceSvg.length ?? 0);
		for (const name of sheet2d.layerNames()) if (hidden.has(name)) sheet2d.setLayerHidden(name, true);
		if (loaded.empty) {
			canvasNote(loaded.noWindows ? 'This layout has no viewport that shows the model, and no entities of its own.' : 'Nothing in this model can be drawn as a 2D sheet.');
		}
		const { w, h } = stageSize();
		const home = loaded.home;
		const keep = !fit && previous && previous.home.x === home.x && previous.home.y === home.y && previous.home.w === home.w && previous.home.h === home.h;
		if (keep && previous) view = { ...previous, home };
		else {
			view = V.fitBox(home, start ?? home, w, h, insets());
			fitted = !start;
		}
		applyView();
		buildLayers();
		showCentre();
	}

	// --- 3D ---
	/** Set when WebGL2 turned out not to work here: the 3D view is not offered again. */
	let gl3dBroken = false;
	const has3d = () => can3d && !gl3dBroken && Boolean(doc?.extras?.[model]?.spatial);

	function syncMode(): void {
		el.root.dataset.mode = mode;
		el.gl.hidden = mode !== '3d';
		el.three.hidden = !has3d();
		el.three.setAttribute('aria-pressed', String(mode === '3d'));
		el.measureToggle.disabled = !doc || mode === '3d';
		el.stage.setAttribute('aria-label', mode === '3d' ? 'Drawing, 3D view' : 'Drawing');
	}

	async function setMode(next: '2d' | '3d'): Promise<void> {
		if (next === '3d' && !has3d()) next = '2d';
		let fresh: MeshResult | null = null;
		if (next === '3d') {
			const key = generation + ':' + model;
			if (meshKey !== key || !mesh) {
				const gen = generation;
				try {
					fresh = await engine.call<MeshResult>({ type: 'mesh', model }, phase);
					if (gen !== generation) return;
					scene ??= new Scene3d(el.gl);
					mesh = fresh;
					meshKey = key;
				} catch (e) {
					const kind = e instanceof DemoError ? e.kind : 'internal';
					if (kind === 'trap' || kind === 'cancelled') throw e;
					if (gen !== generation) return;
					// No mesh or no working WebGL2: keep the view from above.
					if (!(e instanceof DemoError)) gl3dBroken = true;
					fresh = null;
					next = '2d';
					announce('The 3D view is not available here; the model is shown from above.');
				}
			}
			if (next === '3d') stopMeasure();
		}
		if (mode !== next) clearSelection(false);
		mode = next;
		syncMode();
		if (mode === '3d' && scene) {
			// The canvas is visible now, so the first fit uses its real shape.
			if (fresh) scene.setMesh(fresh);
			scene.setSheet(sheet);
			scene.setHidden(hidden);
			scene.invalidate();
		}
		buildLayers();
		showCentre();
	}

	// --- panels ---
	function fillDocument(): void {
		if (!doc) return;
		const i = doc.info;
		const total = i.models.reduce((n, m) => n + m.entities, 0);
		const m = i.models[model];
		const shown = m ? `Showing ${m.name || 'model ' + (model + 1)} (${plural(m.entities, 'entity', 'entities')}${loaded?.layout ? ', a paper layout' : mode === '3d' ? ', in 3D' : m.is3d ? ', 3D, seen from above' : ''}). ` : '';
		el.summary.textContent =
			`${doc.name}: ${formatWithVersion(i.format, i.version)}, units ${unitName(i.units)}, ${plural(total, 'entity', 'entities')} in ${plural(i.models.length, 'model')}. ` +
			shown +
			`${plural(i.layerCount, 'layer')}, ${i.warningCount ? plural(i.warningCount, 'warning') : 'no warnings'}.`;

		const rows: [string, string | Node][] = [
			['File', doc.name],
			['Size', formatBytes(doc.bytes)],
			['Format', formatName(i.format)],
			['Version', versionLabel(i.format, i.version)],
		];
		if (i.application) rows.push(['Application', i.application]);
		if (doc.header?.codepage) rows.push(['Code page', doc.header.codepage]);
		const changed = doc.changes.units ? ` (converted from the file's units)` : '';
		rows.push(
			['Units', unitName(i.units) + (i.metersPerUnit ? ` (${i.metersPerUnit} m per unit)` : '') + changed],
			['Entities', total.toLocaleString('en-US') + (doc.changes.exploded ? ' (blocks exploded)' : '')],
			['Layers', i.layerCount.toLocaleString('en-US')],
			['Blocks', i.blockCount.toLocaleString('en-US')],
			['Models', i.models.length.toLocaleString('en-US')]
		);
		if (doc.kinds) {
			const kinds = Object.entries(doc.kinds).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
			rows.push(['Kinds', h('span', { class: 'ck-kinds' }, ...kinds.map(([k, n]) => h('span', {}, mono(k), ' ' + n.toLocaleString('en-US'))))]);
		}
		if (Number.isFinite(timings.read)) rows.push(['Read in', formatMs(timings.read)]);
		if (loaded && Number.isFinite(timings.render)) {
			rows.push(['Rendered in', `${formatMs(timings.render)}, ${formatBytes(timings.svgBytes)} of SVG, ${plural(loaded.nodes, 'node')}`]);
		}
		fillDl(el.info, rows);

		el.model.replaceChildren(
			...i.models.map((mm, idx) => {
				const kind = doc?.extras?.[idx]?.viewports ? 'layout, ' : '';
				return h('option', { value: String(idx) }, `${mm.name || 'Model ' + (idx + 1)} (${kind}${mm.entities.toLocaleString('en-US')})`);
			})
		);
		el.model.value = String(model);
		el.modelWrap.hidden = i.models.length < 2;

		const missing = Object.entries(doc.notDrawn).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
		const missingTotal = missing.reduce((n, [, c]) => n + c, 0);
		el.notDrawn.hidden = !missing.length;
		el.notDrawnCount.textContent = missing.length ? `(${missingTotal.toLocaleString('en-US')})` : '';
		el.notDrawnList.replaceChildren(...missing.map(([name, c]) => h('li', {}, mono(sourceTypeName(name)), ' ' + c.toLocaleString('en-US'))));
	}

	/** The strip over the drawing that says what is missing from it. */
	function showNotice(): void {
		const missing = doc ? Object.entries(doc.notDrawn).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])) : [];
		if (!doc || !missing.length) {
			el.notice.hidden = true;
			return;
		}
		const n = missing.reduce((s, [, c]) => s + c, 0);
		const total = doc.info.models.reduce((s, m) => s + m.entities, 0);
		const top = missing.slice(0, 3).map(([name, c]) => `${c} ${sourceTypeName(name)}`);
		const rest = missing.length - top.length;
		el.noticeText.textContent = `${n.toLocaleString('en-US')} of ${plural(total, 'entity', 'entities')} ${n === 1 ? 'is' : 'are'} read but not drawn: ${top.join(', ')}${rest > 0 ? `, and ${plural(rest, 'more type')}` : ''}.`;
		el.notice.hidden = false;
	}

	/** Layers hidden when a drawing opens: off or frozen in the file, and the non-plotting Defpoints layer. */
	function defaultHidden(): void {
		hidden.clear();
		for (const l of doc?.layers ?? []) if (!l.visible || l.frozen || /^defpoints$/i.test(l.name)) hidden.add(l.name);
	}

	function drawnLayers(): Set<string> {
		if (mode === '3d' && mesh) return new Set(mesh.layers.filter((l) => l.tri[1] || l.seg[1]).map((l) => l.name));
		return new Set(sheet2d.layerNames());
	}

	function buildLayers(): void {
		if (!doc) {
			el.layers.replaceChildren();
			el.layerCount.textContent = '';
			el.layersHint.hidden = false;
			el.layerFilter.hidden = true;
			return;
		}
		el.layersHint.hidden = true;
		const drawn = drawnLayers();
		const names = doc.layers.map((l) => l.name);
		for (const n of drawn) if (!names.includes(n)) names.push(n);
		const meta = new Map(doc.layers.map((l) => [l.name, l]));
		el.layerCount.textContent = `(${names.length})`;
		el.layerFilter.hidden = names.length <= 12;
		if (!names.length) {
			el.layers.replaceChildren(h('li', { class: 'ck-hint' }, 'This drawing has no layers.'));
			return;
		}
		const filter = el.layerFilter.hidden ? '' : el.layerFilter.value.trim().toLowerCase();
		el.layers.replaceChildren(
			...names
				.filter((name) => !filter || name.toLowerCase().includes(filter))
				.map((name) => {
					const on = drawn.has(name);
					const l = meta.get(name);
					const id = 'ck-layer-' + names.indexOf(name);
					const cb = h('input', { type: 'checkbox', id });
					cb.checked = on && !hidden.has(name);
					cb.disabled = !on;
					cb.addEventListener('change', () => toggleLayer(name, cb.checked));
					const sw = h('span', { class: 'ck-swatch', 'aria-hidden': 'true' });
					const colour = l ? layerSwatch(l.color, sheet === 'dark') : '';
					if (colour) sw.style.background = colour;
					else sw.classList.add('ck-swatch--none');
					const note = l?.frozen ? 'frozen in file' : l && !l.visible ? 'off in file' : /^defpoints$/i.test(name) ? 'not plotted' : !on ? 'nothing drawn' : '';
					const label = h('label', { for: id }, sw, h('span', { class: 'ck-lname' }, name || '(unnamed)'));
					if (note) label.append(h('span', { class: 'ck-lnote' }, note));
					return h('li', {}, cb, label);
				})
		);
	}

	function toggleLayer(name: string, on: boolean): void {
		if (on) hidden.delete(name);
		else hidden.add(name);
		sheet2d.setLayerHidden(name, !on);
		scene?.setHidden(hidden);
		if (!on && (selection?.hit?.layer === name || selection?.detail?.layer === name)) clearSelection(false);
	}

	function setAllLayers(on: boolean): void {
		for (const name of drawnLayers()) toggleLayer(name, on);
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
	/** Fallback highlight (no JSON copy of the document): a box around the entity, padded in pixels. */
	function updateSelBox(): void {
		const hit = selection?.hit;
		if (!hit || !view || selection?.detail?.highlight) {
			sheet2d.setBox(null);
			return;
		}
		const pad = 4 * view.s;
		const [x0, y0] = hit.bbox.min;
		const [x1, y1] = hit.bbox.max;
		sheet2d.setBox({ x: x0 - pad, y: -y1 - pad, w: Math.max(0, x1 - x0) + 2 * pad, h: Math.max(0, y1 - y0) + 2 * pad }, selection?.inSource);
	}

	function clearSelection(speak = true): void {
		const had = Boolean(selection);
		selection = null;
		sheet2d.setHighlight(null);
		sheet2d.setBox(null);
		scene?.setSelected(-1);
		el.sel.replaceChildren();
		el.selHint.hidden = false;
		el.selClear.hidden = true;
		el.selZoom.hidden = true;
		el.selProps.hidden = true;
		el.selJsonWrap.hidden = true;
		el.selchip.hidden = true;
		if (speak && had) announce('Selection cleared.');
	}

	function fillSelection(): void {
		const sel = selection;
		if (!sel) return;
		const hit = sel.hit;
		const d = sel.detail;
		const fmt = (n: number) => (view ? V.formatCoord(n, view.s) : String(n));
		// A pick inside a block reference reports the part that was hit; the entity is the reference.
		const kind = d?.kind ?? hit?.kind ?? '';
		const layer = d?.layer ?? hit?.layer ?? '';
		const rows: [string, string | Node][] = [
			['Kind', mono(kind)],
			['Layer', layer || '(unnamed)'],
		];
		if (hit && d && hit.kind !== d.kind) rows.push(['Picked part', h('span', {}, mono(hit.kind), hit.layer && hit.layer !== layer ? ' on ' + hit.layer : '')]);
		if (sel.path.length === 1) rows.push(['Index', String(sel.path[0])]);
		const id = hit?.id ?? d?.id ?? null;
		if (id !== null) rows.push(['Handle / id', mono(String(id))]);
		if (d?.parents.length) rows.push(['Part of', d.parents.join(' › ')]);
		if (hit?.color) rows.push(['Colour in file', hit.color]);
		const block = d?.block ?? hit?.block;
		if (block) rows.push(['Block', mono(block)]);
		if (hit?.text) rows.push(['Text', hit.text]);
		if (hit) {
			const [x0, y0] = hit.bbox.min;
			const [x1, y1] = hit.bbox.max;
			const u = units();
			rows.push(['Bounds', mono(`(${fmt(x0)}, ${fmt(y0)}) to (${fmt(x1)}, ${fmt(y1)})${u ? ' ' + u : ''}`)]);
			if (hit.truncated) rows.push(['Note', 'Some metadata was shortened to the demo limits.']);
		}
		for (const a of d?.attributes ?? hit?.attributes.map((x) => ({ ...x, invisible: false })) ?? []) rows.push(['Attribute ' + a.tag, a.value + (a.invisible ? ' (not displayed)' : '')]);
		if (sel.inSource) rows.push(['Seen through', 'a viewport of this layout']);
		fillDl(el.sel, rows);
		el.selHint.hidden = true;
		el.selClear.hidden = false;
		el.selZoom.hidden = !hit || sel.inSource || mode === '3d';
		el.selProps.hidden = !d?.props.length;
		if (d?.props.length) {
			el.selPropsCount.textContent = `(${d.props.length})`;
			fillDl(el.selPropsList, d.props.map(([k, v]) => [k, v]));
		}
		el.selJsonWrap.hidden = !d;
		if (d) {
			el.selJson.textContent = d.json;
			el.selJsonNote.textContent = d.jsonTruncated ? 'Cut to the demo limit; the JSON export has all of it.' : '';
		}
		el.selchipText.replaceChildren(mono(kind), ' on ' + (layer || '(unnamed)'));
		el.selchip.hidden = false;
	}

	/** Loads the full entity (properties, JSON, and the highlight for a 2D pick) into the selection. */
	async function loadDetail(sel: Selection): Promise<void> {
		if (!doc?.extras) return;
		try {
			const detail = await engine.call<EntityResult>({ type: 'entity', model: sel.model, path: sel.path, highlight: sel.face < 0 });
			if (selection !== sel) return;
			sel.detail = detail;
			if (sel.face < 0 && detail.highlight && !sheet2d.setHighlight(detail.highlight, sel.inSource)) sel.detail = { ...detail, highlight: null };
			updateSelBox();
			fillSelection();
		} catch (e) {
			const err = e as DemoError;
			if (err.kind === 'trap') handleError(err);
		}
	}

	function select(sel: Selection): void {
		selection = sel;
		sheet2d.setHighlight(null);
		scene?.setSelected(sel.face);
		updateSelBox();
		fillSelection();
		const kind = sel.hit?.kind ?? (mesh && sel.face >= 0 ? mesh.faces[sel.face]?.kind : '') ?? '';
		const layer = sel.hit?.layer ?? (mesh && sel.face >= 0 ? mesh.faces[sel.face]?.layer : '') ?? '';
		announce(`Selected ${kind} on layer ${layer || 'unnamed'}.`);
		void loadDetail(sel);
	}

	async function pickAt(px: number, py: number): Promise<void> {
		if (!doc) return;
		if (mode === '3d') {
			if (!scene || !mesh) return;
			const face = scene.pick(px, py);
			const ref = face >= 0 ? mesh.faces[face] : undefined;
			if (!ref) {
				clearSelection(false);
				announce('Nothing to pick there.');
				return;
			}
			select({ model, path: ref.path, hit: null, face, inSource: false, detail: null });
			return;
		}
		if (!view || !loaded) return;
		const { w, h } = stageSize();
		const p = V.toWorld(view, w, h, px, py);
		if (measuring) {
			addMeasurePoint(p);
			return;
		}
		// On a layout, a point inside a viewport picks in model space.
		const through = loaded.layout ? paperToModel(loaded.layout, p.x, p.y) : null;
		const pickModel = through ? layoutSource : model;
		try {
			const hit = await engine.call<PickedEntity | null>({
				type: 'pick',
				model: pickModel,
				x: through ? through.x : p.x,
				y: through ? through.y : p.y,
				tolerance: PICK_TOLERANCE_PX * view.s * (through ? through.scale : 1),
				excludedLayers: [...hidden],
			});
			if (hit) select({ model: pickModel, path: [hit.index], hit, face: -1, inSource: Boolean(through), detail: null });
			else {
				clearSelection(false);
				announce('Nothing to pick there.');
			}
		} catch (e) {
			handleError(e);
		}
	}

	function zoomToSelection(): void {
		const hit = selection?.hit;
		if (!hit || !view || mode === '3d') return;
		const { w, h } = stageSize();
		const [x0, y0] = hit.bbox.min;
		const [x1, y1] = hit.bbox.max;
		// A point or a line has no area: give the box a size of at least 2 % of the drawing.
		const min = Math.max(view.home.w, view.home.h) * 0.02;
		const bw = Math.max(x1 - x0, min);
		const bh = Math.max(y1 - y0, min);
		const box = { x: (x0 + x1) / 2 - bw * 0.75, y: -(y0 + y1) / 2 - bh * 0.75, w: bw * 1.5, h: bh * 1.5 };
		setView(V.fitBox(view.home, box, w, h, insets()));
		showCentre();
	}

	// --- measure ---
	function fillMeasure(): void {
		el.mbox.hidden = !measuring;
		el.measureToggle.setAttribute('aria-pressed', String(measuring));
		el.root.dataset.measure = measuring ? 'on' : '';
		if (!measuring) return;
		const u = units();
		const len = pathLength(mClosed && mPts.length > 2 ? [...mPts, mPts[0]!] : mPts);
		const withUnit = (v: number, square = false) => formatMeasure(v) + (u ? ' ' + u + (square ? '²' : '') : '');
		let text = 'Click points in the drawing to measure a path.';
		if (mPts.length === 1) text = 'First point set. Click the next one.';
		else if (mPts.length > 1 && mClosed) text = `Perimeter ${withUnit(len)}   Area ${withUnit(polygonArea(mPts), true)}`;
		else if (mPts.length > 1) text = `Length ${withUnit(len)}   ${plural(mPts.length - 1, 'segment')}`;
		el.mboxText.textContent = text + (loaded?.layout ? '   (sheet units)' : '');
		el.mClose.disabled = mPts.length < 3;
		el.mClose.textContent = mClosed ? 'Open shape' : 'Close shape';
		el.mUndo.disabled = !mPts.length;
		el.mClear.disabled = !mPts.length;
		sheet2d.setMeasure(mPts, mClosed, mCursor);
	}
	function addMeasurePoint(p: Pt): void {
		if (mClosed) {
			mPts = [];
			mClosed = false;
		}
		mPts.push(p);
		mCursor = null;
		fillMeasure();
	}
	function startMeasure(): void {
		if (!doc || mode === '3d') return;
		measuring = true;
		clearSelection(false);
		fillMeasure();
		announce('Measuring. Click points in the drawing; Escape stops.');
	}
	function stopMeasure(): void {
		if (!measuring && !mPts.length) return;
		measuring = false;
		mPts = [];
		mClosed = false;
		mCursor = null;
		sheet2d.setMeasure([], false, null);
		fillMeasure();
	}

	// --- opening ---
	function resetDocument(): void {
		doc = null;
		generation++;
		fileToken++;
		loaded = null;
		view = null;
		mesh = null;
		meshKey = '';
		mode = '2d';
		hidden.clear();
		sheet2d.clear();
		stopMeasure();
		clearSelection(false);
		el.readout.textContent = '';
		el.notice.hidden = true;
		el.filename.replaceChildren(h('span', { class: 'ck-muted' }, 'No file open'));
		el.summary.textContent = 'No drawing open. Open a file or a sample to see its format, units, layers and entities here.';
		el.info.replaceChildren();
		el.modelWrap.hidden = true;
		el.notDrawn.hidden = true;
		for (const out of [el.expResult, el.changeResult, el.chkInspectOut, el.chkLimitOut, el.chkCutOut, el.gmlValidateOut, el.gmlPreflightOut]) out.hidden = true;
		buildLayers();
		fillWarnings();
		setEnabled(false);
		syncMode();
		setTouch(false);
		el.touch.hidden = true;
		setState('empty');
	}

	function handleError(e: unknown, name = ''): void {
		const err = e instanceof DemoError ? e : new DemoError('internal', e instanceof Error ? e.message : String(e));
		if (err.kind === 'trap') {
			engine.restart('trap', err.message);
			resetDocument();
		}
		if (err.kind === 'cancelled') return;
		const hint = err.kind === 'unknown_format' && name ? extensionHint(name) : '';
		showAlert(err.kind, hint ? `${err.message}. ${hint}` : err.message);
		announce(explainError(err.kind).title + '.');
	}

	/** Shows a document the worker has just read or changed: sheet, mode and every panel. */
	async function showDocument(result: OpenResult, opts: { keepModel: boolean; start: V.Box | null }): Promise<void> {
		doc = result;
		generation++;
		mesh = null;
		meshKey = '';
		model = opts.keepModel && result.info.models[model] ? model : result.model;
		stopMeasure();
		clearSelection(false);
		mode = '2d';
		setEnabled(true);
		showNotice();
		await render(true, opts.start);
		syncMode();
		// Models with surfaces that leave the XY plane (CityGML buildings, 3D faces) open in 3D.
		if (has3d()) await setMode('3d');
		fillDocument();
		fillWarnings();
		el.chkLimit.value = String(Math.max(1, Math.floor(result.info.models.reduce((n, m) => n + m.entities, 0) / 2)));
		setState('ready');
	}

	async function openBuffer(name: string, buffer: ArrayBuffer, sampleId: string | null, quiet = false): Promise<void> {
		clearAlert();
		if (buffer.byteLength > MAX_FILE_BYTES) {
			showAlert('too_large', `${name} is ${formatBytes(buffer.byteLength)}; the demo opens files up to ${formatBytes(MAX_FILE_BYTES)}.`);
			return;
		}
		const token = setBusy('engine');
		try {
			await engine.ready;
			phase('parse');
			const result = await engine.call<OpenResult>({ type: 'open', name, buffer }, phase, [buffer]);
			if (token !== busyToken) return;
			for (const out of [el.expResult, el.changeResult, el.chkInspectOut, el.chkLimitOut, el.chkCutOut, el.gmlValidateOut, el.gmlPreflightOut]) out.hidden = true;
			doc = result;
			fileToken++;
			defaultHidden();
			const start = sampleId ? (sampleById(sampleId)?.start ?? null) : null;
			await showDocument(result, { keepModel: false, start: start ? { x: start[0], y: -start[3], w: start[2] - start[0], h: start[3] - start[1] } : null });
			el.filename.replaceChildren(h('span', { class: 'ck-fname' }, name), h('span', { class: 'ck-fmt' }, formatWithVersion(result.info.format, result.info.version)));
			el.sample.value = sampleId ?? '';
			if (!quiet) {
				const url = new URL(location.href);
				if (sampleId) url.searchParams.set('sample', sampleId);
				else url.searchParams.delete('sample');
				history.replaceState(null, '', url);
			}
			setBusy(null);
			el.touch.hidden = !coarse.matches;
			if (!quiet) el.stage.focus({ preventScroll: true });
			const i = result.info;
			announce(
				`Opened ${name}: ${formatWithVersion(i.format, i.version)}, ${plural(i.models.reduce((n, m) => n + m.entities, 0), 'entity', 'entities')}, ${plural(i.layerCount, 'layer')}` +
					(i.warningCount ? `, ${plural(i.warningCount, 'warning')} (see the File tab).` : '.') +
					(start ? ' Showing a part of the sheet; Fit shows all of it.' : '')
			);
		} catch (e) {
			if (token === busyToken) setBusy(null);
			handleError(e, name);
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

	async function openSample(id: string, quiet = false): Promise<void> {
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
		await openBuffer(s.path.split('/').pop() ?? s.id, buf, s.id, quiet);
	}

	// --- results shown inside the panel ---
	/** Runs a panel action: the button is busy while it runs, errors appear in `out`, not as an alert. */
	async function runInPanel(button: HTMLButtonElement, out: HTMLElement, work: () => Promise<Node[]>): Promise<void> {
		if (button.getAttribute('aria-busy') === 'true') return;
		button.setAttribute('aria-busy', 'true');
		const token = fileToken;
		try {
			const nodes = await work();
			if (token !== fileToken) return;
			out.className = 'ck-result';
			out.replaceChildren(...nodes);
			out.hidden = false;
		} catch (e) {
			const err = e instanceof DemoError ? e : new DemoError('internal', e instanceof Error ? e.message : String(e));
			if (err.kind === 'trap' || err.kind === 'cancelled') {
				handleError(err);
				return;
			}
			if (token !== fileToken) return;
			out.className = 'ck-result ck-result--err';
			out.replaceChildren(h('p', {}, h('strong', {}, 'Not done. '), mono(err.kind), ' ' + cleanMessage(err.kind, err.message)));
			out.hidden = false;
		} finally {
			button.removeAttribute('aria-busy');
		}
	}

	function roundTripTable(check: RoundTrip): Node[] {
		const same = check.rows.every((r) => r.source === r.back);
		const table = h(
			'table',
			{ class: 'ck-rt' },
			h('thead', {}, h('tr', {}, h('th', { scope: 'col' }, 'Read back'), h('th', { scope: 'col' }, 'Document'), h('th', { scope: 'col' }, 'File'), h('th', { scope: 'col' }, h('span', { class: 'sr-only' }, 'Result')))),
			h(
				'tbody',
				{},
				...check.rows.map((r) =>
					h(
						'tr',
						{},
						h('th', { scope: 'row' }, r.label),
						h('td', {}, r.source.toLocaleString('en-US')),
						h('td', {}, r.back.toLocaleString('en-US')),
						h('td', { class: r.source === r.back ? 'ck-ok' : 'ck-diff' }, r.source === r.back ? 'same' : 'differs')
					)
				)
			)
		);
		const tail = `Read back by cadkit in ${formatMs(check.ms)}${check.warnings === null ? '' : ', ' + (check.warnings ? plural(check.warnings, 'warning') : 'no warnings')}.`;
		const nodes: Node[] = [table, h('p', { class: 'ck-hint' }, tail)];
		if (!same && doc && Object.keys(doc.notDrawn).length) {
			nodes.push(h('p', { class: 'ck-hint' }, 'Entities read as unknown have no form in the output format, so the writer leaves them out (see the File tab).'));
		}
		return nodes;
	}

	// --- exports ---
	const stem = () => (doc?.name ?? 'drawing').replace(/\.[^.]+$/, '') || 'drawing';

	function exportSvg(): void {
		const text = loaded && view ? sheet2d.exportSvg(view.home) : null;
		if (!text) {
			showAlert('internal', 'there is no 2D sheet to export for this model.');
			return;
		}
		download(stem() + '.svg', text, 'image/svg+xml');
		el.expResult.className = 'ck-result';
		el.expResult.replaceChildren(h('p', {}, h('strong', {}, stem() + '.svg'), ` saved: ${formatBytes(new Blob([text]).size)}, the 2D sheet as shown.`));
		el.expResult.hidden = false;
		announce('SVG export saved.');
	}

	async function exportDoc(button: HTMLButtonElement, format: 'dxf' | 'json' | 'citygml'): Promise<void> {
		await runInPanel(button, el.expResult, async () => {
			const r =
				format === 'dxf'
					? await engine.call<ExportResult>({ type: 'export', format, version: el.dxfVersion.value as DxfVersion, codepage: el.dxfCodepage.disabled ? '' : el.dxfCodepage.value })
					: await engine.call<ExportResult>({ type: 'export', format });
			const name = `${stem()}${format === 'citygml' && /\.gml$/i.test(doc?.name ?? '') ? '-cadkit' : ''}.${r.extension}`;
			download(name, r.data as BlobPart, r.mime);
			announce(`${name} saved (${formatBytes(r.bytes)}).`);
			return [h('p', {}, h('strong', {}, name), ` saved: ${formatBytes(r.bytes)}, ${r.note}.`), ...roundTripTable(r.check)];
		});
	}

	async function transform(button: HTMLButtonElement, body: { type: 'transform'; op: 'units'; unit: string } | { type: 'transform'; op: 'explode' | 'reset' }): Promise<void> {
		await runInPanel(button, el.changeResult, async () => {
			const r = await engine.call<TransformResult>(body, phase);
			await showDocument(r.doc, { keepModel: true, start: null });
			announce(r.note);
			return [h('p', {}, r.note)];
		});
	}

	// --- checks ---
	function readOutcome(r: CheckResult, what: string): Node[] {
		if (r.ok) {
			const codes = Object.entries(r.warningCodes ?? {});
			return [
				h('p', {}, h('strong', {}, 'Read. '), `${what}: ${plural(r.entities ?? 0, 'entity', 'entities')}, ${plural(r.layers ?? 0, 'layer')}, ${r.warnings ? plural(r.warnings, 'warning') : 'no warnings'} in ${formatMs(r.ms)}.`),
				...(codes.length ? [h('p', { class: 'ck-hint' }, ...codes.flatMap(([c, n], i) => [i ? ', ' : '', mono(c), ' × ' + n]))] : []),
			];
		}
		return [
			h('p', {}, h('strong', {}, 'Stopped with a typed error. '), mono(r.kind ?? 'error'), ' ' + cleanMessage(r.kind ?? '', r.message ?? '')),
			h('p', { class: 'ck-hint' }, `${what}: returned in ${formatMs(r.ms)}. Nothing crashed; the open drawing is untouched.`),
		];
	}

	async function runCheck(button: HTMLButtonElement, out: HTMLElement, op: 'inspect' | 'limits' | 'truncate'): Promise<void> {
		await runInPanel(button, out, async () => {
			if (op === 'inspect') {
				const r = await engine.call<CheckResult>({ type: 'check', op });
				const hd = r.header;
				const dl = h('dl', { class: 'ck-dl' });
				fillDl(dl, [
					['Detected', r.detected ? formatName(r.detected) : 'not recognised'],
					['Version', hd ? versionLabel(hd.format, hd.version) : '-'],
					['Code page', hd?.codepage ?? 'not stated'],
					['Application', hd?.application ?? 'not stated'],
					['Units', hd?.units ? unitName(hd.units.unit) : 'not stated'],
					['3D', hd?.is_3d === null || hd?.is_3d === undefined ? 'not stated in the header' : hd.is_3d ? 'yes' : 'no'],
				]);
				return [dl, h('p', { class: 'ck-hint' }, `From ${formatBytes(r.bytes)} in ${formatMs(r.ms)}, without decoding entities.`)];
			}
			if (op === 'limits') {
				const n = Math.max(1, Math.floor(Number(el.chkLimit.value) || 1));
				const r = await engine.call<CheckResult>({ type: 'check', op, maxObjects: n }, phase);
				return readOutcome(r, `With at most ${plural(n, 'object')}`);
			}
			const pct = Number(el.chkCut.value) || 50;
			const r = await engine.call<CheckResult>({ type: 'check', op, fraction: pct / 100 }, phase);
			return readOutcome(r, `The first ${formatBytes(r.bytes)} (${pct} %)`);
		});
	}

	async function runValidate(): Promise<void> {
		await runInPanel(el.gmlValidate, el.gmlValidateOut, async () => {
			const r = await engine.call<GmlValidation>({ type: 'gml', op: 'validate' }, phase);
			const dl = h('dl', { class: 'ck-dl' });
			fillDl(dl, Object.entries(r.counters).map(([k, v]) => [k.charAt(0).toUpperCase() + k.slice(1), v.toLocaleString('en-US')]));
			return [h('p', {}, h('strong', {}, 'Valid. '), `Structure and solid geometry checked in ${formatMs(r.ms)}.`), dl];
		});
	}

	async function runPreflight(): Promise<void> {
		await runInPanel(el.gmlPreflight, el.gmlPreflightOut, async () => {
			const r = await engine.call<GmlPreflight>({ type: 'gml', op: 'preflight', profile: el.gmlProfile.value }, phase);
			const nodes: Node[] = [
				h('p', {}, h('strong', {}, r.issueCount ? plural(r.issueCount, 'issue') + '. ' : 'No issues. '), `${r.profile}: ${plural(r.buildings, 'building')}, ${plural(r.storeys, 'storey')}, ${plural(r.independentSections, 'independent section')}, in ${formatMs(r.ms)}.`),
				h('p', { class: 'ck-hint' }, 'Local structural checks only. This is not an official acceptance: ', mono('acceptance_verified'), ` is ${String(r.acceptanceVerified)}${r.uncheckedRules.length ? `, and ${plural(r.uncheckedRules.length, 'rule')} of the guide ${r.uncheckedRules.length === 1 ? 'is' : 'are'} not checked` : ''}.`),
			];
			if (r.issues.length) {
				nodes.push(
					h(
						'table',
						{ class: 'ck-rt' },
						h('thead', {}, h('tr', {}, h('th', { scope: 'col' }, 'Code'), h('th', { scope: 'col' }, 'Field'), h('th', { scope: 'col' }, 'Count'))),
						h('tbody', {}, ...r.issues.map((i) => h('tr', {}, h('th', { scope: 'row' }, mono(i.code), ' ', h('span', { class: 'ck-sev' }, i.severity)), h('td', {}, i.field), h('td', {}, i.count.toLocaleString('en-US')))))
					)
				);
				if (r.truncated) nodes.push(h('p', { class: 'ck-hint' }, 'The list is cut to the demo limit.'));
			}
			return nodes;
		});
	}

	// --- calls ---
	function fillCalls(): void {
		el.callsCount.textContent = callLog.length ? `(${callLog.length.toLocaleString('en-US')})` : '';
		el.callsClear.disabled = !callLog.length;
		el.calls.replaceChildren(
			...callLog.slice(0, MAX_CALLS_SHOWN).map((c) => {
				const meta = h('span', { class: 'ck-call-meta' }, h('span', { class: 'ck-call-ms' }, formatMs(c.ms)), ' ' + c.note);
				if (c.task) {
					const link = h('button', { type: 'button', class: 'ck-link-btn', 'data-task': c.task }, 'Code');
					meta.append(' ', link);
				}
				return h('li', {}, h('code', { class: 'ck-call' }, c.call), meta);
			})
		);
	}
	engine.onCalls = (calls) => {
		let render = 0;
		let rendered = false;
		for (const c of calls) {
			if (c.call.startsWith('read(bytes, { maxInputBytes') && !c.call.includes('maxObjects') && !c.note.startsWith('throws')) timings.read = c.ms;
			if (c.call.startsWith('doc.toSvgBounded(')) {
				render += c.ms;
				rendered = true;
			}
		}
		if (rendered) timings.render = render;
		callLog.unshift(...[...calls].reverse());
		callLog.length = Math.min(callLog.length, MAX_CALLS_SHOWN);
		fillCalls();
	};
	el.calls.addEventListener('click', (e) => {
		const task = (e.target as Element).closest<HTMLElement>('[data-task]')?.dataset.task;
		if (!task) return;
		selectTask(task);
		document.getElementById('ck-code')?.scrollIntoView({ block: 'start', behavior: 'smooth' });
	});
	el.callsClear.addEventListener('click', () => {
		callLog.length = 0;
		fillCalls();
	});

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
		announce('Touch gestures now move the drawing. Press Done to scroll the page again.');
	});
	el.touchDone.addEventListener('click', () => {
		setTouch(false);
		el.touchStart.focus({ preventScroll: true });
	});

	// --- pointer input ---
	type Tracked = { x: number; y: number; sx: number; sy: number; moved: boolean; pan: boolean };
	const pointers = new Map<number, Tracked>();
	/** Touches that started before the gate was opened: a second one opens it (pinch). */
	const waiting = new Map<number, Tracked>();
	let pinch: { d: number; mx: number; my: number } | null = null;
	const pinchState = () => {
		const [a, b] = [...pointers.values()];
		if (!a || !b) return null;
		return { d: Math.hypot(a.x - b.x, a.y - b.y), mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2 };
	};
	const active = () => (mode === '3d' ? Boolean(scene && mesh) : Boolean(view));

	el.stage.addEventListener('pointerdown', (e) => {
		if (!active() || (e.pointerType === 'mouse' && e.button > 2)) return;
		const t: Tracked = { x: e.clientX, y: e.clientY, sx: e.clientX, sy: e.clientY, moved: false, pan: e.shiftKey || e.button === 1 || e.button === 2 };
		if (e.pointerType === 'touch' && !touchActive) {
			waiting.set(e.pointerId, t);
			if (waiting.size < 2) return;
			// Two fingers on the drawing mean the drawing, not the page.
			setTouch(true);
			for (const [id, w] of waiting) {
				pointers.set(id, w);
				try {
					el.stage.setPointerCapture(id);
				} catch {
					// The pointer may be gone already.
				}
			}
			waiting.clear();
			pinch = pinchState();
			el.root.dataset.drag = 'true';
			return;
		}
		if (e.pointerType === 'mouse' && e.button !== 0 && mode !== '3d') return;
		el.stage.setPointerCapture(e.pointerId);
		pointers.set(e.pointerId, t);
		if (pointers.size === 2) pinch = pinchState();
		el.root.dataset.drag = 'true';
	});
	el.stage.addEventListener('pointermove', (e) => {
		const { left, top, w, h } = stageSize();
		if (e.pointerType === 'mouse') {
			showCoords(e.clientX - left, e.clientY - top);
			if (measuring && view && mPts.length && !mClosed && !pointers.size) {
				mCursor = V.toWorld(view, w, h, e.clientX - left, e.clientY - top);
				sheet2d.setMeasure(mPts, mClosed, mCursor);
			}
		}
		const p = pointers.get(e.pointerId);
		if (!p || !active()) return;
		if (Math.hypot(e.clientX - p.sx, e.clientY - p.sy) > 4) p.moved = true;
		const dx = e.clientX - p.x;
		const dy = e.clientY - p.y;
		if (pointers.size === 1 && p.moved) {
			if (mode === '3d') {
				if (p.pan) scene?.pan(dx, dy);
				else scene?.orbit(dx, dy);
			} else if (view) setView(V.pan(view, dx, dy));
		}
		p.x = e.clientX;
		p.y = e.clientY;
		if (pointers.size === 2 && pinch) {
			p.moved = true;
			const now = pinchState();
			if (now && pinch.d > 0 && now.d > 0) {
				if (mode === '3d') {
					scene?.zoom(now.d / pinch.d);
					scene?.pan(now.mx - pinch.mx, now.my - pinch.my);
				} else {
					zoomBy(now.d / pinch.d, now.mx - left, now.my - top);
					if (view) setView(V.pan(view, now.mx - pinch.mx, now.my - pinch.my));
				}
				pinch = now;
			}
		}
	});
	const endPointer = (e: PointerEvent) => {
		waiting.delete(e.pointerId);
		const p = pointers.get(e.pointerId);
		const click = p && !p.moved && pointers.size === 1 && e.type === 'pointerup' && (e.pointerType !== 'mouse' || e.button === 0);
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
		if (e.pointerType !== 'mouse' || pointers.size) return;
		showCentre();
		if (mCursor) {
			mCursor = null;
			sheet2d.setMeasure(mPts, mClosed, null);
		}
	});
	// The right button pans the 3D view; keep the context menu out of the way there.
	el.stage.addEventListener('contextmenu', (e) => {
		if (mode === '3d' && active()) e.preventDefault();
	});
	el.stage.addEventListener(
		'wheel',
		(e) => {
			if (!active()) return;
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
		if (!view || mode === '3d' || measuring) return;
		const { left, top } = stageSize();
		zoomBy(2, e.clientX - left, e.clientY - top);
	});

	// --- keyboard ---
	el.stage.addEventListener('keydown', (e) => {
		if (!doc || e.ctrlKey || e.metaKey || e.altKey) return;
		const { w, h } = stageSize();
		const step = (e.shiftKey ? 0.4 : 0.1) * Math.min(w, h);
		const move = (dx: number, dy: number) => {
			if (mode === '3d') {
				if (e.shiftKey) scene?.pan(dx * 0.25, dy * 0.25);
				else scene?.orbit(-dx * 0.5, -dy * 0.5);
			} else if (view) setView(V.pan(view, dx, dy));
		};
		const actions: Record<string, () => void> = {
			ArrowLeft: () => move(step, 0),
			ArrowRight: () => move(-step, 0),
			ArrowUp: () => move(0, step),
			ArrowDown: () => move(0, -step),
			'+': () => zoomBy(1.25),
			'=': () => zoomBy(1.25),
			'-': () => zoomBy(0.8),
			_: () => zoomBy(0.8),
			'0': () => fitView(),
			Enter: () => void pickAt(w / 2, h / 2),
			' ': () => void pickAt(w / 2, h / 2),
			Escape: () => (measuring ? stopMeasure() : clearSelection()),
			Backspace: () => {
				if (measuring && mPts.length) el.mUndo.click();
			},
			m: () => el.measureToggle.click(),
			M: () => el.measureToggle.click(),
			'3': () => {
				if (!el.three.hidden) el.three.click();
			},
			f: () => {
				if (!el.full.hidden) el.full.click();
			},
			F: () => {
				if (!el.full.hidden) el.full.click();
			},
		};
		const fn = actions[e.key];
		if (!fn) return;
		e.preventDefault();
		fn();
		if (!['Enter', ' ', 'Escape', 'Backspace'].includes(e.key)) showCentre();
	});
	el.stage.addEventListener('focus', () => {
		if (active()) showCentre();
	});

	new ResizeObserver(() => {
		if (mode === '3d') scene?.invalidate();
		if (!view) return;
		if (fitted) {
			const { w, h } = stageSize();
			setView(V.fitBox(view.home, view.home, w, h, insets()), false);
		} else applyView();
	}).observe(el.stage);

	// --- controls ---
	el.zin.addEventListener('click', () => zoomBy(1.4));
	el.zout.addEventListener('click', () => zoomBy(1 / 1.4));
	el.fit.addEventListener('click', fitView);
	const syncSheet = () => {
		el.root.dataset.sheet = sheet;
		el.root.style.setProperty('--ck-sheet-bg', SHEET_BG[sheet]);
		el.sheet.setAttribute('aria-pressed', String(sheet === 'dark'));
	};
	syncSheet();
	el.sheet.addEventListener('click', () => {
		sheet = sheet === 'dark' ? 'light' : 'dark';
		syncSheet();
		sheet2d.setSheet(sheet);
		scene?.setSheet(sheet);
		buildLayers();
		announce(sheet === 'dark' ? 'Dark sheet.' : 'Light sheet.');
	});
	el.three.addEventListener('click', async () => {
		const token = setBusy('mesh');
		try {
			await setMode(mode === '3d' ? '2d' : '3d');
			fillDocument();
			announce(mode === '3d' ? '3D view. Drag to turn, Shift-drag to pan.' : '2D sheet, seen from above.');
		} catch (e) {
			handleError(e);
		} finally {
			if (token === busyToken) setBusy(null);
		}
	});
	el.measureToggle.addEventListener('click', () => (measuring ? stopMeasure() : startMeasure()));
	el.mClose.addEventListener('click', () => {
		mClosed = !mClosed;
		fillMeasure();
	});
	el.mUndo.addEventListener('click', () => {
		mPts.pop();
		if (mPts.length < 3) mClosed = false;
		fillMeasure();
	});
	el.mClear.addEventListener('click', () => {
		mPts = [];
		mClosed = false;
		fillMeasure();
	});
	el.model.addEventListener('change', async () => {
		model = Number(el.model.value) || 0;
		clearSelection(false);
		stopMeasure();
		mode = '2d';
		const token = setBusy('render');
		try {
			await render(true);
			syncMode();
			if (has3d()) await setMode('3d');
			fillDocument();
		} catch (e) {
			handleError(e);
		} finally {
			if (token === busyToken) setBusy(null);
		}
		const m = doc?.info.models[model];
		if (m) announce(`Showing ${m.name}, ${plural(m.entities, 'entity', 'entities')}${loaded?.layout ? ', a paper layout with ' + plural(loaded.layout.frames.length, 'viewport') : ''}.`);
	});
	el.layersAll.addEventListener('click', () => setAllLayers(true));
	el.layersNone.addEventListener('click', () => setAllLayers(false));
	el.layerFilter.addEventListener('input', buildLayers);
	el.selClear.addEventListener('click', () => {
		clearSelection();
		el.selHeading.focus();
	});
	el.selZoom.addEventListener('click', zoomToSelection);
	el.selchipMore.addEventListener('click', () => revealTab('entity'));
	el.selCopy.addEventListener('click', async () => {
		try {
			await navigator.clipboard.writeText(el.selJson.textContent ?? '');
			announce('Entity JSON copied.');
		} catch {
			announce('Copying is not available here; select the text instead.');
		}
	});
	el.noticeMore.addEventListener('click', () => {
		revealTab('file');
		el.notDrawn.scrollIntoView({ block: 'nearest' });
	});
	el.noticeClose.addEventListener('click', () => {
		el.notice.hidden = true;
		if (fitted) fitView();
	});

	el.expSvg.addEventListener('click', exportSvg);
	el.expDxf.addEventListener('click', () => void exportDoc(el.expDxf, 'dxf'));
	el.expJson.addEventListener('click', () => void exportDoc(el.expJson, 'json'));
	el.expGml.addEventListener('click', () => void exportDoc(el.expGml, 'citygml'));
	el.dxfVersion.addEventListener('change', () => {
		el.dxfCodepage.disabled = !LEGACY_DXF_VERSIONS.includes(el.dxfVersion.value as DxfVersion);
	});
	el.unitApply.addEventListener('click', () => void transform(el.unitApply, { type: 'transform', op: 'units', unit: el.unit.value }));
	el.explode.addEventListener('click', () => void transform(el.explode, { type: 'transform', op: 'explode' }));
	el.reset.addEventListener('click', () => void transform(el.reset, { type: 'transform', op: 'reset' }));
	el.chkInspect.addEventListener('click', () => void runCheck(el.chkInspect, el.chkInspectOut, 'inspect'));
	el.chkLimitRun.addEventListener('click', () => void runCheck(el.chkLimitRun, el.chkLimitOut, 'limits'));
	el.chkCutRun.addEventListener('click', () => void runCheck(el.chkCutRun, el.chkCutOut, 'truncate'));
	el.chkCut.addEventListener('input', () => (el.chkCutValue.textContent = el.chkCut.value + '%'));
	el.gmlValidate.addEventListener('click', () => void runValidate());
	el.gmlPreflight.addEventListener('click', () => void runPreflight());

	// --- panel and full screen ---
	function setPanel(open: boolean): void {
		el.root.dataset.panel = open ? 'open' : 'closed';
		el.panelToggle.setAttribute('aria-pressed', String(open));
	}
	el.panelToggle.addEventListener('click', () => setPanel(el.root.dataset.panel === 'closed'));
	if (!document.fullscreenEnabled) el.full.hidden = true;
	el.full.addEventListener('click', () => {
		if (document.fullscreenElement) void document.exitFullscreen();
		else void el.root.requestFullscreen().catch(() => announce('Full screen is not available here.'));
	});
	document.addEventListener('fullscreenchange', () => {
		const on = document.fullscreenElement === el.root;
		el.full.setAttribute('aria-pressed', String(on));
		el.full.setAttribute('aria-label', on ? 'Leave full screen' : 'Full screen');
	});

	el.cancel.addEventListener('click', () => {
		setBusy(null);
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
	syncMode();
	fillCalls();
	engine.start();
	engine.ready.catch((e: unknown) => handleError(e));
	const wanted = new URL(location.href).searchParams.get('sample');
	// Without ?sample= the page opens the plan sample, so the first thing a visitor sees is a drawing.
	void openSample(wanted ?? DEFAULT_SAMPLE, !wanted);
}
