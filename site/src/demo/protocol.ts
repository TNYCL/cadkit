// Messages between the demo page and its Web Worker (src/demo/worker.ts).
// Keep this file free of runtime code other than constants: both sides import it.

/** Largest file the demo opens (bytes). Larger files get a clear message instead of a parse. */
export const MAX_FILE_BYTES = 64 * 1024 * 1024;
/** Complete SVG budget for one preview render (bytes, including markup). */
export const MAX_SVG_BYTES = 32 * 1024 * 1024;
/** JSON budget for one picked entity (bytes; cadkit allows up to 64 KiB). */
export const MAX_PICK_BYTES = 16 * 1024;
/** Traversal budget for one pick. */
export const MAX_PICK_WORK = 2_000_000;
/** Entity-kind histogram is computed only up to this many entities (it needs the full JSON). */
export const MAX_KIND_HISTOGRAM_ENTITIES = 100_000;
/** Warnings sent to the page in full; the rest are counted by code. */
export const MAX_WARNINGS_LISTED = 500;

export interface ModelInfo {
	index: number;
	name: string;
	entities: number;
	is3d: boolean;
}

export interface DocumentInfo {
	format: string;
	version: string;
	application: string | null;
	units: string;
	metersPerUnit: number | null;
	models: ModelInfo[];
	layerCount: number;
	blockCount: number;
	warningCount: number;
}

export interface LayerInfo {
	name: string;
	color: { type: string; index?: number; r?: number; g?: number; b?: number };
	visible: boolean;
	frozen: boolean;
	locked: boolean;
}

export interface WarningInfo {
	code: string;
	message: string;
	offset: number | null;
}

export interface PickedEntity {
	index: number;
	id: number | null;
	layer: string;
	kind: string;
	color: string;
	bbox: { min: [number, number, number]; max: [number, number, number] };
	distance: number;
	text?: string;
	block?: string;
	attributes: Array<{ tag: string; value: string }>;
	truncated: boolean;
}

export interface OpenResult {
	name: string;
	bytes: number;
	info: DocumentInfo;
	layers: LayerInfo[];
	warnings: WarningInfo[];
	warningCodes: Record<string, number>;
	/** Entity kinds of all models, or null when the document is too large to count cheaply. */
	kinds: Record<string, number> | null;
	/** First model with entities (index), used for the first render. */
	model: number;
}

export interface RenderResult {
	model: number;
	svg: string;
}

export interface ExportResult {
	data: string;
	mime: string;
	extension: string;
}

export type Request =
	| { id: number; type: 'open'; name: string; buffer: ArrayBuffer }
	| { id: number; type: 'render'; model: number; background: string }
	| {
			id: number;
			type: 'pick';
			model: number;
			x: number;
			y: number;
			tolerance: number;
			excludedLayers: string[];
	  }
	| { id: number; type: 'export'; format: 'dxf' | 'json' };

export interface WireError {
	/** cadkit error kind (unknown_format, unsupported, invalid, truncated, limit_exceeded, io, bad_argument), or "trap" / "internal". */
	kind: string;
	message: string;
}

export type Response =
	| { type: 'ready'; version: string }
	| { type: 'boot-error'; error: WireError }
	| { type: 'phase'; id: number; phase: 'parse' | 'render' | 'export' | 'pick' }
	| { type: 'result'; id: number; ok: true; value: unknown }
	| { type: 'result'; id: number; ok: false; error: WireError };
