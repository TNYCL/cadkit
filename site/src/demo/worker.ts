// Web Worker that owns the cadkit-wasm module and the open document. The page never touches
// WebAssembly directly: a trap or a runaway parse can be ended by terminating this worker.
// Files arrive as ArrayBuffers from the page; nothing is sent anywhere else. What the worker
// does with them is in core.ts.

import init, { CadDocument, detect, inspect, read, readCityGml } from 'cadkit-wasm';
import wasmUrl from 'cadkit-wasm/cadkit_wasm_bg.wasm?url';
import pkg from 'cadkit-wasm/package.json';
import { createCore, toWire } from './core.ts';
import type { Request, Response } from './protocol.ts';

interface WorkerScope {
	postMessage(message: Response, transfer?: Transferable[]): void;
	addEventListener(type: 'message', listener: (event: MessageEvent<Request>) => void): void;
}
const scope = self as unknown as WorkerScope;
const core = createCore({ CadDocument, detect, inspect, read, readCityGml });

scope.addEventListener('message', (event) => {
	const { id, ...body } = event.data;
	try {
		const { value, transfer } = core.handle(body, (phase) => scope.postMessage({ type: 'phase', id, phase }));
		scope.postMessage({ type: 'result', id, ok: true, value, calls: core.takeCalls() }, transfer);
	} catch (e) {
		scope.postMessage({ type: 'result', id, ok: false, error: toWire(e), calls: core.takeCalls() });
	}
});

init({ module_or_path: wasmUrl }).then(
	() => scope.postMessage({ type: 'ready', version: pkg.version }),
	(e: unknown) => scope.postMessage({ type: 'boot-error', error: toWire(e) })
);
