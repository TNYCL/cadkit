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
  /** Write coordinates relative to this point, for drawings in map coordinates (browsers hold SVG coordinates in 32-bit floats). The root then carries `data-origin="ox oy"` and an SVG point (sx, sy) is the drawing point (ox + sx, oy - sy). Default none. */
  origin?: SvgOriginOption | null;
  /** Tag drawn elements with `data-e="i.j.k"` (the path of the model entity, for `entity(model, path)`), contrast-replaced colours with `data-c="#rrggbb"` and image frames with `data-image-path` and `data-image-px`. Default false. */
  entityRefs?: boolean;
  /** Draw points as round dots of this diameter in CSS pixels (greater than 0, at most 1000) instead of crosses. Default null. */
  pointPx?: number | null;
  /** Cap height / font size of the display font (0.05 to 20, about 0.7 for sans-serif): `font-size` becomes the CAD height divided by it. Default null (font-size is the CAD height). */
  capHeightRatio?: number | null;
  /** CSS `font-family` on the root element (at most 1,024 UTF-16 units); `null` or `""` writes none. */
  fontFamily?: string | null;
  /** Space hatch pattern lines by the pattern's own spacing (3.175 drawing units times the scale), at most 500 lines across a hatch, instead of 0.8 % to 8 % of the drawing. Default false. */
  hatchNativeSpacing?: boolean;
  /** For a paper layout, draw each viewport as a clipped, scaled window onto the first design model. Default false. */
  layoutViewports?: boolean;
}

/** `"auto"`: a round number near the centre of the drawing; `[x, y]` or `{x, y}`: that point in drawing units; `"none"`: no shift. */
export type SvgOriginOption = "auto" | "none" | [number, number] | { x: number; y: number };

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

/** How a 2D/3D difference between the source model and the seed is handled. */
export type DgnDimensionPolicy = "strict" | "promote_2d_to_3d" | "project_to_seed";

/** What `prepare` (and `prepareForDgn`) does to the copy of the document. */
export interface DgnPrepareSteps {
  /** Default `"promote_2d_to_3d"` (or the writer's `dimensionPolicy` when given). */
  dimensionPolicy?: DgnDimensionPolicy;
  /** Explode block references (default true). */
  explodeInserts?: boolean;
  /** Convert to the seed's master unit when both scales are known (default true). */
  convertUnits?: boolean;
  /** Normalise colours, lineweights, linetypes, fonts, widths, text alignment and other DGN-specific constraints (default true). */
  normalizeForDgn?: boolean;
}

/** `dgn::WriteOptions`; camelCase or snake_case keys. */
export interface DgnWriteOptions extends LimitOptions {
  clearSeedModel?: boolean;
  clear_seed_model?: boolean;
  codepage?: string | null;
  timestampMs?: number;
  /** Largest distance of polygon ring points from their plane, drawing units (default 0.01). */
  planarityTolerance?: number;
  planarity_tolerance?: number;
  preserveSeedRasters?: boolean;
  preserveSeedControls?: boolean;
  /** Model to write (default: the only model, or the only design model beside paper layouts). */
  modelIndex?: number;
  /** Default `"strict"`: a model of the other dimension than the seed is an error. */
  dimensionPolicy?: DgnDimensionPolicy;
  /** Write centred and right-aligned text without a stored length with an estimated one instead of failing (default false). */
  estimateTextLength?: boolean;
  /** Prepare a copy of the document for the seed first (`true` for the default steps). Needed for most documents that did not come from the seed's own file. */
  prepare?: boolean | DgnPrepareSteps;
}

/** Options of `prepareForDgn()`. Unknown keys are errors. */
export interface DgnPrepareOptions extends LimitOptions, DgnPrepareSteps {
  /** Model to prepare (default: the first design model). */
  modelIndex?: number;
  /** Code page of the seed's 8-bit text. */
  codepage?: string | null;
}

/** A DGN writer warning: `dgn.model_not_written` (`object`: the model index), `dgn.model_dimension_changed`, `dgn.text_length_estimated`. */
export interface DgnWriteWarning extends WarningInfo {
  object: number | null;
}

/** Result of `toDgnWithReport()`. */
export interface DgnWriteResult {
  bytes: Uint8Array;
  /** Index of the source model that was written. */
  modelIndex: number;
  warnings: DgnWriteWarning[];
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
  /** `gml:id` of the element; only from `preflightTkgmLocated()`, when it has one. */
  id?: string;
  /** `gml:id` of the enclosing building; only from `preflightTkgmLocated()`. */
  building_id?: string;
}

/** Result of `preflightTkgm()` and `preflightTkgmLocated()`: local checks only; `acceptance_verified` is always false. */
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

/** One entity kept as metadata only, or a closed polyline written as a line string. */
export interface CityGmlExportIssue {
  /** `"gml.metadata_only"`, or with `closedPolylinesAsSurfaces` why a closed polyline stayed a line string (`"gml.face_not_planar"`, `"gml.face_degenerate"`, `"gml.face_self_intersecting"`, `"gml.face_invalid"`). */
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

/** Surface options of the CityGML exports; camelCase or snake_case keys. */
export interface CityGmlExportOptions extends LimitOptions {
  /** Write closed, straight polylines with at least three distinct vertices as surfaces when they are planar and simple (default false). */
  closedPolylinesAsSurfaces?: boolean;
  /** Largest distance of a ring point from the face plane, as a fraction of the face's bounding-box diagonal (default 1e-3). */
  planarityTolerance?: number;
  /** Ceiling of that distance in coordinate units (default 0.01, the default validation tolerance). */
  maxPlanarityDeviation?: number;
}

export type CityGmlBuildingGrouping = "single" | "layer" | "top_level" | "connected";

/** Options of `toCityGmlBuildings()`. Unknown keys are errors. */
export interface CityGmlBuildingOptions extends CityGmlExportOptions {
  /** CRS identifier of the existing coordinates (no reprojection). Required. */
  srsName: string;
  /** Level of detail; CityGML 2.0 has wall, roof and ground surfaces from LoD 2. Required. */
  lod: 2 | 3 | 4;
  /** Source model (default 0). */
  model?: number;
  /** One building for all faces (default), per effective layer, per top-level entity, or per connected component. */
  grouping?: CityGmlBuildingGrouping;
  /** Smallest tilt of a wall, degrees from horizontal (default 80). */
  wallMinTiltDegrees?: number;
  /** Largest tilt of a horizontal face, degrees (default 10); below `wallMinTiltDegrees`. */
  horizontalMaxTiltDegrees?: number;
  /** Height band above a building's lowest point that counts as its base, coordinate units (default 0.01). */
  groundTolerance?: number;
  /** Distance within which vertices are shared for `"connected"`, coordinate units (default 0.001). */
  weldTolerance?: number;
  /** Work budget of the walk, the face checks and the welding (default 100,000,000). */
  maxWork?: number;
}

export interface CityGmlBuildingSummary {
  /** `gml:id` of the building. */
  id: string;
  /** The layer, for `grouping: "layer"`. */
  layer: string | null;
  /** The top-level entity index, for `grouping: "top_level"`. */
  topLevel: number | null;
  wallSurfaces: number;
  roofSurfaces: number;
  groundSurfaces: number;
  unclassifiedFaces: number;
}

/** A face that was skipped or kept without a class. */
export interface CityGmlBuildingIssue {
  /** Skipped: `gml.face_degenerate`, `gml.face_not_planar`, `gml.face_self_intersecting`, `gml.face_invalid`, `gml.face_curved`, `gml.face_wide`, `gml.face_index_out_of_range`; kept unclassified: `gml.face_sloped_at_base`. */
  code: string;
  /** Path of the source entity: `entity(model, path)` returns it. */
  path: number[];
  /** Path of the model entity that owns it (the outermost insert for block geometry). */
  modelPath: number[];
  /** Face index in a `mesh` entity. */
  meshFace: number | null;
  entityId: number | null;
  /** Effective layer. */
  layer: string;
  entityType: EntityKindName;
  buildingId: string | null;
  polygonId: string | null;
}

export interface CityGmlBuildingReport {
  /** Source model index. */
  model: number;
  buildings: CityGmlBuildingSummary[];
  wallSurfaces: number;
  roofSurfaces: number;
  groundSurfaces: number;
  unclassifiedFaces: number;
  skippedFaces: number;
  skippedByCode: Record<string, number>;
  /** Visited entities that are not face candidates (lines, open polylines, text, ...). */
  ignoredEntities: number;
  /** Closed polylines not considered because `closedPolylinesAsSurfaces` is off. */
  closedPolylinesIgnored: number;
  /** Skipped faces in traversal order, then unclassified ones. */
  issues: CityGmlBuildingIssue[];
  /** More issues than min(maxObjects, 100,000): the rest are only counted. */
  issuesTruncated: boolean;
}

/** Result of `toCityGmlBuildings()`. */
export interface CityGmlBuildingsResult {
  /** UTF-8 XML, validated with geometry checks. */
  xml: Uint8Array;
  report: CityGmlBuildingReport;
}

/** Result of `toCityGmlBuildingsDocument()`; free `document` when done. */
export interface CityGmlBuildingsDocumentResult {
  document: CityGmlDocument;
  report: CityGmlBuildingReport;
}

/** A polygon or solid-shell issue of `CityGmlDocument.geometryIssues()`. The ids come from the file and may encode client data. */
export interface GmlGeometryIssue {
  /** Byte offset in the source XML (0 for built documents). */
  offset: number;
  code: string;
  message: string;
  /** Index of the polygon or solid in the element traversal (as `TkgmIssue.element`). */
  element: number;
  /** `gml:id` of the polygon or solid. */
  id: string | null;
  /** `gml:id` of the nearest enclosing city object, such as a wall surface. */
  objectId: string | null;
  buildingId: string | null;
}

// --- Renderer buffers (surfaceMesh, lineBuffer) -----------------------------------------------

/** Three finite numbers. */
export type Vector3 = [number, number, number] | Float32Array | Float64Array;

/** sRGB: `[r, g, b]` with integers 0 to 255, or `"#rrggbb"`. */
export type RgbOption = [number, number, number] | string;

/** Options shared by `surfaceMesh()` and `lineBuffer()`. Unknown keys are errors. */
export interface SceneWalkOptions {
  /** Model index (default 0). */
  model?: number;
  /** Deepest block and group nesting expanded (default 32, at most 64). */
  maxDepth?: number;
  /** Expand block references (default true). */
  expandInserts?: boolean;
  /** Walk work budget (default 20,000,000); when it runs out the buffers hold a prefix and the report says `truncated`. */
  maxWork?: number;
}

export interface SurfaceMeshOptions extends SceneWalkOptions {
  /** Closed polylines as faces: in 3D models only (`"auto"`, default), `"always"` or `"never"`. */
  polylines?: "auto" | "always" | "never";
  /** Largest distance of a polyline face vertex from its plane, as a fraction of its bounding-box diagonal (default 1e-3). */
  planarity?: number;
  /** Draw lines, open polylines and curves as segments (default true). */
  wires?: boolean;
  /** Draw face outlines as segments (default true). */
  outlines?: boolean;
  /** Chordal tolerance of flattened curves, drawing units (default 0.1 mm in drawing units). */
  tolerance?: number;
  /** Throw `limit_exceeded` (`limit: "maxError"`) when a stored f32 coordinate is further than this from the drawing coordinate (drawing units). Default: no limit. */
  maxError?: number;
  /** Colour of faces that resolve to black (ACI 7, no colour); `null` keeps black. Default [201, 204, 210]. */
  neutralColor?: RgbOption | null;
  /** Background that segment colours are contrasted against; default `null` (colours as resolved). */
  lineBackground?: RgbOption | null;
  /** Most triangles (default 2,000,000, at most 4,000,000). */
  maxTriangles?: number;
  /** Most segments (default 2,000,000, at most 4,000,000). */
  maxSegments?: number;
  /** Work budget of duplicate detection, hatch nesting and triangulation (default 200,000,000). */
  maxGeometryWork?: number;
}

export interface LineBufferOptions extends SceneWalkOptions {
  /** Store x, y and z per end point (default false: x and y). */
  threeD?: boolean;
  /** Chordal tolerance of flattened curves, drawing units. */
  tolerance?: number;
  /** As in `SurfaceMeshOptions`. */
  maxError?: number;
  /** Background that colours are contrasted against (default white); `null` keeps colours as resolved. */
  background?: RgbOption | null;
  /** Most segments (default 2,000,000, at most 4,000,000). */
  maxSegments?: number;
}

/** Origin and extents of a buffer. */
export interface SceneBounds {
  /** Drawing coordinates (f64) that every position is relative to. */
  origin: [number, number, number];
  /** Lowest corner of the box around all positions, relative to `origin`. */
  min: [number, number, number];
  /** Highest corner, relative to `origin`. */
  max: [number, number, number];
  /** Largest error of one stored f32 coordinate, drawing units. */
  maxError: number;
}

/** Where one layer's triangles and segments sit in the buffers. */
export interface SceneLayerRange {
  name: string;
  firstTriangle: number;
  triangleCount: number;
  firstSegment: number;
  segmentCount: number;
}

/** The item table as columns: item `i` has the path `paths.subarray(pathStart[i], pathStart[i] + pathLength[i])`. */
export interface SceneItemTable {
  count: number;
  pathStart: Uint32Array;
  pathLength: Uint32Array;
  /** Leading part of the path that addresses the model entity (the outermost insert). */
  modelPathLength: Uint32Array;
  /** Index into `layers()`. */
  layer: Uint32Array;
  /** Index into `kindNames`. */
  kind: Uint8Array;
  kindNames: EntityKindName[];
  /** 1 for a face, 0 for wire geometry. */
  surface: Uint8Array;
  paths: Uint32Array;
}

/** One item: a face or the wire geometry of one entity. */
export interface SceneItem {
  index: number;
  /** `entity(model, path)` returns the entity drawn. */
  path: number[];
  /** Path of the model entity that owns it. */
  modelPath: number[];
  layer: string | null;
  layerIndex: number;
  kind: EntityKindName;
  surface: boolean;
}

export interface SurfaceMeshReport {
  faces: number;
  triangles: number;
  segments: number;
  /** Entities drawn as wires. */
  wires: number;
  /** Faces left out because another face has the same corners. */
  duplicates: number;
  /** Faces and wires without area or length, or with non-finite coordinates. */
  degenerate: number;
  /** Curved entities flattened into segments. */
  flattened: number;
  /** Entities drawn neither as faces nor as wires, per kind. */
  skipped: Partial<Record<EntityKindName, number>>;
  /** A budget ran out: the buffers hold a prefix of the model in walk order. */
  truncated: boolean;
}

export interface LineBufferReport {
  drawn: number;
  segments: number;
  flattened: number;
  degenerate: number;
  skipped: Partial<Record<EntityKindName, number>>;
  truncated: boolean;
}

/** Result of `SurfaceMesh.pick()`. */
export interface RayHit {
  triangle: number;
  /** Index of the item (`item(hit.item)`). */
  item: number;
  /** Ray parameter of the hit; drawing units when the direction has unit length. */
  distance: number;
}
