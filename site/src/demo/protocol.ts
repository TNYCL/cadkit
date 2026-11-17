// Messages between the demo page and its Web Worker (src/demo/worker.ts).
// Keep this file free of runtime code other than constants: both sides import it.

import type { FaceRef, LayerRange } from './mesh.ts';
import type { Origin } from './origin.ts';
import type { Viewport } from './scene2d.ts';
import type { Box } from './view.ts';
import type { PickTarget } from './walk.ts';

/** Largest file the demo opens (bytes). Larger files get a clear message instead of a parse. */
export const MAX_FILE_BYTES = 64 * 1024 * 1024;
/** Complete SVG budget for one preview render (bytes, including markup). */
export const MAX_SVG_BYTES = 32 * 1024 * 1024;
/** SVG budget for the highlight of one selected entity. */
export const MAX_HIGHLIGHT_BYTES = 4 * 1024 * 1024;
/** JSON budget for one picked entity (bytes; cadkit allows up to 64 KiB). */
export const MAX_PICK_BYTES = 16 * 1024;
/** Traversal budget for one pick. */
export const MAX_PICK_WORK = 2_000_000;
/**
 * The worker keeps the document as a JSON object (for the kind histogram, the inspector, the
 * selection highlight, layouts and the 3D view) only up to this many entities.
 */
export const MAX_JSON_ENTITIES = 100_000;
/** Pretty-printed JSON of one entity sent to the inspector (bytes); longer text is cut. */
export const MAX_ENTITY_JSON_BYTES = 256 * 1024;
/** Warnings sent to the page in full; the rest are counted by code. */
export const MAX_WARNINGS_LISTED = 500;
/** Preflight issues listed per group. */
export const MAX_ISSUES_LISTED = 200;

/** DXF versions cadkit writes, oldest first. */
export const DXF_VERSIONS = ['r12', 'r2000', 'r2004', 'r2007', 'r2010', 'r2013', 'r2018'] as const;
export type DxfVersion = (typeof DXF_VERSIONS)[number];
/** Versions that take a legacy 8-bit code page; newer ones are always UTF-8. */
export const LEGACY_DXF_VERSIONS: readonly DxfVersion[] = ['r12', 'r2000', 'r2004'];
/** Code pages offered for legacy DXF output ("" = cadkit's default). */
export const DXF_CODEPAGES = ['', 'windows-1252', 'windows-1250', 'windows-1251', 'windows-1253', 'windows-1254', 'DOS857'] as const;
/** Units offered for conversion (cadkit_core::Unit names). */
export const TARGET_UNITS = ['millimeter', 'centimeter', 'meter', 'kilometer', 'inch', 'foot', 'yard'] as const;
/** TKGM preflight profiles of cadkit-gml. */
export const TKGM_PROFILES = ['city-model-tender', 'digital-building-registration'] as const;

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

/** Header facts from `inspect` (no entity streams decoded). */
export interface HeaderInfo {
	format: string;
	version: string;
	application: string | null;
	codepage: string | null;
	units: { unit: string; meters_per_unit: number | null } | null;
	is_3d: boolean | null;
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

/** A pick: the engine's hit and where it is in the document. */
export interface PickResult {
	/** The entity that was hit, as cadkit-wasm reports it (for a block part: the part). */
	hit: PickedEntity;
	/**
	 * The hit's `index` mapped to index chains into the document (src/demo/walk.ts); null when the
	 * worker keeps no JSON copy or the walk could not be repeated within its budget.
	 */
	target: PickTarget | null;
}

/** One cadkit-wasm call made by the worker, as shown in the Calls tab. */
export interface CallRecord {
	/** The call as JavaScript, for example `doc.toDxfBytes("r2000", "windows-1254")`. */
	call: string;
	ms: number;
	/** What came back, in a few words. */
	note: string;
	/** Which code example on the page shows the same task in other languages. */
	task: 'read' | 'svg' | 'inspect' | 'untrusted' | 'citygml' | null;
}

/** What the 2D or 3D view can do for one model (only known when the JSON copy is kept). */
export interface ModelExtras {
	/** Viewport entities (paper-space layouts). */
	viewports: number;
	/** Surfaces: polygons, faces, mesh faces and, in a 3D model, closed planar polylines. */
	surfaces: number;
	/** How many of `surfaces` are closed planar polylines (DGN shapes). */
	shapes: number;
	/** The surfaces leave the XY plane: a 3D view is offered. */
	spatial: boolean;
	/** The model opens in the 3D view (see src/demo/mesh.ts, surfaceInfo). */
	open3d: boolean;
}

export interface OpenResult {
	name: string;
	bytes: number;
	info: DocumentInfo;
	header: HeaderInfo | null;
	layers: LayerInfo[];
	warnings: WarningInfo[];
	warningCodes: Record<string, number>;
	/** Entity kinds of all models, or null when the document is too large to count cheaply. */
	kinds: Record<string, number> | null;
	/** Entities read as "unknown" by their source type name (not drawn), e.g. {"dwg.MULTILEADER": 15}. */
	notDrawn: Record<string, number>;
	/** Per model; null when the JSON copy is not kept. */
	extras: ModelExtras[] | null;
	/** First model with entities (index), used for the first render. */
	model: number;
	/** Changes applied to the document since it was read. */
	changes: { units: string | null; exploded: boolean };
}

export interface LayoutResult {
	/** The viewports of the sheet, in drawing units. */
	viewports: Viewport[];
	/** Model the viewports look at (model space). */
	source: number;
	/** SVG of that model, relative to `sourceOrigin`. */
	sourceSvg: string;
	/** Local origin of `sourceSvg` (SVG units: absolute = local + origin). */
	sourceOrigin: Origin;
	/** Extents of the raster image frames in `sourceSvg`, or null. */
	sourceFrames: Box | null;
}

export interface RenderResult {
	model: number;
	/** SVG of the model's own entities, relative to `origin` (src/demo/origin.ts). */
	svg: string;
	/** Local origin of `svg` in SVG units (drawing X, drawing Y negated): absolute = local + origin. */
	origin: Origin;
	/** Extents of the raster image frames in `svg` (which its viewBox leaves out), or null. */
	frames: Box | null;
	/** Why the coordinates were left absolute although they needed a local origin; null otherwise. */
	relocation: string | null;
	/** Set for a paper-space layout with viewports. */
	layout: LayoutResult | null;
}

export interface EntityResult {
	/** Pretty-printed JSON of the entity as cadkit reads it (cut to MAX_ENTITY_JSON_BYTES). */
	json: string;
	jsonTruncated: boolean;
	kind: string;
	layer: string;
	id: number | null;
	/** Format-specific properties ("dgn.", "dwg.", "dxf.", "gml." keys), values as text. */
	props: [string, string][];
	/** Names of the groups (and, for a block part, the block references) the entity is nested in, outermost first. */
	parents: string[];
	/** Block a block reference inserts; null for other kinds. */
	block: string | null;
	/** Attributes attached to the entity (block attributes, DGN tags), values as text. */
	attributes: { tag: string; value: string; invisible: boolean }[];
	/** SVG of this entity alone, for the highlight, relative to the requested origin; null when it cannot be drawn. */
	highlight: string | null;
}

export interface MeshResult {
	positions: Float32Array;
	normals: Float32Array;
	colors: Uint8Array;
	faceOfTri: Uint32Array;
	segments: Float32Array;
	layers: LayerRange[];
	faces: FaceRef[];
	center: [number, number, number];
	min: [number, number, number];
	max: [number, number, number];
	duplicates: number;
	truncated: boolean;
}

/** What reading an exported file back gave, next to the document it was written from. */
export interface RoundTrip {
	rows: { label: string; source: number; back: number }[];
	/** Warnings of the second read; null when the reader reports none (native CityGML). */
	warnings: number | null;
	ms: number;
}

export interface ExportResult {
	data: Uint8Array | string;
	mime: string;
	extension: string;
	bytes: number;
	/** The export read back by cadkit in the worker. */
	check: RoundTrip;
	/** One sentence about the export, for the status line. */
	note: string;
}

export interface TransformResult {
	doc: OpenResult;
	note: string;
}

/** Outcome of a robustness check: a second read of the same bytes under other conditions. */
export interface CheckResult {
	ok: boolean;
	/** Error kind and message when the read failed. */
	kind?: string;
	message?: string;
	entities?: number;
	layers?: number;
	warnings?: number;
	warningCodes?: Record<string, number>;
	header?: HeaderInfo;
	detected?: string | null;
	/** Bytes given to the reader. */
	bytes: number;
	ms: number;
}

export interface GmlValidation {
	counters: Record<string, number>;
	ms: number;
}

export interface PreflightIssue {
	code: string;
	severity: string;
	field: string;
	count: number;
}

export interface GmlPreflight {
	profile: string;
	workflow: string;
	acceptanceVerified: boolean;
	buildings: number;
	storeys: number;
	independentSections: number;
	issues: PreflightIssue[];
	issueCount: number;
	truncated: boolean;
	uncheckedRules: string[];
	ms: number;
}

export type Request =
	| { id: number; type: 'open'; name: string; buffer: ArrayBuffer }
	| { id: number; type: 'render'; model: number }
	| { id: number; type: 'pick'; model: number; x: number; y: number; tolerance: number; excludedLayers: string[]; excludedKinds: string[] }
	| { id: number; type: 'entity'; model: number; path: number[]; highlight: boolean; origin: Origin }
	| { id: number; type: 'mesh'; model: number }
	| { id: number; type: 'export'; format: 'dxf'; version: DxfVersion; codepage: string }
	| { id: number; type: 'export'; format: 'json' | 'citygml' }
	| { id: number; type: 'transform'; op: 'units'; unit: string }
	| { id: number; type: 'transform'; op: 'explode' | 'reset' }
	| { id: number; type: 'check'; op: 'inspect' }
	| { id: number; type: 'check'; op: 'limits'; maxObjects: number }
	| { id: number; type: 'check'; op: 'truncate'; fraction: number }
	| { id: number; type: 'gml'; op: 'validate' }
	| { id: number; type: 'gml'; op: 'preflight'; profile: string };

export interface WireError {
	/** cadkit error kind (unknown_format, unsupported, invalid, truncated, limit_exceeded, io, bad_argument), or "trap" / "internal". */
	kind: string;
	message: string;
}

export type Phase = 'parse' | 'render' | 'export' | 'pick' | 'mesh' | 'check';

export type Response =
	| { type: 'ready'; version: string }
	| { type: 'boot-error'; error: WireError }
	| { type: 'phase'; id: number; phase: Phase }
	| { type: 'result'; id: number; ok: true; value: unknown; calls: CallRecord[] }
	| { type: 'result'; id: number; ok: false; error: WireError; calls: CallRecord[] };
