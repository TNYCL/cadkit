// The demo's engine logic, independent of where it runs: it owns the open document and answers
// the page's requests (src/demo/protocol.ts) by calling cadkit-wasm. In the browser a Web Worker
// (worker.ts) hosts it with the web build of the module; the tests (test/demo-core.test.mjs) run
// it in Node with the Node build. Every cadkit-wasm call is timed and reported with the result,
// so the page can show what was called.

import type { CadDocument as Doc } from 'cadkit-wasm';
import { buildMesh, entityAt, surfaceStats, type DocJson, type EntityJson } from './mesh.ts';
import type { Viewport } from './scene2d.ts';
import {
	MAX_ENTITY_JSON_BYTES,
	MAX_FILE_BYTES,
	MAX_HIGHLIGHT_BYTES,
	MAX_ISSUES_LISTED,
	MAX_JSON_ENTITIES,
	MAX_PICK_BYTES,
	MAX_PICK_WORK,
	MAX_SVG_BYTES,
	MAX_WARNINGS_LISTED,
	type CallRecord,
	type CheckResult,
	type EntityResult,
	type ExportResult,
	type GmlPreflight,
	type GmlValidation,
	type HeaderInfo,
	type MeshResult,
	type ModelExtras,
	type OpenResult,
	type Phase,
	type PreflightIssue,
	type RenderResult,
	type Request,
	type RoundTrip,
	type TransformResult,
	type WireError,
} from './protocol.ts';

/** The parts of the cadkit-wasm module the demo uses (the web and the Node build both have them). */
export type CadkitApi = Pick<typeof import('cadkit-wasm'), 'CadDocument' | 'detect' | 'inspect' | 'read' | 'readCityGml'>;

/** A request without its id (the host numbers and routes them). */
export type RequestBody = Request extends infer R ? (R extends Request ? Omit<R, 'id'> : never) : never;

export interface Outcome {
	value: unknown;
	/** Buffers of `value` that can be transferred to the page instead of copied. */
	transfer: Transferable[];
}

export interface Core {
	/** Answers one request; throws an Error with a `kind` (see toWire) when it cannot. */
	handle(req: RequestBody, phase?: (p: Phase) => void): Outcome;
	/** The cadkit-wasm calls made since the last take, oldest first. */
	takeCalls(): CallRecord[];
}

/** An error as it crosses to the page: cadkit's kind, "trap" for a WebAssembly trap, or "internal". */
export function toWire(e: unknown): WireError {
	if (e instanceof WebAssembly.RuntimeError) return { kind: 'trap', message: e.message };
	if (e instanceof Error) {
		const kind = (e as Error & { kind?: unknown }).kind;
		return { kind: typeof kind === 'string' ? kind : 'internal', message: e.message };
	}
	return { kind: 'internal', message: String(e) };
}

function fail(kind: string, message: string): never {
	throw Object.assign(new Error(message), { kind });
}

export function createCore(api: CadkitApi): Core {
	/** The file as opened (kept for the checks, the reset and the native CityGML path). */
	let bytes: Uint8Array | null = null;
	let name = '';
	let header: HeaderInfo | null = null;
	let doc: Doc | null = null;
	/** JSON copy of the document; null when it is too large to keep (MAX_JSON_ENTITIES). */
	let json: DocJson | null = null;
	let changes: OpenResult['changes'] = { units: null, exploded: false };
	let calls: CallRecord[] = [];

	/** Runs one cadkit-wasm call, records its duration and what it returned (or the error kind). */
	function timed<T>(call: string, task: CallRecord['task'], fn: () => T, note: (value: T) => string): T {
		const t0 = performance.now();
		try {
			const value = fn();
			calls.push({ call, ms: performance.now() - t0, note: note(value), task });
			return value;
		} catch (e) {
			calls.push({ call, ms: performance.now() - t0, note: 'throws ' + toWire(e).kind, task });
			throw e;
		}
	}

	const count = (n: number, one: string, many = one + 's') => `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`;
	const kib = (n: number) => (n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KB` : `${(n / (1024 * 1024)).toFixed(1)} MiB`);

	function requireDoc(): Doc {
		if (!doc) fail('bad_argument', 'no document is open');
		return doc;
	}
	function requireBytes(): Uint8Array {
		if (!bytes) fail('bad_argument', 'no document is open');
		return bytes;
	}
	function requireJson(): DocJson {
		if (!json) fail('unsupported', 'this view needs the document as JSON, which the demo keeps only up to ' + MAX_JSON_ENTITIES.toLocaleString('en-US') + ' entities');
		return json;
	}

	const totalEntities = (d: Doc) => d.info().models.reduce((n, m) => n + m.entities, 0);

	/** Facts about the open document for the page; refreshes the JSON copy. */
	function describe(): OpenResult {
		const d = requireDoc();
		const info = d.info();
		const all = d.warnings();
		const warningCodes: Record<string, number> = {};
		for (const w of all) warningCodes[w.code] = (warningCodes[w.code] ?? 0) + 1;
		const total = info.models.reduce((n, m) => n + m.entities, 0);
		json = null;
		let kinds: Record<string, number> | null = null;
		let extras: ModelExtras[] | null = null;
		const notDrawn: Record<string, number> = {};
		if (total <= MAX_JSON_ENTITIES) {
			const copy = timed(
				'doc.toJSON()',
				'inspect',
				() => d.toJSON() as DocJson,
				() => 'the whole document as a JavaScript object'
			);
			json = copy;
			const histogram: Record<string, number> = {};
			const unknowns = (list: EntityJson[], depth: number) => {
				for (const e of list) {
					const k = e?.kind;
					if (!k) continue;
					if (k.type === 'unknown') {
						const source = typeof k.type_name === 'string' && k.type_name ? k.type_name : 'unknown';
						notDrawn[source] = (notDrawn[source] ?? 0) + 1;
					} else if (k.type === 'group' && depth < 32 && Array.isArray(k.children)) unknowns(k.children as EntityJson[], depth + 1);
				}
			};
			extras = (copy.models ?? []).map((m, index) => {
				const list = m.entities ?? [];
				let viewports = 0;
				for (const e of list) {
					const k = e?.kind?.type ?? 'unknown';
					histogram[k] = (histogram[k] ?? 0) + 1;
					if (k === 'viewport') viewports++;
				}
				unknowns(list, 0);
				return { viewports, ...surfaceStats(copy, index) };
			});
			kinds = histogram;
		}
		const first = info.models.findIndex((m) => m.entities > 0);
		return {
			name,
			bytes: bytes?.byteLength ?? 0,
			info,
			header,
			layers: d.layers(),
			warnings: all.slice(0, MAX_WARNINGS_LISTED),
			warningCodes,
			kinds,
			notDrawn,
			extras,
			model: first < 0 ? 0 : first,
			changes: { ...changes },
		};
	}

	function open(fileName: string, buffer: ArrayBuffer): OpenResult {
		if (buffer.byteLength > MAX_FILE_BYTES) fail('limit_exceeded', `file is ${buffer.byteLength} bytes; the demo limit is ${MAX_FILE_BYTES}`);
		const incoming = new Uint8Array(buffer);
		let head: HeaderInfo | null = null;
		try {
			head = timed(
				'inspect(bytes)',
				'inspect',
				() => api.inspect(incoming) as HeaderInfo,
				(h) => `header only: ${h.format} ${h.version}`
			);
		} catch {
			// The full read below reports the real error.
		}
		const next = timed(
			`read(bytes, { maxInputBytes: ${MAX_FILE_BYTES} })`,
			'read',
			() => api.read(incoming, { maxInputBytes: MAX_FILE_BYTES }),
			(d) => `${count(totalEntities(d), 'entity', 'entities')} from ${kib(incoming.byteLength)}`
		);
		doc?.free();
		doc = next;
		bytes = incoming;
		name = fileName;
		header = head;
		changes = { units: null, exploded: false };
		return describe();
	}

	const SVG_OPTIONS = 'maxOutputBytes, includeHiddenLayers: true';
	function svgOf(d: Doc, model: number): string {
		return timed(
			`doc.toSvgBounded({ model: ${model}, ${SVG_OPTIONS} })`,
			'svg',
			() =>
				d.toSvgBounded({
					model,
					maxOutputBytes: MAX_SVG_BYTES,
					widthPx: 1600,
					strokePx: 1,
					background: '#ffffff',
					text: true,
					images: true,
					expandBlocks: true,
					includeHiddenLayers: true,
				}),
			(svg) => `${kib(svg.length)} of SVG`
		);
	}

	const num = (v: unknown): number => (typeof v === 'number' ? v : Number.NaN);
	function viewportsOf(model: number): Viewport[] {
		const out: Viewport[] = [];
		for (const e of json?.models?.[model]?.entities ?? []) {
			const k = e?.kind;
			if (k?.type !== 'viewport') continue;
			const c = k.center as { x?: unknown; y?: unknown } | undefined;
			const v = k.view_center as { x?: unknown; y?: unknown } | undefined;
			out.push({ cx: num(c?.x), cy: num(c?.y), w: num(k.width), h: num(k.height), vcx: num(v?.x), vcy: num(v?.y), vh: num(k.view_height) });
		}
		return out;
	}

	function render(model: number): RenderResult {
		const d = requireDoc();
		const svg = svgOf(d, model);
		const viewports = viewportsOf(model);
		if (!viewports.length || !json) return { model, svg, layout: null };
		// Viewports of a DWG or DXF layout look at model space: the first model that is not a layout.
		const models = json.models ?? [];
		let source = models.findIndex((m) => m.kind === 'model');
		if (source < 0) source = 0;
		if (source === model) return { model, svg, layout: null };
		return { model, svg, layout: { viewports, source, sourceSvg: svgOf(d, source) } };
	}

	const short = (text: string, max: number) => (text.length > max ? text.slice(0, max) + '…' : text);

	function entity(model: number, path: number[], highlight: boolean): EntityResult {
		const j = requireJson();
		const found = entityAt(j, model, path);
		if (!found) fail('bad_argument', 'there is no such entity');
		const e = found.entity;
		const pretty = JSON.stringify(e, null, 2);
		const props: [string, string][] = Object.entries(e.props ?? {}).map(([k, v]) => [k, short(typeof v === 'string' ? v : JSON.stringify(v), 400)]);
		const parents = found.parents.map((p) => {
			const n = p.kind?.name;
			const id = p.props?.['gml.id'];
			return typeof n === 'string' && n ? n : typeof id === 'string' && id ? id : p.kind.type;
		});
		let svg: string | null = null;
		if (highlight) {
			// Draw the entity with the engine itself: a document that holds only this entity (and the
			// block definitions, when it refers to blocks) rendered as SVG in the same coordinates.
			const compact = JSON.stringify(e);
			const mini = { ...j, blocks: /"type":"(insert|dimension)"/.test(compact) ? j.blocks : [], models: [{ ...j.models?.[model], entities: [e] }], warnings: [] };
			let one: Doc | null = null;
			try {
				one = api.CadDocument.fromJsonString(JSON.stringify(mini));
				svg = one.toSvgBounded({ model: 0, maxOutputBytes: MAX_HIGHLIGHT_BYTES, background: '#ffffff', images: false, includeHiddenLayers: true });
			} catch (err) {
				if (err instanceof WebAssembly.RuntimeError) throw err;
				svg = null;
			} finally {
				one?.free();
			}
		}
		return {
			json: pretty.length > MAX_ENTITY_JSON_BYTES ? pretty.slice(0, MAX_ENTITY_JSON_BYTES) : pretty,
			jsonTruncated: pretty.length > MAX_ENTITY_JSON_BYTES,
			kind: e.kind.type,
			layer: e.layer ?? '',
			id: typeof e.id === 'number' ? e.id : null,
			props,
			parents,
			block: e.kind.type === 'insert' && typeof e.kind.block === 'string' ? e.kind.block : null,
			attributes: (Array.isArray(e.attributes) ? (e.attributes as { tag?: unknown; value?: unknown; invisible?: unknown }[]) : []).slice(0, 200).map((a) => ({
				tag: String(a?.tag ?? ''),
				value: short(typeof a?.value === 'string' ? a.value : JSON.stringify(a?.value ?? null), 400),
				invisible: a?.invisible === true,
			})),
			highlight: svg,
		};
	}

	function mesh(model: number): { value: MeshResult; transfer: Transferable[] } {
		const m = buildMesh(requireJson(), model);
		if (!m) fail('unsupported', 'this model has no surfaces to show in 3D');
		return { value: m, transfer: [m.positions.buffer, m.normals.buffer, m.colors.buffer, m.faceOfTri.buffer, m.segments.buffer] };
	}

	function exportDxf(version: string, codepage: string): { value: ExportResult; transfer: Transferable[] } {
		const d = requireDoc();
		const cp = codepage || null;
		const data = timed(
			`doc.toDxfBytes("${version}"${cp ? `, "${cp}"` : ''})`,
			'read',
			() => d.toDxfBytes(version, cp),
			(out) => `${kib(out.byteLength)} of DXF`
		);
		const src = d.info();
		const t0 = performance.now();
		const back = timed(
			'read(dxfBytes)',
			'read',
			() => api.read(data, { maxInputBytes: MAX_FILE_BYTES }),
			(b) => `read back: ${count(totalEntities(b), 'entity', 'entities')}`
		);
		let check: RoundTrip;
		try {
			const info = back.info();
			check = {
				rows: [
					{ label: 'Entities', source: src.models.reduce((n, m) => n + m.entities, 0), back: info.models.reduce((n, m) => n + m.entities, 0) },
					{ label: 'Layers', source: src.layerCount, back: info.layerCount },
					{ label: 'Blocks', source: src.blockCount, back: info.blockCount },
				],
				warnings: info.warningCount,
				ms: performance.now() - t0,
			};
		} finally {
			back.free();
		}
		return {
			value: { data, mime: 'application/dxf', extension: 'dxf', bytes: data.byteLength, check, note: `DXF ${version.toUpperCase()}${cp ? ', ' + cp : ''}` },
			transfer: [data.buffer],
		};
	}

	function exportJson(): ExportResult {
		const d = requireDoc();
		const text = timed(
			'doc.toJsonString(true)',
			'inspect',
			() => d.toJsonString(true),
			(t) => `${kib(t.length)} of JSON`
		);
		const src = d.info();
		const t0 = performance.now();
		const back = timed(
			'api.CadDocument.fromJsonString(json)',
			'inspect',
			() => api.CadDocument.fromJsonString(text),
			(b) => `read back: ${count(totalEntities(b), 'entity', 'entities')}`
		);
		let check: RoundTrip;
		try {
			const info = back.info();
			check = {
				rows: [
					{ label: 'Entities', source: src.models.reduce((n, m) => n + m.entities, 0), back: info.models.reduce((n, m) => n + m.entities, 0) },
					{ label: 'Layers', source: src.layerCount, back: info.layerCount },
					{ label: 'Blocks', source: src.blockCount, back: info.blockCount },
				],
				warnings: info.warningCount,
				ms: performance.now() - t0,
			};
		} finally {
			back.free();
		}
		return { data: text, mime: 'application/json', extension: 'json', bytes: text.length, check, note: 'format-neutral JSON' };
	}

	type Counters = Record<string, number>;
	const counters = (value: unknown): Counters => {
		const out: Counters = {};
		for (const [k, v] of Object.entries((value ?? {}) as Record<string, unknown>)) if (typeof v === 'number') out[k] = v;
		return out;
	};

	function requireCityGml(): Uint8Array {
		const b = requireBytes();
		if (requireDoc().info().format !== 'city_gml') fail('unsupported', 'this needs a CityGML file: the native CityGML model is read from the file itself');
		return b;
	}

	function exportCityGml(): { value: ExportResult; transfer: Transferable[] } {
		const b = requireCityGml();
		const native = timed(
			'readCityGml(bytes)',
			'citygml',
			() => api.readCityGml(b, { maxInputBytes: MAX_FILE_BYTES }),
			() => 'native CityGML model: object graph, ids and references kept'
		);
		let data: Uint8Array;
		let before: Counters;
		try {
			before = counters(native.validate({ geometry: true }));
			data = timed(
				'city.toGml({ geometry: true })',
				'citygml',
				() => native.toGml({ geometry: true }),
				(out) => `${kib(out.byteLength)} of CityGML 2.0`
			);
		} finally {
			native.free();
		}
		const t0 = performance.now();
		const back = timed(
			'readCityGml(gmlBytes)',
			'citygml',
			() => api.readCityGml(data, { maxInputBytes: MAX_FILE_BYTES }),
			() => 'read back'
		);
		let after: Counters;
		try {
			after = counters(back.validate({ geometry: true }));
		} finally {
			back.free();
		}
		const label = (k: string) => k.charAt(0).toUpperCase() + k.slice(1);
		const check: RoundTrip = { rows: Object.keys(before).map((k) => ({ label: label(k), source: before[k] ?? 0, back: after[k] ?? 0 })), warnings: null, ms: performance.now() - t0 };
		return {
			value: { data, mime: 'application/gml+xml', extension: 'gml', bytes: data.byteLength, check, note: 'CityGML 2.0 from the native model' },
			transfer: [data.buffer],
		};
	}

	function transform(req: Extract<RequestBody, { type: 'transform' }>): TransformResult {
		if (req.op === 'reset') {
			const b = requireBytes();
			const next = timed(
				`read(bytes, { maxInputBytes: ${MAX_FILE_BYTES} })`,
				'read',
				() => api.read(b, { maxInputBytes: MAX_FILE_BYTES }),
				(d) => `${count(totalEntities(d), 'entity', 'entities')}, as in the file`
			);
			doc?.free();
			doc = next;
			changes = { units: null, exploded: false };
			return { doc: describe(), note: 'Read the file again: units and blocks are as in the file.' };
		}
		const d = requireDoc();
		if (req.op === 'units') {
			const unit = req.unit;
			const factor = timed(
				`doc.convertUnits("${unit}")`,
				'inspect',
				() => d.convertUnits(unit),
				(f) => `coordinates scaled by ${f}`
			);
			changes = { ...changes, units: unit };
			return { doc: describe(), note: `Converted to ${unit.replace(/_/g, ' ')}: coordinates, block definitions, dash patterns and text heights scaled by ${Number(factor.toPrecision(9))}.` };
		}
		const before = totalEntities(d);
		const result = timed(
			'doc.explodeInserts()',
			'inspect',
			() => d.explodeInserts() as { truncated?: boolean },
			(r) => (r?.truncated ? 'stopped at the work limit' : 'block references expanded')
		);
		changes = { ...changes, exploded: true };
		const described = describe();
		const after = described.info.models.reduce((n, m) => n + m.entities, 0);
		return {
			doc: described,
			note: `Block references expanded into world-space entities: ${count(before, 'entity', 'entities')} became ${after.toLocaleString('en-US')}${result?.truncated ? ' (stopped at the work limit)' : ''}.`,
		};
	}

	/** Reads the given bytes with the given options and reports the outcome instead of throwing. */
	function tryRead(call: string, input: Uint8Array, options: { maxInputBytes: number; maxObjects?: number }): CheckResult {
		const t0 = performance.now();
		let d: Doc | null = null;
		try {
			d = timed(
				call,
				'untrusted',
				() => api.read(input, options),
				(x) => count(totalEntities(x), 'entity', 'entities')
			);
			const info = d.info();
			const warningCodes: Record<string, number> = {};
			for (const w of d.warnings()) warningCodes[w.code] = (warningCodes[w.code] ?? 0) + 1;
			return {
				ok: true,
				entities: info.models.reduce((n, m) => n + m.entities, 0),
				layers: info.layerCount,
				warnings: info.warningCount,
				warningCodes,
				bytes: input.byteLength,
				ms: performance.now() - t0,
			};
		} catch (e) {
			if (e instanceof WebAssembly.RuntimeError) throw e;
			const w = toWire(e);
			return { ok: false, kind: w.kind, message: w.message, bytes: input.byteLength, ms: performance.now() - t0 };
		} finally {
			d?.free();
		}
	}

	function check(req: Extract<RequestBody, { type: 'check' }>): CheckResult {
		const b = requireBytes();
		if (req.op === 'inspect') {
			const t0 = performance.now();
			const detected = timed(
				'detect(bytes)',
				'inspect',
				() => api.detect(b) ?? null,
				(f) => f ?? 'not recognised'
			);
			const head = timed(
				'inspect(bytes)',
				'inspect',
				() => api.inspect(b) as HeaderInfo,
				(h) => `header only: ${h.format} ${h.version}`
			);
			return { ok: true, header: head, detected, bytes: b.byteLength, ms: performance.now() - t0 };
		}
		if (req.op === 'limits') {
			const maxObjects = Math.max(1, Math.floor(req.maxObjects));
			return tryRead(`read(bytes, { maxInputBytes: ${MAX_FILE_BYTES}, maxObjects: ${maxObjects} })`, b, { maxInputBytes: MAX_FILE_BYTES, maxObjects });
		}
		const n = Math.max(0, Math.min(b.byteLength, Math.floor(b.byteLength * req.fraction)));
		return tryRead(`read(bytes.subarray(0, ${n}), { maxInputBytes: ${MAX_FILE_BYTES} })`, b.subarray(0, n), { maxInputBytes: MAX_FILE_BYTES });
	}

	function gml(req: Extract<RequestBody, { type: 'gml' }>): GmlValidation | GmlPreflight {
		const b = requireCityGml();
		const t0 = performance.now();
		const native = timed(
			'readCityGml(bytes)',
			'citygml',
			() => api.readCityGml(b, { maxInputBytes: MAX_FILE_BYTES }),
			() => 'native CityGML model'
		);
		try {
			if (req.op === 'validate') {
				const result = timed(
					'city.validate({ geometry: true })',
					'citygml',
					() => counters(native.validate({ geometry: true })),
					(c) => `valid: ${count(c.polygons ?? 0, 'polygon')}`
				);
				return { counters: result, ms: performance.now() - t0 };
			}
			const profile = req.profile;
			const raw = timed(
				`city.preflightTkgm("${profile}")`,
				'citygml',
				() => native.preflightTkgm(profile) as Record<string, unknown>,
				(r) => `${count(Array.isArray(r.issues) ? r.issues.length : 0, 'issue')}`
			);
			const grouped = new Map<string, PreflightIssue>();
			const list = Array.isArray(raw.issues) ? (raw.issues as Record<string, unknown>[]) : [];
			for (const i of list) {
				const item = { code: String(i.code ?? ''), severity: String(i.severity ?? ''), field: String(i.field ?? '') };
				const key = item.severity + '|' + item.code + '|' + item.field;
				const g = grouped.get(key);
				if (g) g.count++;
				else grouped.set(key, { ...item, count: 1 });
			}
			const issues = [...grouped.values()].sort((x, y) => x.severity.localeCompare(y.severity) || x.code.localeCompare(y.code) || x.field.localeCompare(y.field));
			const unchecked = Array.isArray(raw.unchecked_rules) ? raw.unchecked_rules.map((r) => (typeof r === 'string' ? r : JSON.stringify(r))) : [];
			return {
				profile: String(raw.profile ?? profile),
				workflow: String(raw.workflow ?? ''),
				acceptanceVerified: raw.acceptance_verified === true,
				buildings: Number(raw.buildings ?? 0),
				storeys: Number(raw.storeys ?? 0),
				independentSections: Number(raw.independent_sections ?? 0),
				issues: issues.slice(0, MAX_ISSUES_LISTED),
				issueCount: list.length,
				truncated: raw.truncated === true || issues.length > MAX_ISSUES_LISTED,
				uncheckedRules: unchecked.slice(0, MAX_ISSUES_LISTED),
				ms: performance.now() - t0,
			};
		} finally {
			native.free();
		}
	}

	function handle(req: RequestBody, phase: (p: Phase) => void): { value: unknown; transfer?: Transferable[] } {
		switch (req.type) {
			case 'open':
				phase('parse');
				return { value: open(req.name, req.buffer) };
			case 'render':
				phase('render');
				return { value: render(req.model) };
			case 'pick': {
				const { model, x, y, tolerance, excludedLayers } = req;
				return {
					value: timed(
						`doc.pickEntityBounded(${model}, x, y, tolerance, { maxOutputBytes: ${MAX_PICK_BYTES}, maxWork: ${MAX_PICK_WORK}, excludedLayers }, true)`,
						'inspect',
						() => requireDoc().pickEntityBounded(model, x, y, tolerance, { maxOutputBytes: MAX_PICK_BYTES, maxWork: MAX_PICK_WORK, excludedLayers }, true),
						(hit) => (hit ? `${hit.kind} on layer ${hit.layer || '(unnamed)'}` : 'nothing there')
					),
				};
			}
			case 'entity':
				return { value: entity(req.model, req.path, req.highlight) };
			case 'mesh':
				phase('mesh');
				return mesh(req.model);
			case 'export':
				phase('export');
				if (req.format === 'dxf') return exportDxf(req.version, req.codepage);
				if (req.format === 'citygml') return exportCityGml();
				return { value: exportJson() };
			case 'transform':
				phase('parse');
				return { value: transform(req) };
			case 'check':
				phase('check');
				return { value: check(req) };
			case 'gml':
				phase('check');
				return { value: gml(req) };
		}
	}


	return {
		handle(req, onPhase = () => {}) {
			const { value, transfer } = handle(req, onPhase);
			return { value, transfer: transfer ?? [] };
		},
		takeCalls() {
			const taken = calls;
			calls = [];
			return taken;
		},
	};
}
