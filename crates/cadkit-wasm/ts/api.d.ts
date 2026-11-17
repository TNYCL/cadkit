// Options, results and errors of the cadkit-wasm API. The document model (DocumentJson,
// Entity, EntityKind, Block, ...) is in document.d.ts, generated from the JSON schema.
// Results that were typed `any` before (ExplodeResult, GmlValidationReport, TkgmReport,
// DocumentJson) are type aliases, so that existing casts to Record<string, unknown> compile.

/** Bytes of a file: a Uint8Array (or Node Buffer), any other typed array or DataView (its bytes), or an ArrayBuffer. */
export type BytesLike = Uint8Array | ArrayBuffer | SharedArrayBuffer | ArrayBufferView;

export type FormatName = "dwg" | "dxf" | "dgn_v7" | "dgn_v8" | "city_gml";
export type DxfVersionName = "r12" | "r2000" | "r2004" | "r2007" | "r2010" | "r2013" | "r2018";

/** Every `kind` a thrown error can have. The list is closed. */
export type CadkitErrorKind =
  | "bad_argument"
  | "unknown_format"
  | "unsupported"
  | "invalid"
  | "truncated"
  | "limit_exceeded"
  | "io";

/** The errors thrown by every function and method (a JS `Error` with extra properties). */
export interface CadkitError extends Error {
  kind: CadkitErrorKind;
  /** Absolute byte offset in the input, when the cause has one (`invalid`, `truncated`). */
  offset?: number;
  /** For `limit_exceeded`: the option that bounds what was exceeded, e.g. `"maxObjects"`, `"maxWork"`, `"maxOutputBytes"`. */
  limit?: string;
  /** For `limit_exceeded`: the value of that limit in this call, when known. */
  limitValue?: number;
  /** Bytes (`truncated`) or units (`limit_exceeded`) that were needed, when known. */
  needed?: number;
}

/** Numeric resource limits; every one is capped by the module's ceiling (see docs/wasm.md). */
export interface LimitValues {
  /** Largest input, in bytes (default 512 MiB, ceiling 512 MiB). */
  maxInputBytes?: number;
  /** Largest total of decompressed data, in bytes (default and ceiling 1 GiB). */
  maxDecompressedBytes?: number;
  /** Most records / objects / elements per document (default 1,500,000, ceiling 5,000,000). */
  maxObjects?: number;
  /** Deepest nesting (default and ceiling 64). */
  maxDepth?: number;
  /** Longest single string, in bytes (default 16 MiB, ceiling 64 MiB). */
  maxStringBytes?: number;
  /** Most vertices in one entity (default and ceiling 10,000,000). */
  maxVertices?: number;
  /** Most vertex-like items in the whole document (default and ceiling 50,000,000). */
  maxTotalVertices?: number;
}

/** Limits: a preset, the limit keys, and optionally a nested `limits` object (snake_case keys are accepted too). */
export interface LimitOptions extends LimitValues {
  /** `"strict"`: 64 MiB input, 256 MiB decompressed, 500,000 objects, depth 32, 4 MiB strings, 1,000,000 vertices per entity, 5,000,000 in total, for untrusted input. Limit keys given with it override it. */
  preset?: "default" | "strict";
  limits?: LimitValues;
}

export interface ReadOptions extends LimitOptions {
  /** Keep the original record bytes in `entity.raw`. */
  keepRaw?: boolean;
  /** Code page (WHATWG label such as `"windows-1254"`) for 8-bit text when the file declares none. */
  codepage?: string | null;
  /** Same as `codepage`. */
  fallbackCodepage?: string | null;
}

export interface SvgOptions {
  /** Model index (default 0). */
  model?: number;
  /** Output width in CSS pixels (default 1600). */
  widthPx?: number;
  /** Stroke width in CSS pixels (default 1). */
  strokePx?: number;
  /** CSS color; `null`, `""` or `"none"` for a transparent background. */
  background?: string | null;
  text?: boolean;
  images?: boolean;
  expandBlocks?: boolean;
  includeHiddenLayers?: boolean;
}

/** Complete SVG export with a strict UTF-8 output budget. */
export interface BoundedSvgOptions extends SvgOptions {
  /** Size of the complete SVG in UTF-8 bytes, 1 to 2^32 - 1. Required. */
  maxOutputBytes: number;
  /** Traversal work budget (positive safe integer). */
  maxWork?: number;
}

/** Result of `inspect()` (keys of the Rust `Summary`). */
export interface Summary {
  format: FormatName | "unknown";
  version: string;
  application: string | null;
  codepage: string | null;
  units: { unit: LengthUnit; meters_per_unit: number | null } | null;
  is_3d: boolean | null;
}

export interface ModelInfo {
  index: number;
  name: string;
  /** Number of top-level entities. */
  entities: number;
  is3d: boolean;
}

/** Result of `models()`. */
export interface ModelSummary extends ModelInfo {
  id: number | null;
  kind: ModelKind;
  props: Record<string, Value>;
}

export interface DocumentInfo {
  format: FormatName | "unknown";
  version: string;
  application: string | null;
  units: LengthUnit;
  metersPerUnit: number | null;
  models: ModelInfo[];
  layerCount: number;
  blockCount: number;
  warningCount: number;
}

export interface LayerInfo {
  name: string;
  color: Color;
  visible: boolean;
  frozen: boolean;
  locked: boolean;
}

export interface WarningInfo {
  code: string;
  message: string;
  offset: number | null;
}

/** Where an entity sits: an index of the model's entity list, or indices that step into group children and into the block of an insert. */
export type EntityPath = number | number[];

export interface EntityPageOptions {
  /** Matching entities to skip (default 0). */
  offset?: number;
  /** Most entities in the page (default 1,000, at most 100,000). */
  limit?: number;
  /** Only entities on these layers (own layer name; the default layer is `"0"`). */
  layers?: string[];
  /** Only these kinds (JSON `type` tags; `"mtext"` is accepted for `"m_text"`). */
  kinds?: Array<EntityKindName | "mtext">;
  /** JSON size of the page in bytes (default 16 MiB, at most 256 MiB). */
  maxOutputBytes?: number;
}

export interface EntityPage {
  /** Number of top-level entities that pass the filters. */
  total: number;
  offset: number;
  /** Offset of the next page, or `null` after the last one. */
  next: number | null;
  /** `index`: the entity's index in `toJSON().models[model].entities`. */
  entities: Array<{ index: number; entity: Entity }>;
}

export interface BlockSummary {
  name: string;
  id: number | null;
  /** Number of entities in the block. */
  entities: number;
  basePoint: Point3;
  description: string | null;
  isXref: boolean;
  xrefPath: string | null;
}

export interface DocumentStats {
  /** Top-level entities. */
  entities: number;
  /** Group children at any depth. */
  nested: number;
  /** Entities in block definitions (only when no model was given). */
  blockEntities: number;
  /** Entities per JSON `type` tag, nested ones included. */
  kinds: Partial<Record<EntityKindName, number>>;
  /** Source type names of `unknown` entities, e.g. `"dwg.ACAD_TABLE"`. */
  unknownTypes: Record<string, number>;
  /** Entities per own layer name. */
  layers: Record<string, number>;
}

/** Model-space box in drawing units. */
export interface BoundingBox {
  min: [number, number, number];
  max: [number, number, number];
}

export interface ViewportItem {
  path: number[];
  entity: Entity & { kind: EntityKindViewport };
}

export interface PickOptions {
  /** Layer names to skip (at most 20,000 names of at most 2,048 UTF-16 units, 65,536 bytes in total). */
  excludedLayers?: string[];
  /** Kinds to skip (`EntityKind` type names such as `"line"`, `"mtext"`; `"m_text"` is accepted). */
  excludedKinds?: string[];
}

/** Selection with bounded metadata construction and a complete JSON byte budget. */
export interface BoundedPickOptions extends PickOptions {
  /** Complete JSON size of the result, 1 to 65,536 bytes. Required. */
  maxOutputBytes: number;
  /** Traversal budget, 1 to 5,000,000 (default 5,000,000). */
  maxWork?: number;
}

/** Options of `pick()`. */
export interface PickRequest extends PickOptions {
  /** Largest distance from the point, in world units. Required. */
  tolerance: number;
  /** Complete JSON size of the result, 64 to 65,536 bytes (default 65,536). */
  maxOutputBytes?: number;
  /** Traversal budget, 1 to 5,000,000 (default 5,000,000). */
  maxWork?: number;
  /** Also pick on hidden and frozen layers. */
  includeHiddenLayers?: boolean;
}

/** Result of `pick()`. Text fields are capped as in `pickEntityBounded`; `truncated` says so. */
export interface PickHit {
  /** Path of the entity that was hit; `entity(model, path)` returns it (a block part for geometry drawn from a block). */
  path: number[];
  /** Path of the model entity that owns the hit: the outermost insert for a block part, otherwise `path`. */
  modelPath: number[];
  /** JSON `type` tag of the hit. */
  kind: EntityKindName;
  /** Effective layer of the hit (block parts on layer "0" take the insert's layer). */
  layer: string;
  id: number | null;
  modelKind: EntityKindName;
  /** Own layer of the model entity. */
  modelLayer: string;
  modelId: number | null;
  /** Block of the model entity (or of the hit when it is an unexpanded insert). */
  block: string | null;
  /** Attributes of the model entity (values as text previews). */
  attributes: Array<{ tag: string; value: string }>;
  /** Display color of the hit, `#rrggbb`. */
  color: string;
  bbox: BoundingBox;
  /** Distance from the point, in world units. */
  distance: number;
  text?: string;
  truncated: boolean;
}

/** @deprecated Result of the deprecated `pickEntityBounded`; use `pick()`. */
export interface BoundedPickedEntity {
  /** Position of the hit in the walk's visiting order (block contents and group children included), not an index of `toJSON().models[m].entities`. */
  index: number;
  id: number | null;
  layer: string;
  /** `EntityKind` type name (`"mtext"` for multi-line text). */
  kind: string;
  color: string;
  bbox: BoundingBox;
  distance: number;
  text?: string;
  block?: string;
  attributes: Array<{ tag: string; value: string }>;
  truncated: boolean;
}

/** @deprecated Result of the deprecated `pickEntity`; use `pick()`. */
export interface PickedEntity {
  /** Position of the hit in the walk's visiting order (block contents and group children included), not an index of `toJSON().models[m].entities`. */
  index: number;
  id: number | null;
  layer: string;
  /** `EntityKind` type name (`"mtext"` for multi-line text). */
  kind: string;
  color: string;
  bbox: BoundingBox;
  distance: number;
  text?: string;
  block?: string;
  attributes: Array<{ tag: string; value: Value }>;
  props: Record<string, Value>;
}

export interface ExplodeOptions {
  /** Nesting expanded (default 32, at most 64). */
  maxDepth?: number;
  /** Entities the expansion may add (default 1,000,000, at most 3,000,000); heavy entities count as several. */
  maxEntities?: number;
}

export type ExplodeResult = {
  /** The budget ran out; the remaining inserts are still inserts. */
  truncated: boolean;
  /** Work units spent. */
  work: number;
  /** Top-level entities of all models afterwards. */
  entities: number;
};

/** A row-major affine matrix (12 numbers or 3 rows of 4; 16 numbers or 4 rows of 4 ending in 0, 0, 0, 1), or parts applied as scale, then rotation, then translation. */
export type TransformSpec =
  | number[]
  | number[][]
  | { matrix: number[] | number[][] }
  | {
      scale?: number | [number, number] | [number, number, number];
      /** Counter-clockwise rotation about Z, in radians. */
      rotateZ?: number;
      translate?: [number, number] | [number, number, number];
      /** Fixed point of scale and rotation (default the origin). */
      origin?: [number, number] | [number, number, number];
    };

export interface RetainResult {
  /** Layer definitions removed. */
  layers: number;
  /** Model entities removed, group children included. */
  entities: number;
}

export interface PurgeResult {
  blocks: number;
  layers: number;
  linetypes: number;
  textStyles: number;
}

export interface DxfTextResult {
  text: string;
  /** What could not be written. */
  warnings: WarningInfo[];
}

export interface DxfBytesResult {
  bytes: Uint8Array;
  /** What could not be written. */
  warnings: WarningInfo[];
}

/** `dgn::WriteOptions`; camelCase or snake_case keys. */
export interface DgnWriteOptions extends LimitOptions {
  clearSeedModel?: boolean;
  clear_seed_model?: boolean;
  codepage?: string | null;
  timestampMs?: number;
  planarityTolerance?: number;
  planarity_tolerance?: number;
  preserveSeedRasters?: boolean;
  preserveSeedControls?: boolean;
}

/** `dgn::StripSeedOptions`; snake_case or camelCase keys. An unknown key is an error. */
export interface DgnStripSeedOptions {
  keep_controls?: boolean;
  keepControls?: boolean;
  keep_raster_template?: boolean;
  keepRasterTemplate?: boolean;
  keep_cell_definitions?: boolean;
  keepCellDefinitions?: boolean;
  model_name?: string | null;
  modelName?: string | null;
  timestamp_ms?: number;
  timestampMs?: number;
  application?: string;
}

/** `dgn::StripSeedReport`: what was removed and what remains. */
export interface DgnStripSeedReport {
  removed_graphic_records: number;
  removed_control_records: number;
  removed_control_types: Record<string, number>;
  removed_auxiliary_records: number;
  removed_cell_definitions: number;
  removed_property_sets: number;
  removed_summary_properties: number[];
  removed_document_summary_properties: number[];
  kept_control_records: number;
  kept_control_types: Record<string, number>;
  kept_raster_attachments: number;
  raster_file_references: number;
  dictionary_records: number;
  dictionary_pages_rewritten: number;
  levels: number;
  named_levels: number;
  fonts: number;
  tag_sets: number;
  cell_definitions: number;
  model_name_from_seed: boolean;
  model_description: boolean;
  unknown_streams: number;
  timestamp_ms: number;
  warnings: WarningInfo[];
}

/** A stripped seed and its report. */
export interface StrippedDgnSeed {
  seed: Uint8Array;
  report: DgnStripSeedReport;
}

/** `gml::ValidationOptions`; camelCase or snake_case keys. */
export interface GmlValidationOptions extends LimitOptions {
  geometry?: boolean;
  tolerance?: number;
  planarityTolerance?: number;
  planarity_tolerance?: number;
  maxGeometryWork?: number;
}

/** Counters of `CityGmlDocument.validate()`. */
export type GmlValidationReport = {
  elements: number;
  ids: number;
  references: number;
  polygons: number;
  interiors: number;
  positions: number;
  attributes: number;
};

export type TkgmProfile = "city-model-tender" | "digital-building-registration";

export interface TkgmIssue {
  code: string;
  severity: "error" | "review";
  element: number;
  field: string | null;
  id: string | null;
  building_id: string | null;
}

/** Result of `preflightTkgm()`; `acceptance_verified` is always false. */
export type TkgmReport = {
  profile: string;
  workflow: string;
  acceptance_verified: boolean;
  buildings: number;
  storeys: number;
  independent_sections: number;
  issues: TkgmIssue[];
  truncated: boolean;
  unchecked_rules: string[];
};

/** One entity kept as metadata only. */
export interface CityGmlExportIssue {
  /** Currently `"gml.metadata_only"`. */
  code: string;
  model: number;
  /** Top-level entity index followed by group child indices. */
  path: number[];
  entity_type: string;
}

export interface CityGmlExportReport {
  /** Source entities visited, group nodes included. */
  entities: number;
  geometry_entities: number;
  metadata_only_entities: number;
  issues: CityGmlExportIssue[];
}

/** Result of `toCityGmlReport()`; `toCityGmlWithReport()` returns the same as JSON text. */
export interface CityGmlReportResult {
  xml: string;
  report: CityGmlExportReport;
}
