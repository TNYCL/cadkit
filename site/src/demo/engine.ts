// The demo's engine: one dedicated Web Worker (src/demo/worker.ts) that owns cadkit-wasm and the
// open document. It is restarted after a WebAssembly trap or when the user cancels.

import type { RequestBody } from './core.ts';
import type { CallRecord, Phase, Request, Response } from './protocol.ts';

export class DemoError extends Error {
	constructor(
		readonly kind: string,
		message: string
	) {
		super(message);
	}
}

type Pending = { resolve: (v: unknown) => void; reject: (e: DemoError) => void; onPhase?: (p: Phase) => void };

export class Engine {
	private worker: Worker | null = null;
	private pending = new Map<number, Pending>();
	private nextId = 1;
	/** Settles once the worker has instantiated the WebAssembly module (pending until start()). */
	ready: Promise<string> = new Promise<string>(() => {});
	onState: (state: string) => void = () => {};
	/** Called with the cadkit-wasm calls of every finished request, successful or not. */
	onCalls: (calls: CallRecord[]) => void = () => {};

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
					if (m.calls.length) this.onCalls(m.calls);
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
