// Axonometric line drawing of 3D polygons (CityGML surfaces read by cadkit), for the landing page.
// cadkit-wasm renders a 3D model as a plan view; this projects the polygons of the document
// model instead, hides back faces and paints the rest far to near, so a district of buildings
// reads as a drawing. Pure (no I/O) so the tests can feed it fixtures.

const RAD = Math.PI / 180;

/** Every polygon entity below `entities`, groups expanded (depth-bounded), as 3D rings. */
export function collectPolygons(entities, maxDepth = 8) {
	/** @type {{ owner: number, ring: {x: number, y: number, z: number}[] }[]} */
	const out = [];
	const walk = (list, depth, owner) => {
		if (depth > maxDepth) return;
		list.forEach((e, i) => {
			const k = e?.kind;
			if (!k || e.visible === false) return;
			const o = owner ?? i;
			if (k.type === 'group' && Array.isArray(k.children)) walk(k.children, depth + 1, o);
			else if (k.type === 'polygon' && Array.isArray(k.exterior) && k.exterior.length >= 3)
				out.push({ owner: o, ring: k.exterior.map((p) => ({ x: p.x, y: p.y, z: p.z ?? 0 })) });
		});
	};
	walk(entities, 0, null);
	return out;
}

const sub = (a, b) => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });
const dot = (a, b) => a.x * b.x + a.y * b.y + a.z * b.z;
const mean = (ps) => {
	const n = ps.length || 1;
	return ps.reduce((s, p) => ({ x: s.x + p.x / n, y: s.y + p.y / n, z: s.z + p.z / n }), { x: 0, y: 0, z: 0 });
};

/** Newell normal of a ring (not normalised). */
export function newell(ring) {
	const n = { x: 0, y: 0, z: 0 };
	for (let i = 0; i < ring.length; i++) {
		const a = ring[i];
		const b = ring[(i + 1) % ring.length];
		n.x += (a.y - b.y) * (a.z + b.z);
		n.y += (a.z - b.z) * (a.x + b.x);
		n.z += (a.x - b.x) * (a.y + b.y);
	}
	const len = Math.hypot(n.x, n.y, n.z) || 1;
	return { x: n.x / len, y: n.y / len, z: n.z / len };
}

/**
 * Projects closed solids (polygons grouped by owner) to an axonometric view.
 * @param {{ owner: number, ring: {x: number, y: number, z: number}[] }[]} polygons
 * @param {{ azimuth?: number, elevation?: number, precision?: number }} [opts] degrees
 * @returns {{ viewBox: [number, number, number, number], faces: { kind: 'roof' | 'wall-a' | 'wall-b', d: string }[], ground: string | null }}
 */
export function axonometric(polygons, opts = {}) {
	const a = (opts.azimuth ?? 32) * RAD;
	const e = (opts.elevation ?? 42) * RAD;
	const prec = opts.precision ?? 2;
	const right = { x: Math.cos(a), y: -Math.sin(a), z: 0 };
	const up = { x: Math.sin(a) * Math.sin(e), y: Math.cos(a) * Math.sin(e), z: Math.cos(e) };
	const toViewer = { x: -Math.sin(a) * Math.cos(e), y: -Math.cos(a) * Math.cos(e), z: Math.sin(e) };

	// Local origin: georeferenced coordinates (x = 500 000, y = 5 600 000) would lose precision.
	const all = polygons.flatMap((p) => p.ring);
	if (!all.length) return { viewBox: [0, 0, 1, 1], faces: [], ground: null };
	const o = { x: Math.min(...all.map((p) => p.x)), y: Math.min(...all.map((p) => p.y)), z: Math.min(...all.map((p) => p.z)) };
	const local = polygons.map((p) => ({ owner: p.owner, ring: p.ring.map((q) => sub(q, o)) }));

	// Orient every normal away from its solid's centre, whatever the ring order in the file.
	const centres = new Map();
	for (const p of local) {
		const list = centres.get(p.owner) ?? [];
		list.push(...p.ring);
		centres.set(p.owner, list);
	}
	for (const [k, ps] of centres) centres.set(k, mean(ps));

	const faces = [];
	for (const p of local) {
		let n = newell(p.ring);
		const c = mean(p.ring);
		if (dot(n, sub(c, centres.get(p.owner))) < 0) n = { x: -n.x, y: -n.y, z: -n.z };
		if (dot(n, toViewer) <= 1e-6) continue; // back face (grounds included)
		const kind = n.z > 0.2 ? 'roof' : dot(n, right) > 0 ? 'wall-b' : 'wall-a';
		faces.push({ kind, depth: dot(c, toViewer), pts: p.ring.map((q) => [dot(q, right), -dot(q, up)]) });
	}
	faces.sort((f, g) => f.depth - g.depth);

	// The site plane: the footprint bounding box, 4 m larger on every side, at the lowest z.
	const xs = all.map((p) => p.x - o.x);
	const ys = all.map((p) => p.y - o.y);
	const m = 4;
	const [x0, x1, y0, y1] = [Math.min(...xs) - m, Math.max(...xs) + m, Math.min(...ys) - m, Math.max(...ys) + m];
	const plane = [
		{ x: x0, y: y0, z: 0 },
		{ x: x1, y: y0, z: 0 },
		{ x: x1, y: y1, z: 0 },
		{ x: x0, y: y1, z: 0 },
	].map((q) => [dot(q, right), -dot(q, up)]);

	const pts = [...plane, ...faces.flatMap((f) => f.pts)];
	const minX = Math.min(...pts.map((p) => p[0]));
	const minY = Math.min(...pts.map((p) => p[1]));
	const maxX = Math.max(...pts.map((p) => p[0]));
	const maxY = Math.max(...pts.map((p) => p[1]));
	const r = (v) => Number(v.toFixed(prec));
	const path = (ps) => 'M' + ps.map(([x, y]) => `${r(x - minX)} ${r(y - minY)}`).join('L') + 'Z';
	return {
		viewBox: [0, 0, r(maxX - minX), r(maxY - minY)],
		faces: faces.map((f) => ({ kind: f.kind, d: path(f.pts) })),
		ground: path(plane),
	};
}
