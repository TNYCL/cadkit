// The order in which cadkit-wasm walks a model when it picks (unit-tested in
// test/demo-walk.test.mjs). A pick reports `index`: the position of the hit among the entities
// the engine's walk visits (cadkit_core::ops::Walker as published in cadkit-wasm 0.5), which is
// not an index into the model's entity list. This module repeats that walk over the neutral
// document JSON, so the worker can turn the counter back into the entity it names.
//
// The rules, as the engine applies them with block expansion on and hidden layers included:
// - an entity with `visible: false` is skipped together with everything below it;
// - a group is never visited itself; its children are, while the depth is below 32;
// - an insert whose block exists is replaced by the block's entities, once per array cell
//   (rows x columns, each at least 1), unless the depth is 32 or the block is already being
//   expanded further up (a cycle); then, and when the block is missing, the insert is visited;
// - a dimension that names an existing block is expanded like a one-cell insert;
// - everything else is visited (including viewports and unknown records).
// Block names resolve to the first block of that name, as in the engine.
// Keep this file free of non-erasable TypeScript syntax: Node imports it directly.

import type { DocJson, EntityJson } from './mesh.ts';

/** Nesting limit of the engine's walk (cadkit_core::ops::DEFAULT_WALK_DEPTH). */
export const WALK_MAX_DEPTH = 32;
/** Steps one lookup may take before it gives up (about what the pick's own work budget allows). */
export const MAX_LOCATE_STEPS = 4_000_000;

/** Where a picked entity sits in the document. */
export interface PickTarget {
	/**
	 * Index chain to the entity that was hit: from the model's entity list down through group
	 * children and, after an insert or a dimension, the entities of its block.
	 */
	part: number[];
	/**
	 * Index chain to the model entity that owns the part: the outermost insert or dimension when
	 * the part comes from a block, otherwise the part itself.
	 */
	owner: number[];
	/** Blocks entered between the owner and the part, outermost first (empty for a model entity). */
	blocks: string[];
}

type Blocks = Map<string, EntityJson[]>;

const blockCache = new WeakMap<DocJson, Blocks>();

/** Block entity lists by name; the first block of a name wins, as in the engine. */
function blocksOf(doc: DocJson): Blocks {
	let map = blockCache.get(doc);
	if (map) return map;
	map = new Map();
	for (const b of Array.isArray(doc.blocks) ? doc.blocks : []) {
		const block = b as { name?: unknown; entities?: unknown } | null;
		if (!block || typeof block.name !== 'string' || map.has(block.name)) continue;
		map.set(block.name, Array.isArray(block.entities) ? (block.entities as EntityJson[]) : []);
	}
	blockCache.set(doc, map);
	return map;
}

/** Array cells of an insert: rows x columns, each at least one (u16 in the model). */
function cellsOf(k: EntityJson['kind']): number {
	const n = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? Math.max(1, Math.floor(v)) : 1);
	return k.type === 'insert' ? n(k.rows) * n(k.columns) : 1;
}

/** The block an insert or dimension refers to, by name, or null when it has none. */
function blockName(e: EntityJson): string | null {
	const k = e.kind;
	if ((k.type === 'insert' || k.type === 'dimension') && typeof k.block === 'string') return k.block;
	return null;
}

/** Thrown inside the walk when the step budget runs out. */
const EXHAUSTED = Symbol('exhausted');

interface Walk {
	blocks: Blocks;
	/** Steps left before the lookup gives up. */
	left: number;
	/** Visited-entity counts of one expansion of a block, by block, depth and stack. */
	memo: Map<string, number>;
}

/** How a list entry is walked at this depth with these blocks on the stack. */
function expansion(w: Walk, e: EntityJson, depth: number, stack: string[]): { list: EntityJson[]; name: string | null; cells: number } | null {
	const k = e.kind;
	if (k.type === 'group') {
		return { list: depth < WALK_MAX_DEPTH && Array.isArray(k.children) ? (k.children as EntityJson[]) : [], name: null, cells: 1 };
	}
	const name = blockName(e);
	if (name === null) return null;
	const list = w.blocks.get(name);
	if (!list || depth >= WALK_MAX_DEPTH || stack.includes(name)) return null;
	return { list, name, cells: cellsOf(k) };
}

/** Number of entities the engine visits for one list entry. */
function countEntity(w: Walk, e: EntityJson | null | undefined, depth: number, stack: string[]): number {
	if (--w.left < 0) throw EXHAUSTED;
	if (!e || !e.kind || typeof e.kind.type !== 'string' || e.visible === false) return 0;
	const x = expansion(w, e, depth, stack);
	if (!x) return 1;
	if (x.name === null) return countList(w, x.list, depth + 1, stack);
	const key = x.name + '\u0000' + depth + '\u0000' + stack.join('\u0000');
	let one = w.memo.get(key);
	if (one === undefined) {
		one = countList(w, x.list, depth + 1, [...stack, x.name]);
		w.memo.set(key, one);
	}
	return one * x.cells;
}

function countList(w: Walk, list: EntityJson[], depth: number, stack: string[]): number {
	let n = 0;
	for (const e of list) n += countEntity(w, e, depth, stack);
	return n;
}

function locateIn(w: Walk, list: EntityJson[], depth: number, stack: string[], target: number, path: number[], blocks: string[], ownerLength: number | null): PickTarget | null {
	for (let i = 0; i < list.length; i++) {
		const e = list[i];
		const n = countEntity(w, e, depth, stack);
		if (target >= n) {
			target -= n;
			continue;
		}
		const here = [...path, i];
		const x = e ? expansion(w, e, depth, stack) : null;
		if (!x) return { part: here, owner: here.slice(0, ownerLength ?? here.length), blocks };
		if (x.name === null) return locateIn(w, x.list, depth + 1, stack, target, here, blocks, ownerLength);
		// Every cell of an array visits the same block entities: only the position within one cell matters.
		const one = n / x.cells;
		return locateIn(w, x.list, depth + 1, [...stack, x.name], target % one, here, [...blocks, x.name], ownerLength ?? here.length);
	}
	return null;
}

/**
 * The entity behind a pick's `index` in `models[model]`, or null when the index is past the end
 * of the walk or the lookup would take more than MAX_LOCATE_STEPS steps.
 */
export function locatePick(doc: DocJson, model: number, index: number): PickTarget | null {
	const list = doc.models?.[model]?.entities;
	if (!Array.isArray(list) || !Number.isInteger(index) || index < 0) return null;
	const w: Walk = { blocks: blocksOf(doc), left: MAX_LOCATE_STEPS, memo: new Map() };
	try {
		return locateIn(w, list, 0, [], index, [], [], null);
	} catch (e) {
		if (e === EXHAUSTED) return null;
		throw e;
	}
}

/** Number of entities the engine's walk visits in `models[model]`; null past MAX_LOCATE_STEPS. */
export function walkCount(doc: DocJson, model: number): number | null {
	const list = doc.models?.[model]?.entities;
	if (!Array.isArray(list)) return 0;
	const w: Walk = { blocks: blocksOf(doc), left: MAX_LOCATE_STEPS, memo: new Map() };
	try {
		return countList(w, list, 0, []);
	} catch (e) {
		if (e === EXHAUSTED) return null;
		throw e;
	}
}

/**
 * The entity an index chain points at, with the entities it passes on the way (outermost
 * first). After a group the next index is into its children; after an insert or a dimension
 * it is into the entities of the block it names.
 */
export function entityAt(doc: DocJson, model: number, path: number[]): { entity: EntityJson; parents: EntityJson[] } | null {
	let list: EntityJson[] | undefined = doc.models?.[model]?.entities;
	const parents: EntityJson[] = [];
	let entity: EntityJson | undefined;
	for (let i = 0; i < path.length; i++) {
		entity = Array.isArray(list) ? list[path[i]!] : undefined;
		if (!entity || !entity.kind) return null;
		if (i < path.length - 1) {
			parents.push(entity);
			const children = entity.kind.children;
			const name = blockName(entity);
			list = Array.isArray(children) ? (children as EntityJson[]) : name !== null ? blocksOf(doc).get(name) : undefined;
		}
	}
	return entity ? { entity, parents } : null;
}
