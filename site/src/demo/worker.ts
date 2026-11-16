// Web Worker that owns the cadkit-wasm module and the open document. The page never touches
// WebAssembly directly: a trap or a runaway parse can be ended by terminating this worker.
// Files arrive as ArrayBuffers from the page; nothing is sent anywhere else.

import init, { read } from 'cadkit-wasm';
import wasmUrl from 'cadkit-wasm/cadkit_wasm_bg.wasm?url';
import pkg from 'cadkit-wasm/package.json';
import type { CadDocument } from 'cadkit-wasm';
import {
	MAX_FILE_BYTES,
	MAX_KIND_HISTOGRAM_ENTITIES,
	MAX_PICK_BYTES,
	MAX_PICK_WORK,
	MAX_SVG_BYTES,
	MAX_WARNINGS_LISTED,
	type OpenResult,
	type Request,
	type Response,
	type WireError,
} from './protocol.ts';

interface WorkerScope {
	postMessage(message: Response): void;
	addEventListener(type: 'message', listener: (event: MessageEvent<Request>) => void): void;
}
const scope = self as unknown as WorkerScope;
const post = (message: Response) => scope.postMessage(message);

let doc: CadDocument | null = null;

function toWire(e: unknown): WireError {
	if (e instanceof WebAssembly.RuntimeError) return { kind: 'trap', message: e.message };
	if (e instanceof Error) {
		const kind = (e as Error & { kind?: unknown }).kind;
		return { kind: typeof kind === 'string' ? kind : 'internal', message: e.message };
	}
	return { kind: 'internal', message: String(e) };
}

function open(name: string, buffer: ArrayBuffer): OpenResult {
	if (buffer.byteLength > MAX_FILE_BYTES) {
		throw Object.assign(new Error(`file is ${buffer.byteLength} bytes; the demo limit is ${MAX_FILE_BYTES}`), {
			kind: 'limit_exceeded',
		});
	}
	const next = read(new Uint8Array(buffer), { maxInputBytes: MAX_FILE_BYTES });
	doc?.free();
	doc = next;
	const info = doc.info();
	const all = doc.warnings();
	const warningCodes: Record<string, number> = {};
	for (const w of all) warningCodes[w.code] = (warningCodes[w.code] ?? 0) + 1;
	const total = info.models.reduce((n, m) => n + m.entities, 0);
	let kinds: Record<string, number> | null = null;
	if (total <= MAX_KIND_HISTOGRAM_ENTITIES) {
		kinds = {};
		const json = doc.toJSON() as { models?: { entities?: { kind?: { type?: string } }[] }[] };
		for (const m of json.models ?? [])
			for (const e of m.entities ?? []) {
				const k = e.kind?.type ?? 'unknown';
				kinds[k] = (kinds[k] ?? 0) + 1;
			}
	}
	const first = info.models.findIndex((m) => m.entities > 0);
	return {
		name,
		bytes: buffer.byteLength,
		info,
		layers: doc.layers(),
		warnings: all.slice(0, MAX_WARNINGS_LISTED),
		warningCodes,
		kinds,
		model: first < 0 ? 0 : first,
	};
}

function requireDoc(): CadDocument {
	if (!doc) throw Object.assign(new Error('no document is open'), { kind: 'bad_argument' });
	return doc;
}

function handle(req: Request): unknown {
	switch (req.type) {
		case 'open':
			post({ type: 'phase', id: req.id, phase: 'parse' });
			return open(req.name, req.buffer);
		case 'render': {
			post({ type: 'phase', id: req.id, phase: 'render' });
			const svg = requireDoc().toSvgBounded({
				model: req.model,
				maxOutputBytes: MAX_SVG_BYTES,
				widthPx: 1600,
				strokePx: 1,
				background: req.background,
				text: true,
				images: true,
				expandBlocks: true,
			});
			return { model: req.model, svg };
		}
		case 'pick':
			return requireDoc().pickEntityBounded(
				req.model,
				req.x,
				req.y,
				req.tolerance,
				{ maxOutputBytes: MAX_PICK_BYTES, maxWork: MAX_PICK_WORK, excludedLayers: req.excludedLayers },
				false
			);
		case 'export': {
			post({ type: 'phase', id: req.id, phase: 'export' });
			const d = requireDoc();
			if (req.format === 'dxf') return { data: d.toDxf('r2018'), mime: 'application/dxf', extension: 'dxf' };
			return { data: d.toJsonString(true), mime: 'application/json', extension: 'json' };
		}
	}
}

scope.addEventListener('message', (event) => {
	const req = event.data;
	try {
		post({ type: 'result', id: req.id, ok: true, value: handle(req) });
	} catch (e) {
		post({ type: 'result', id: req.id, ok: false, error: toWire(e) });
	}
});

init({ module_or_path: wasmUrl }).then(
	() => post({ type: 'ready', version: pkg.version }),
	(e: unknown) => post({ type: 'boot-error', error: toWire(e) })
);
