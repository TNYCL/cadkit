// 3D surfaces of a model for the orbit view (unit-tested in test/demo-scene.test.mjs). Runs in
// the worker on the neutral document JSON: collects polygons, faces and meshes (through groups)
// and, in a 3D model, closed planar polylines (DGN shape elements arrive that way), drops
// duplicate faces, triangulates them and packs the result into typed arrays the page can hand to
// WebGL. No DOM and no WebGL here.

import { modelRgb, type ModelColor, type Rgb } from './colors.ts';

export { entityAt } from './walk.ts';

export interface P3 {
	x: number;
	y: number;
	z: number;
}

/** The parts of the neutral document JSON (docs/schema/document-v1.schema.json) used here. */
export interface EntityJson {
	id?: number | null;
	layer?: string;
	color?: ModelColor;
	visible?: boolean;
	kind: { type: string; [key: string]: unknown };
	attributes?: unknown[];
	props?: Record<string, unknown>;
}
export interface DocJson {
	layers?: { name: string; color?: ModelColor }[];
	blocks?: unknown[];
	models?: { name?: string; kind?: string; entities?: EntityJson[] }[];
	[key: string]: unknown;
}

/** Budgets: a model past them is drawn up to the limit and reported as truncated. */
export const MAX_MESH_TRIANGLES = 600_000;
export const MAX_MESH_SEGMENTS = 600_000;
const MAX_RING_VERTICES = 4096;
const MAX_WALK_DEPTH = 32;
/** A closed polyline is a surface when no vertex is further than this from its plane (fraction of its size). */
export const SHAPE_PLANAR_TOLERANCE = 1e-3;
/**
 * A model whose only surfaces are closed polylines opens in 3D when its height is at least this
 * fraction of its larger horizontal extent. Below it, the polylines are mostly a plan drawn in
 * 3D coordinates (footprints, slabs on a few levels) and the sheet is the better first view; the
 * 3D view is still offered.
 */
export const SHAPES_OPEN_3D_RATIO = 0.12;

/** Which entities count as surfaces. */
export interface SurfaceOptions {
	/** Closed, planar, bulge-free polylines with three or more vertices are surfaces (3D models). */
	shapes?: boolean;
}

/** Colour of surfaces whose colour is the sheet foreground (index 7) or not set. */
const NEUTRAL: Rgb = [201, 204, 210];

// ---------------------------------------------------------------------------------------------
// Triangulation of one planar polygon with holes (ear clipping).

type P2 = [number, number];

/** Twice the signed area: positive for a counter-clockwise ring (Y up). */
const area2 = (ring: P2[]): number => {
	let a = 0;
	for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) a += (ring[j]![0] - ring[i]![0]) * (ring[i]![1] + ring[j]![1]);
	return a;
};
const cross = (a: P2, b: P2, c: P2): number => (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
const inTriangle = (p: P2, a: P2, b: P2, c: P2): boolean => cross(a, b, p) >= 0 && cross(b, c, p) >= 0 && cross(c, a, p) >= 0;

/** Proper intersection of segments ab and cd (shared end points do not count). */
function segmentsCross(a: P2, b: P2, c: P2, d: P2): boolean {
	const d1 = cross(c, d, a);
	const d2 = cross(c, d, b);
	const d3 = cross(a, b, c);
	const d4 = cross(a, b, d);
	return ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0));
}

/** True when q lies on the open segment ab (collinear and strictly between the end points). */
function onSegment(a: P2, b: P2, q: P2): boolean {
	if (cross(a, b, q) !== 0) return false;
	if ((q[0] === a[0] && q[1] === a[1]) || (q[0] === b[0] && q[1] === b[1])) return false;
	return q[0] >= Math.min(a[0], b[0]) && q[0] <= Math.max(a[0], b[0]) && q[1] >= Math.min(a[1], b[1]) && q[1] <= Math.max(a[1], b[1]);
}

/** Even-odd point-in-ring test (points on the boundary may fall either way). */
function insideRing(ring: P2[], q: P2): boolean {
	let inside = false;
	for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
		const a = ring[i]!;
		const b = ring[j]!;
		if (a[1] > q[1] !== b[1] > q[1] && q[0] < ((b[0] - a[0]) * (q[1] - a[1])) / (b[1] - a[1]) + a[0]) inside = !inside;
	}
	return inside;
}

/**
 * Triangulates a polygon given as rings of 2D points (the first ring is the outline, the rest
 * are holes). Returns triangles as indices into the concatenation of the rings. Rings may have
 * either winding and must not repeat their first point at the end. Degenerate input gives fewer
 * (or no) triangles; the function always terminates.
 */
export function triangulate(rings: P2[][]): number[] {
	const outer = rings[0];
	if (!outer || outer.length < 3) return [];
	// Work on index loops into the concatenated point list.
	const pts: P2[] = [];
	const loops: number[][] = [];
	for (const ring of rings) {
		if (ring.length < 3) {
			for (const p of ring) pts.push(p);
			continue;
		}
		const base = pts.length;
		const idx = ring.map((_, i) => base + i);
		for (const p of ring) pts.push(p);
		loops.push(idx);
	}
	if (!loops.length) return [];
	const at = (i: number): P2 => pts[i]!;
	const signed = (loop: number[]) => area2(loop.map(at));
	// Outline counter-clockwise, holes clockwise.
	let loop = loops[0]!;
	if (signed(loop) < 0) loop = [...loop].reverse();
	const holes = loops
		.slice(1)
		.map((h) => (signed(h) > 0 ? [...h].reverse() : h))
		.sort((a, b) => Math.max(...b.map((i) => at(i)[0])) - Math.max(...a.map((i) => at(i)[0])));
	// Join each hole to the outline with a bridge between two mutually visible vertices.
	for (const hole of holes) {
		let hi = 0;
		for (let k = 1; k < hole.length; k++) if (at(hole[k]!)[0] > at(hole[hi]!)[0]) hi = k;
		const m = at(hole[hi]!);
		const edges: [P2, P2][] = [];
		const corners: P2[] = [];
		for (const l of [loop, ...holes])
			for (let k = 0; k < l.length; k++) {
				edges.push([at(l[k]!), at(l[(k + 1) % l.length]!)]);
				corners.push(at(l[k]!));
			}
		const holeRings = holes.map((h) => h.map(at));
		let best = -1;
		let bestD = Infinity;
		for (let k = 0; k < loop.length; k++) {
			const p = at(loop[k]!);
			const dist = (p[0] - m[0]) ** 2 + (p[1] - m[1]) ** 2;
			if (dist >= bestD) continue;
			// The bridge must not cross an edge, run through a corner, or cut through a hole.
			if (edges.some(([a, b]) => segmentsCross(m, p, a, b))) continue;
			if (corners.some((c) => onSegment(m, p, c))) continue;
			const mid: P2 = [(m[0] + p[0]) / 2, (m[1] + p[1]) / 2];
			if (holeRings.some((r) => insideRing(r, mid))) continue;
			best = k;
			bestD = dist;
		}
		if (best < 0) continue; // No visible vertex (invalid input): leave the hole out.
		const rotated = [...hole.slice(hi), ...hole.slice(0, hi)];
		loop = [...loop.slice(0, best + 1), ...rotated, rotated[0]!, loop[best]!, ...loop.slice(best + 1)];
	}
	// Ear clipping.
	const out: number[] = [];
	const ring = [...loop];
	let guard = ring.length * ring.length + 16;
	while (ring.length > 3 && guard-- > 0) {
		let clipped = false;
		for (let i = 0; i < ring.length; i++) {
			const ia = ring[(i + ring.length - 1) % ring.length]!;
			const ib = ring[i]!;
			const ic = ring[(i + 1) % ring.length]!;
			const [a, b, c] = [at(ia), at(ib), at(ic)];
			const turn = cross(a, b, c);
			if (turn < 0) continue; // Reflex corner.
			if (turn === 0) {
				// Collinear or repeated point: drop it without a triangle.
				ring.splice(i, 1);
				clipped = true;
				break;
			}
			let empty = true;
			for (const j of ring) {
				if (j === ia || j === ib || j === ic) continue;
				const p = at(j);
				if ((p[0] === a[0] && p[1] === a[1]) || (p[0] === b[0] && p[1] === b[1]) || (p[0] === c[0] && p[1] === c[1])) continue;
				if (inTriangle(p, a, b, c)) {
					empty = false;
					break;
				}
			}
			if (!empty) continue;
			out.push(ia, ib, ic);
			ring.splice(i, 1);
			clipped = true;
			break;
		}
		if (!clipped) {
			// Self-intersecting or otherwise invalid outline: cut a corner anyway so the loop ends.
			out.push(ring[0]!, ring[1]!, ring[2]!);
			ring.splice(1, 1);
		}
	}
	if (ring.length === 3 && cross(at(ring[0]!), at(ring[1]!), at(ring[2]!)) !== 0) out.push(ring[0]!, ring[1]!, ring[2]!);
	return out;
}

// ---------------------------------------------------------------------------------------------
// Mesh of a model.

/** Where a face came from: the index chain from the model's entity list down through groups. */
export interface FaceRef {
	path: number[];
	layer: string;
	kind: string;
}

export interface LayerRange {
	name: string;
	/** First triangle and triangle count. */
	tri: [number, number];
	/** First segment and segment count (face outlines and wires). */
	seg: [number, number];
}

export interface MeshData {
	/** Triangle vertices, xyz relative to `center` (3 vertices per triangle, not indexed). */
	positions: Float32Array;
	/** Face normal per vertex. */
	normals: Float32Array;
	/** RGB per vertex. */
	colors: Uint8Array;
	/** Face index per triangle (into `faces`). */
	faceOfTri: Uint32Array;
	/** Line segments, xyz relative to `center` (2 vertices per segment). */
	segments: Float32Array;
	/** Triangles and segments are grouped by layer in this order. */
	layers: LayerRange[];
	faces: FaceRef[];
	/** World coordinates of the local origin (f64). */
	center: [number, number, number];
	/** Extents relative to `center`. */
	min: [number, number, number];
	max: [number, number, number];
	/** Faces left out because another face has the same outline. */
	duplicates: number;
	truncated: boolean;
}

interface RawFace {
	rings: P3[][];
	ref: FaceRef;
	color: Rgb;
	/** From a closed polyline, not a polygon, face or mesh. */
	shape: boolean;
}
interface RawWire {
	pts: P3[];
	closed: boolean;
	layer: string;
}

const isP3 = (p: unknown): p is P3 => {
	const q = p as P3 | null;
	return !!q && Number.isFinite(q.x) && Number.isFinite(q.y) && Number.isFinite(q.z);
};
const ringOf = (value: unknown): P3[] => {
	if (!Array.isArray(value)) return [];
	const pts = value.filter(isP3);
	const n = pts.length;
	// Rings may repeat the first point at the end (GML does): drop the repeat.
	if (n > 1 && pts[0]!.x === pts[n - 1]!.x && pts[0]!.y === pts[n - 1]!.y && pts[0]!.z === pts[n - 1]!.z) pts.pop();
	return pts.slice(0, MAX_RING_VERTICES);
};

/** True when every point of the ring lies within SHAPE_PLANAR_TOLERANCE of its size from its plane. */
function planar(ring: P3[]): boolean {
	const [nx, ny, nz] = newell(ring);
	const len = Math.hypot(nx, ny, nz);
	if (!(len > 0)) return false; // Collinear or repeated points: no area.
	let x0 = Infinity;
	let y0 = Infinity;
	let z0 = Infinity;
	let x1 = -Infinity;
	let y1 = -Infinity;
	let z1 = -Infinity;
	for (const p of ring) {
		x0 = Math.min(x0, p.x);
		y0 = Math.min(y0, p.y);
		z0 = Math.min(z0, p.z);
		x1 = Math.max(x1, p.x);
		y1 = Math.max(y1, p.y);
		z1 = Math.max(z1, p.z);
	}
	const tol = SHAPE_PLANAR_TOLERANCE * Math.max(x1 - x0, y1 - y0, z1 - z0);
	const o = ring[0]!;
	return ring.every((p) => Math.abs(((p.x - o.x) * nx + (p.y - o.y) * ny + (p.z - o.z) * nz) / len) <= tol);
}

function collect(doc: DocJson, model: number, opts: SurfaceOptions = {}): { faces: RawFace[]; wires: RawWire[] } {
	const layerColor = new Map<string, ModelColor | undefined>((doc.layers ?? []).map((l) => [l.name, l.color]));
	const faces: RawFace[] = [];
	const wires: RawWire[] = [];
	const colorOf = (e: EntityJson): Rgb => modelRgb(e.color, false) ?? modelRgb(layerColor.get(e.layer ?? ''), false) ?? NEUTRAL;
	const tint = (c: Rgb): Rgb => (c[0] + c[1] + c[2] === 0 ? NEUTRAL : c);
	const walk = (list: EntityJson[], path: number[], depth: number) => {
		list.forEach((e, i) => {
			if (!e || !e.kind || e.visible === false) return;
			const here = [...path, i];
			const layer = e.layer ?? '';
			const k = e.kind;
			const ref = (): FaceRef => ({ path: here, layer, kind: k.type });
			const face = (rings: P3[][], shape = false) => {
				if (rings[0] && rings[0].length >= 3) faces.push({ rings, ref: ref(), color: tint(colorOf(e)), shape });
			};
			switch (k.type) {
				case 'group':
					if (depth < MAX_WALK_DEPTH && Array.isArray(k.children)) walk(k.children as EntityJson[], here, depth + 1);
					break;
				case 'polygon':
					face([ringOf(k.exterior), ...(Array.isArray(k.interiors) ? k.interiors.map(ringOf).filter((r) => r.length >= 3) : [])]);
					break;
				case 'face':
					face([ringOf(k.points)]);
					break;
				case 'mesh': {
					const vs = Array.isArray(k.vertices) ? (k.vertices as unknown[]) : [];
					for (const f of Array.isArray(k.faces) ? (k.faces as unknown[]) : []) {
						if (!Array.isArray(f)) continue;
						face([f.map((n) => vs[n as number]).filter(isP3)]);
					}
					break;
				}
				case 'line':
					if (isP3(k.start) && isP3(k.end)) wires.push({ pts: [k.start, k.end], closed: false, layer });
					break;
				case 'polyline': {
					const vertices = Array.isArray(k.vertices) ? (k.vertices as ({ position?: unknown; bulge?: unknown } | null)[]) : [];
					const pts = vertices.map((v) => v?.position).filter(isP3);
					if (opts.shapes && k.closed === true && pts.length === vertices.length && pts.length <= MAX_RING_VERTICES && vertices.every((v) => !v?.bulge)) {
						const ring = ringOf(pts);
						if (ring.length >= 3 && planar(ring)) {
							face([ring], true);
							break;
						}
					}
					if (pts.length >= 2) wires.push({ pts, closed: k.closed === true, layer });
					break;
				}
				default:
					break;
			}
		});
	};
	walk(doc.models?.[model]?.entities ?? [], [], 0);
	return { faces, wires };
}

/** What the 3D view can show of a model. */
export interface SurfaceInfo {
	/** All surfaces, closed planar polylines included when `shapes` is on. */
	surfaces: number;
	/** Of those, closed planar polylines. */
	shapes: number;
	/** The surfaces leave the XY plane: the 3D view is worth offering. */
	spatial: boolean;
	/**
	 * The model opens in 3D: it has polygons, faces or meshes that leave the XY plane, or only
	 * closed polylines whose height is at least SHAPES_OPEN_3D_RATIO of its larger horizontal extent.
	 */
	open3d: boolean;
}

/** Surfaces of a model and how it should open (see SurfaceInfo). */
export function surfaceInfo(doc: DocJson, model: number, opts: SurfaceOptions = {}): SurfaceInfo {
	const { faces } = collect(doc, model, opts);
	let zmin = Infinity;
	let zmax = -Infinity;
	let span = 0;
	let x0 = Infinity;
	let x1 = -Infinity;
	let y0 = Infinity;
	let y1 = -Infinity;
	for (const f of faces)
		for (const r of f.rings)
			for (const p of r) {
				zmin = Math.min(zmin, p.z);
				zmax = Math.max(zmax, p.z);
				x0 = Math.min(x0, p.x);
				x1 = Math.max(x1, p.x);
				y0 = Math.min(y0, p.y);
				y1 = Math.max(y1, p.y);
			}
	if (faces.length) span = Math.max(x1 - x0, y1 - y0);
	const shapes = faces.filter((f) => f.shape).length;
	const spatial = faces.length > 0 && zmax - zmin > 1e-9 * Math.max(1, span);
	const open3d = spatial && (shapes < faces.length || zmax - zmin >= SHAPES_OPEN_3D_RATIO * span);
	return { surfaces: faces.length, shapes, spatial, open3d };
}

/** Number of polygons, faces and mesh faces in a model and whether they leave the XY plane. */
export function surfaceStats(doc: DocJson, model: number): { surfaces: number; spatial: boolean } {
	const { surfaces, spatial } = surfaceInfo(doc, model);
	return { surfaces, spatial };
}

/** Newell normal of a ring (not normalised). */
function newell(ring: P3[]): [number, number, number] {
	let nx = 0;
	let ny = 0;
	let nz = 0;
	for (let i = 0; i < ring.length; i++) {
		const a = ring[i]!;
		const b = ring[(i + 1) % ring.length]!;
		nx += (a.y - b.y) * (a.z + b.z);
		ny += (a.z - b.z) * (a.x + b.x);
		nz += (a.x - b.x) * (a.y + b.y);
	}
	return [nx, ny, nz];
}

/**
 * Builds the mesh of one model: null when it has no surfaces. Two faces with the same set of
 * corner points are drawn once; the more deeply nested one wins. A CityGML building lists the
 * faces of its solid directly and again inside each boundary surface (wall, roof, ground), and
 * the nested copy carries the surface type as its layer.
 */
export function buildMesh(doc: DocJson, model: number, opts: SurfaceOptions = {}): MeshData | null {
	const { faces: raw, wires } = collect(doc, model, opts);
	if (!raw.length) return null;

	// Extents and local origin.
	const lo = [Infinity, Infinity, Infinity];
	const hi = [-Infinity, -Infinity, -Infinity];
	const grow = (p: P3) => {
		lo[0] = Math.min(lo[0]!, p.x);
		lo[1] = Math.min(lo[1]!, p.y);
		lo[2] = Math.min(lo[2]!, p.z);
		hi[0] = Math.max(hi[0]!, p.x);
		hi[1] = Math.max(hi[1]!, p.y);
		hi[2] = Math.max(hi[2]!, p.z);
	};
	for (const f of raw) for (const r of f.rings) r.forEach(grow);
	const center: [number, number, number] = [(lo[0]! + hi[0]!) / 2, (lo[1]! + hi[1]!) / 2, (lo[2]! + hi[2]!) / 2];
	const size = Math.max(hi[0]! - lo[0]!, hi[1]! - lo[1]!, hi[2]! - lo[2]!, 1e-12);

	// Duplicates: same corner set after rounding to a millionth of the model size.
	const q = (v: number, c: number) => Math.round(((v - c) / size) * 1e6);
	const keyOf = (f: RawFace) =>
		f.rings[0]!
			.map((p) => q(p.x, center[0]) + ',' + q(p.y, center[1]) + ',' + q(p.z, center[2]))
			.sort()
			.join(';');
	const byKey = new Map<string, number>();
	const kept: RawFace[] = [];
	let duplicates = 0;
	for (const f of raw) {
		const key = keyOf(f);
		const prev = byKey.get(key);
		if (prev === undefined) {
			byKey.set(key, kept.length);
			kept.push(f);
			continue;
		}
		duplicates++;
		if (f.ref.path.length > kept[prev]!.ref.path.length) kept[prev] = f;
	}

	// Group by layer so a layer is one contiguous range of triangles and segments.
	const layerNames = [...new Set([...kept.map((f) => f.ref.layer), ...wires.map((w) => w.layer)])].sort((a, b) => a.localeCompare(b));
	const positions: number[] = [];
	const normals: number[] = [];
	const colors: number[] = [];
	const faceOfTri: number[] = [];
	const segments: number[] = [];
	const faces: FaceRef[] = [];
	const layers: LayerRange[] = [];
	let truncated = false;
	const rel = (p: P3): [number, number, number] => [p.x - center[0], p.y - center[1], p.z - center[2]];
	const segment = (a: P3, b: P3) => {
		if (segments.length / 6 >= MAX_MESH_SEGMENTS) {
			truncated = true;
			return;
		}
		segments.push(...rel(a), ...rel(b));
	};

	for (const name of layerNames) {
		const tri0 = faceOfTri.length;
		const seg0 = segments.length / 6;
		for (const f of kept) {
			if (f.ref.layer !== name) continue;
			const [nx, ny, nz] = newell(f.rings[0]!);
			const len = Math.hypot(nx, ny, nz);
			if (!(len > 0)) continue; // Degenerate (zero-area) face.
			const n: [number, number, number] = [nx / len, ny / len, nz / len];
			// Project onto the plane's dominant axis pair.
			const ax = Math.abs(n[0]);
			const ay = Math.abs(n[1]);
			const az = Math.abs(n[2]);
			const drop = ax >= ay && ax >= az ? 0 : ay >= az ? 1 : 2;
			const flat = (p: P3): P2 => {
				const r = rel(p);
				return drop === 0 ? [r[1], r[2]] : drop === 1 ? [r[0], r[2]] : [r[0], r[1]];
			};
			const all = f.rings.flat();
			const tris = triangulate(f.rings.map((r) => r.map(flat)));
			if (!tris.length) continue;
			if (faceOfTri.length + tris.length / 3 > MAX_MESH_TRIANGLES) {
				truncated = true;
				break;
			}
			const faceIndex = faces.length;
			faces.push(f.ref);
			for (let t = 0; t < tris.length; t += 3) {
				for (let k = 0; k < 3; k++) {
					positions.push(...rel(all[tris[t + k]!]!));
					normals.push(n[0], n[1], n[2]);
					colors.push(f.color[0], f.color[1], f.color[2]);
				}
				faceOfTri.push(faceIndex);
			}
			for (const ring of f.rings) for (let i = 0; i < ring.length; i++) segment(ring[i]!, ring[(i + 1) % ring.length]!);
		}
		for (const w of wires) {
			if (w.layer !== name) continue;
			for (let i = 1; i < w.pts.length; i++) segment(w.pts[i - 1]!, w.pts[i]!);
			if (w.closed && w.pts.length > 2) segment(w.pts[w.pts.length - 1]!, w.pts[0]!);
		}
		layers.push({ name, tri: [tri0, faceOfTri.length - tri0], seg: [seg0, segments.length / 6 - seg0] });
	}
	if (!faceOfTri.length) return null;

	return {
		positions: new Float32Array(positions),
		normals: new Float32Array(normals),
		colors: new Uint8Array(colors),
		faceOfTri: new Uint32Array(faceOfTri),
		segments: new Float32Array(segments),
		layers,
		faces,
		center,
		min: [lo[0]! - center[0], lo[1]! - center[1], lo[2]! - center[2]],
		max: [hi[0]! - center[0], hi[1]! - center[1], hi[2]! - center[2]],
		duplicates,
		truncated,
	};
}

/**
 * Nearest face hit by a ray (origin and direction relative to the mesh centre), or -1.
 * `visible` tells whether a triangle's layer is shown.
 */
export function pickFace(
	positions: Float32Array,
	faceOfTri: Uint32Array,
	origin: [number, number, number],
	dir: [number, number, number],
	visible: (tri: number) => boolean = () => true
): number {
	let best = Infinity;
	let face = -1;
	const [ox, oy, oz] = origin;
	const [dx, dy, dz] = dir;
	for (let t = 0; t < faceOfTri.length; t++) {
		const o = t * 9;
		const ax = positions[o]!;
		const ay = positions[o + 1]!;
		const az = positions[o + 2]!;
		const e1x = positions[o + 3]! - ax;
		const e1y = positions[o + 4]! - ay;
		const e1z = positions[o + 5]! - az;
		const e2x = positions[o + 6]! - ax;
		const e2y = positions[o + 7]! - ay;
		const e2z = positions[o + 8]! - az;
		// Moller-Trumbore, both sides.
		const px = dy * e2z - dz * e2y;
		const py = dz * e2x - dx * e2z;
		const pz = dx * e2y - dy * e2x;
		const det = e1x * px + e1y * py + e1z * pz;
		if (Math.abs(det) < 1e-12) continue;
		const inv = 1 / det;
		const tx = ox - ax;
		const ty = oy - ay;
		const tz = oz - az;
		const u = (tx * px + ty * py + tz * pz) * inv;
		if (u < 0 || u > 1) continue;
		const qx = ty * e1z - tz * e1y;
		const qy = tz * e1x - tx * e1z;
		const qz = tx * e1y - ty * e1x;
		const v = (dx * qx + dy * qy + dz * qz) * inv;
		if (v < 0 || u + v > 1) continue;
		const dist = (e2x * qx + e2y * qy + e2z * qz) * inv;
		if (dist <= 0 || dist >= best || !visible(t)) continue;
		best = dist;
		face = faceOfTri[t]!;
	}
	return face;
}
