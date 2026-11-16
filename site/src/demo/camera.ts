// Orbit camera of the 3D view (unit-tested in test/demo-scene.test.mjs). World is Z up; the
// camera looks at `target` from a distance, turned by `yaw` around Z and raised by `pitch`.
// Matrices are column-major Float32Array(16), as WebGL takes them.

export type V3 = [number, number, number];

export interface Camera {
	target: V3;
	/** Radians around Z, 0 looks along -X... measured from +X towards +Y for the eye position. */
	yaw: number;
	/** Radians above the XY plane, clamped to (-PITCH_LIMIT, PITCH_LIMIT). */
	pitch: number;
	/** Distance from the target, in model units. */
	distance: number;
}

/** Vertical field of view (radians). */
export const FOV_Y = (35 * Math.PI) / 180;
export const PITCH_LIMIT = (89 * Math.PI) / 180;
/** Starting direction: from the south-east, 30 degrees up, a common view for buildings. */
export const HOME_YAW = (-55 * Math.PI) / 180;
export const HOME_PITCH = (30 * Math.PI) / 180;

const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const crossV = (a: V3, b: V3): V3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const dot = (a: V3, b: V3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const norm = (a: V3): V3 => {
	const l = Math.hypot(a[0], a[1], a[2]) || 1;
	return [a[0] / l, a[1] / l, a[2] / l];
};

export function clampPitch(pitch: number): number {
	return Math.max(-PITCH_LIMIT, Math.min(PITCH_LIMIT, pitch));
}

/** Eye position. */
export function eye(c: Camera): V3 {
	const cp = Math.cos(c.pitch);
	return [c.target[0] + c.distance * cp * Math.cos(c.yaw), c.target[1] + c.distance * cp * Math.sin(c.yaw), c.target[2] + c.distance * Math.sin(c.pitch)];
}

/** Camera axes in world space: right, up and forward (towards the target), unit length. */
export function basis(c: Camera): { right: V3; up: V3; forward: V3 } {
	const forward = norm(sub(c.target, eye(c)));
	const right = norm(crossV(forward, [0, 0, 1]));
	const up = crossV(right, forward);
	return { right, up, forward };
}

/** Distance at which a sphere of `radius` around the target fits a viewport of `aspect` (w / h). */
export function fitDistance(radius: number, aspect: number): number {
	const half = FOV_Y / 2;
	const halfX = Math.atan(Math.tan(half) * Math.max(aspect, 1e-6));
	return (Math.max(radius, 1e-9) / Math.sin(Math.min(half, halfX))) * 1.05;
}

/**
 * Distance at which a box (min and max corners, relative to the camera target's frame) fills the
 * viewport from the camera's direction without leaving it: every corner projects inside
 * `margin` of the half-width and half-height. A flat, wide model gets much closer than its
 * bounding sphere would allow.
 */
export function fitBoxDistance(c: Camera, aspect: number, min: V3, max: V3, margin = 0.86): number {
	const radius = Math.max(Math.hypot(max[0] - min[0], max[1] - min[1], max[2] - min[2]) / 2, 1e-9);
	const corners: V3[] = [];
	for (const x of [min[0], max[0]]) for (const y of [min[1], max[1]]) for (const z of [min[2], max[2]]) corners.push([x, y, z]);
	const fits = (distance: number) => {
		const m = viewProjection({ ...c, distance }, aspect, radius);
		return corners.every((p) => {
			const n = project(m, p);
			return Math.abs(n[0]) <= margin && Math.abs(n[1]) <= margin;
		});
	};
	// Twice the sphere distance always holds the box; search inwards from there.
	let hi = fitDistance(radius, aspect) * 2;
	if (!fits(hi)) return hi;
	let lo = radius * 0.05;
	for (let i = 0; i < 24; i++) {
		const mid = (lo + hi) / 2;
		if (fits(mid)) hi = mid;
		else lo = mid;
	}
	return hi;
}

/** View-projection matrix for a viewport of `aspect`; near and far follow the scene radius. */
export function viewProjection(c: Camera, aspect: number, radius: number): Float32Array {
	const { right, up, forward } = basis(c);
	const e = eye(c);
	const near = Math.max(c.distance - radius * 2, c.distance * 0.01, 1e-6);
	const far = c.distance + radius * 4 + 1e-6;
	const f = 1 / Math.tan(FOV_Y / 2);
	const a = f / Math.max(aspect, 1e-6);
	const zz = (far + near) / (near - far);
	const zw = (2 * far * near) / (near - far);
	// projection * view, written out (view rows: right, up, -forward; translation -R * eye).
	const tx = -dot(right, e);
	const ty = -dot(up, e);
	const tz = dot(forward, e);
	const m = new Float32Array(16);
	m[0] = a * right[0];
	m[1] = f * up[0];
	m[2] = zz * -forward[0];
	m[3] = forward[0];
	m[4] = a * right[1];
	m[5] = f * up[1];
	m[6] = zz * -forward[1];
	m[7] = forward[1];
	m[8] = a * right[2];
	m[9] = f * up[2];
	m[10] = zz * -forward[2];
	m[11] = forward[2];
	m[12] = a * tx;
	m[13] = f * ty;
	m[14] = zz * tz + zw;
	m[15] = -tz;
	return m;
}

/** Applies a view-projection matrix to a point; returns normalised device coordinates. */
export function project(m: Float32Array, p: V3): V3 {
	const x = m[0]! * p[0] + m[4]! * p[1] + m[8]! * p[2] + m[12]!;
	const y = m[1]! * p[0] + m[5]! * p[1] + m[9]! * p[2] + m[13]!;
	const z = m[2]! * p[0] + m[6]! * p[1] + m[10]! * p[2] + m[14]!;
	const w = m[3]! * p[0] + m[7]! * p[1] + m[11]! * p[2] + m[15]!;
	return [x / w, y / w, z / w];
}

/** Ray through a viewport point given in normalised device coordinates (-1..1, Y up). */
export function screenRay(c: Camera, aspect: number, ndcX: number, ndcY: number): { origin: V3; dir: V3 } {
	const { right, up, forward } = basis(c);
	const t = Math.tan(FOV_Y / 2);
	const dx = ndcX * t * aspect;
	const dy = ndcY * t;
	return {
		origin: eye(c),
		dir: norm([forward[0] + right[0] * dx + up[0] * dy, forward[1] + right[1] * dx + up[1] * dy, forward[2] + right[2] * dx + up[2] * dy]),
	};
}

/** Turns the camera around its target by pixel deltas. */
export function orbit(c: Camera, dxPx: number, dyPx: number): Camera {
	return { ...c, yaw: c.yaw - dxPx * 0.008, pitch: clampPitch(c.pitch + dyPx * 0.008) };
}

/** Moves the target in the view plane by pixel deltas (content follows the pointer). */
export function panCamera(c: Camera, dxPx: number, dyPx: number, viewportHeightPx: number): Camera {
	const { right, up } = basis(c);
	const k = (2 * c.distance * Math.tan(FOV_Y / 2)) / Math.max(1, viewportHeightPx);
	return {
		...c,
		target: [c.target[0] - right[0] * dxPx * k + up[0] * dyPx * k, c.target[1] - right[1] * dxPx * k + up[1] * dyPx * k, c.target[2] - right[2] * dxPx * k + up[2] * dyPx * k],
	};
}

/** Scales the distance (factor > 1 moves closer), within sensible bounds for a scene radius. */
export function dolly(c: Camera, factor: number, radius: number): Camera {
	return { ...c, distance: Math.max(radius * 0.02, Math.min(radius * 60, c.distance / factor)) };
}

/** A "nice" grid step (1, 2 or 5 times a power of ten) giving about `target` steps over `span`. */
export function gridStep(span: number, target = 10): number {
	if (!(span > 0) || !Number.isFinite(span)) return 1;
	const raw = span / target;
	const pow = 10 ** Math.floor(Math.log10(raw));
	const n = raw / pow;
	return (n < 1.5 ? 1 : n < 3.5 ? 2 : n < 7.5 ? 5 : 10) * pow;
}
