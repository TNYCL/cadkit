// WebGL2 renderer of the 3D view: lit surfaces, their outlines and a ground grid, drawn from
// the typed arrays the worker builds (src/demo/mesh.ts) and looked at through the orbit camera
// (src/demo/camera.ts). No library: one small shader, three vertex arrays.

import * as K from './camera.ts';
import { parseHex, SHEET_BG, type Sheet } from './colors.ts';
import { pickFace } from './mesh.ts';
import type { MeshResult } from './protocol.ts';

const VERTEX = `#version 300 es
layout(location = 0) in vec3 aPos;
layout(location = 1) in vec3 aNormal;
layout(location = 2) in vec3 aColor;
uniform mat4 uViewProj;
out vec3 vPos;
out vec3 vNormal;
out vec3 vColor;
void main() {
	vPos = aPos;
	vNormal = aNormal;
	vColor = aColor;
	gl_Position = uViewProj * vec4(aPos, 1.0);
}`;

// uMode 0: lit surface. 1: flat colour (outlines, grid). 2: lit surface tinted with uFlat (selection).
const FRAGMENT = `#version 300 es
precision highp float;
in vec3 vPos;
in vec3 vNormal;
in vec3 vColor;
uniform vec3 uEye;
uniform int uMode;
uniform vec4 uFlat;
out vec4 outColor;
void main() {
	if (uMode == 1) {
		outColor = uFlat;
		return;
	}
	vec3 n = normalize(vNormal);
	// Faces have no reliable winding (files differ), so light the side that is looked at.
	if (dot(n, uEye - vPos) < 0.0) n = -n;
	float sun = max(dot(n, normalize(vec3(-0.45, -0.6, 0.66))), 0.0);
	float sky = 0.5 + 0.5 * n.z;
	vec3 base = pow(vColor, vec3(2.2));
	vec3 lit = base * (0.36 + 0.24 * sky + 0.55 * sun);
	if (uMode == 2) lit = mix(lit, pow(uFlat.rgb, vec3(2.2)), uFlat.a);
	outColor = vec4(pow(lit, vec3(1.0 / 2.2)), 1.0);
}`;

interface Buffers {
	faces: WebGLVertexArrayObject;
	lines: WebGLVertexArrayObject;
	grid: WebGLVertexArrayObject;
	gridCount: number;
	pick: WebGLVertexArrayObject;
	pickBuffer: WebGLBuffer;
	pickCount: number;
	all: WebGLBuffer[];
}

const rgb01 = (hex: string): [number, number, number] => {
	const c = parseHex(hex) ?? [128, 128, 128];
	return [c[0] / 255, c[1] / 255, c[2] / 255];
};

/** Colours of the 3D view per sheet: outlines, grid, selection tint. */
const INK = {
	light: { edge: '#1c2430', grid: '#c9cdd4', pick: '#f76707' },
	dark: { edge: '#e8edf5', grid: '#3a4250', pick: '#ffa94d' },
} as const;

export class Scene3d {
	camera: K.Camera = { target: [0, 0, 0], yaw: K.HOME_YAW, pitch: K.HOME_PITCH, distance: 1 };
	private gl: WebGL2RenderingContext;
	private program: WebGLProgram | null = null;
	private uniforms: Record<'uViewProj' | 'uEye' | 'uMode' | 'uFlat', WebGLUniformLocation | null> = { uViewProj: null, uEye: null, uMode: null, uFlat: null };
	private buffers: Buffers | null = null;
	private mesh: MeshResult | null = null;
	private triLayer = new Uint16Array(0);
	private hidden = new Set<string>();
	private selected = -1;
	private sheet: Sheet = 'light';
	private radius = 1;
	private frame = 0;
	private lost = false;

	/** True when the browser can create a WebGL2 context. */
	static supported(): boolean {
		try {
			return Boolean(document.createElement('canvas').getContext('webgl2'));
		} catch {
			return false;
		}
	}

	constructor(readonly canvas: HTMLCanvasElement) {
		const gl = canvas.getContext('webgl2', { antialias: true, alpha: false, powerPreference: 'default' });
		if (!gl) throw new Error('WebGL2 is not available');
		this.gl = gl;
		canvas.addEventListener('webglcontextlost', (e) => {
			e.preventDefault();
			this.lost = true;
		});
		canvas.addEventListener('webglcontextrestored', () => {
			this.lost = false;
			this.program = null;
			this.buffers = null;
			this.setup();
			this.invalidate();
		});
		this.setup();
	}

	private setup(): void {
		const gl = this.gl;
		const compile = (type: number, source: string): WebGLShader => {
			const shader = gl.createShader(type);
			if (!shader) throw new Error('could not create a shader');
			gl.shaderSource(shader, source);
			gl.compileShader(shader);
			if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error('shader: ' + (gl.getShaderInfoLog(shader) ?? 'compile failed'));
			return shader;
		};
		const program = gl.createProgram();
		if (!program) throw new Error('could not create a program');
		gl.attachShader(program, compile(gl.VERTEX_SHADER, VERTEX));
		gl.attachShader(program, compile(gl.FRAGMENT_SHADER, FRAGMENT));
		gl.linkProgram(program);
		if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error('program: ' + (gl.getProgramInfoLog(program) ?? 'link failed'));
		this.program = program;
		for (const name of ['uViewProj', 'uEye', 'uMode', 'uFlat'] as const) this.uniforms[name] = gl.getUniformLocation(program, name);
		if (this.mesh) this.upload(this.mesh);
	}

	private upload(mesh: MeshResult): void {
		const gl = this.gl;
		this.release();
		const all: WebGLBuffer[] = [];
		const attribute = (location: number, data: ArrayBufferView | null, size: number, type: number, normalized: boolean, usage: number = gl.STATIC_DRAW): WebGLBuffer => {
			const buffer = gl.createBuffer();
			if (!buffer) throw new Error('could not create a buffer');
			all.push(buffer);
			gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
			if (data) gl.bufferData(gl.ARRAY_BUFFER, data, usage);
			gl.enableVertexAttribArray(location);
			gl.vertexAttribPointer(location, size, type, normalized, 0, 0);
			return buffer;
		};
		const vao = (): WebGLVertexArrayObject => {
			const v = gl.createVertexArray();
			if (!v) throw new Error('could not create a vertex array');
			gl.bindVertexArray(v);
			return v;
		};
		const faces = vao();
		attribute(0, mesh.positions, 3, gl.FLOAT, false);
		attribute(1, mesh.normals, 3, gl.FLOAT, false);
		attribute(2, mesh.colors, 3, gl.UNSIGNED_BYTE, true);
		const lines = vao();
		attribute(0, mesh.segments, 3, gl.FLOAT, false);
		// Ground grid under the model, on "nice" steps.
		const span = Math.max(mesh.max[0] - mesh.min[0], mesh.max[1] - mesh.min[1], 1e-9);
		const step = K.gridStep(span * 1.6);
		const pad = span * 0.3;
		const x0 = Math.floor((mesh.min[0] - pad) / step) * step;
		const x1 = Math.ceil((mesh.max[0] + pad) / step) * step;
		const y0 = Math.floor((mesh.min[1] - pad) / step) * step;
		const y1 = Math.ceil((mesh.max[1] + pad) / step) * step;
		const z = mesh.min[2];
		const g: number[] = [];
		for (let x = x0; x <= x1 + step / 2 && g.length < 6000; x += step) g.push(x, y0, z, x, y1, z);
		for (let y = y0; y <= y1 + step / 2 && g.length < 12000; y += step) g.push(x0, y, z, x1, y, z);
		const grid = vao();
		attribute(0, new Float32Array(g), 3, gl.FLOAT, false);
		// Selected face: filled when a face is selected (interleaved position, normal, colour as floats).
		const pick = vao();
		const pickBuffer = gl.createBuffer();
		if (!pickBuffer) throw new Error('could not create a buffer');
		all.push(pickBuffer);
		gl.bindBuffer(gl.ARRAY_BUFFER, pickBuffer);
		for (let i = 0; i < 3; i++) {
			gl.enableVertexAttribArray(i);
			gl.vertexAttribPointer(i, 3, gl.FLOAT, false, 36, i * 12);
		}
		gl.bindVertexArray(null);
		this.buffers = { faces, lines, grid, gridCount: g.length / 3, pick, pickBuffer, pickCount: 0, all };
		this.fillPick();
	}

	private release(): void {
		const gl = this.gl;
		const b = this.buffers;
		if (!b) return;
		for (const buffer of b.all) gl.deleteBuffer(buffer);
		for (const v of [b.faces, b.lines, b.grid, b.pick]) gl.deleteVertexArray(v);
		this.buffers = null;
	}

	private fillPick(): void {
		const gl = this.gl;
		const b = this.buffers;
		const mesh = this.mesh;
		if (!b || !mesh) return;
		const data: number[] = [];
		if (this.selected >= 0) {
			for (let t = 0; t < mesh.faceOfTri.length; t++) {
				if (mesh.faceOfTri[t] !== this.selected) continue;
				for (let v = t * 3; v < t * 3 + 3; v++) {
					const o = v * 3;
					data.push(mesh.positions[o]!, mesh.positions[o + 1]!, mesh.positions[o + 2]!, mesh.normals[o]!, mesh.normals[o + 1]!, mesh.normals[o + 2]!);
					data.push(mesh.colors[o]! / 255, mesh.colors[o + 1]! / 255, mesh.colors[o + 2]! / 255);
				}
			}
		}
		gl.bindBuffer(gl.ARRAY_BUFFER, b.pickBuffer);
		gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(data), gl.DYNAMIC_DRAW);
		b.pickCount = data.length / 9;
	}

	/** Shows a mesh and looks at it from the home direction. */
	setMesh(mesh: MeshResult): void {
		this.mesh = mesh;
		this.selected = -1;
		this.triLayer = new Uint16Array(mesh.faceOfTri.length);
		mesh.layers.forEach((l, i) => this.triLayer.fill(i, l.tri[0], l.tri[0] + l.tri[1]));
		this.radius = Math.max(Math.hypot(mesh.max[0] - mesh.min[0], mesh.max[1] - mesh.min[1], mesh.max[2] - mesh.min[2]) / 2, 1e-9);
		if (!this.lost) this.upload(mesh);
		this.home();
	}

	/** Back to the starting view: the whole model from the home direction. */
	home(): void {
		const start: K.Camera = { target: [0, 0, 0], yaw: K.HOME_YAW, pitch: K.HOME_PITCH, distance: 1 };
		const mesh = this.mesh;
		const distance = mesh ? K.fitBoxDistance(start, this.aspect(), mesh.min, mesh.max) : K.fitDistance(this.radius, this.aspect());
		this.camera = { ...start, distance };
		this.invalidate();
	}

	setSheet(sheet: Sheet): void {
		this.sheet = sheet;
		this.invalidate();
	}

	setHidden(names: Iterable<string>): void {
		this.hidden = new Set(names);
		this.invalidate();
	}

	/** Tints one face (index into mesh.faces); -1 clears. */
	setSelected(face: number): void {
		this.selected = face;
		if (!this.lost) this.fillPick();
		this.invalidate();
	}

	private aspect(): number {
		return Math.max(1, this.canvas.clientWidth) / Math.max(1, this.canvas.clientHeight);
	}

	orbit(dx: number, dy: number): void {
		this.camera = K.orbit(this.camera, dx, dy);
		this.invalidate();
	}
	pan(dx: number, dy: number): void {
		this.camera = K.panCamera(this.camera, dx, dy, this.canvas.clientHeight);
		this.invalidate();
	}
	zoom(factor: number): void {
		this.camera = K.dolly(this.camera, factor, this.radius);
		this.invalidate();
	}

	/** Face under a canvas point (CSS pixels from the canvas top-left), or -1. */
	pick(px: number, py: number): number {
		const mesh = this.mesh;
		if (!mesh) return -1;
		const w = Math.max(1, this.canvas.clientWidth);
		const h = Math.max(1, this.canvas.clientHeight);
		const ray = K.screenRay(this.camera, w / h, (px / w) * 2 - 1, 1 - (py / h) * 2);
		const hiddenLayers = new Set<number>();
		mesh.layers.forEach((l, i) => {
			if (this.hidden.has(l.name)) hiddenLayers.add(i);
		});
		return pickFace(mesh.positions, mesh.faceOfTri, ray.origin, ray.dir, (t) => !hiddenLayers.has(this.triLayer[t]!));
	}

	/** Requests a redraw on the next animation frame. */
	invalidate(): void {
		if (this.frame) return;
		this.frame = requestAnimationFrame(() => {
			this.frame = 0;
			this.draw();
		});
	}

	private draw(): void {
		const gl = this.gl;
		const b = this.buffers;
		const mesh = this.mesh;
		if (this.lost || !this.program) return;
		const dpr = Math.min(window.devicePixelRatio || 1, 2);
		const w = Math.max(1, Math.round(this.canvas.clientWidth * dpr));
		const h = Math.max(1, Math.round(this.canvas.clientHeight * dpr));
		if (this.canvas.width !== w || this.canvas.height !== h) {
			this.canvas.width = w;
			this.canvas.height = h;
		}
		gl.viewport(0, 0, w, h);
		const bg = rgb01(SHEET_BG[this.sheet]);
		gl.clearColor(bg[0], bg[1], bg[2], 1);
		gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
		if (!b || !mesh) return;
		const ink = INK[this.sheet];
		gl.useProgram(this.program);
		gl.uniformMatrix4fv(this.uniforms.uViewProj, false, K.viewProjection(this.camera, w / h, this.radius));
		gl.uniform3fv(this.uniforms.uEye, K.eye(this.camera));
		gl.enable(gl.DEPTH_TEST);
		gl.depthFunc(gl.LEQUAL);
		gl.disable(gl.CULL_FACE);
		gl.enable(gl.BLEND);
		gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

		// Grid first, without writing depth, so the model always covers it.
		gl.depthMask(false);
		gl.uniform1i(this.uniforms.uMode, 1);
		gl.uniform4f(this.uniforms.uFlat, ...rgb01(ink.grid), 1);
		gl.bindVertexArray(b.grid);
		gl.drawArrays(gl.LINES, 0, b.gridCount);
		gl.depthMask(true);

		// Surfaces, pushed back a little so their outlines and the selection win the depth test.
		gl.enable(gl.POLYGON_OFFSET_FILL);
		gl.polygonOffset(1, 1);
		gl.uniform1i(this.uniforms.uMode, 0);
		gl.bindVertexArray(b.faces);
		for (const l of mesh.layers) if (l.tri[1] && !this.hidden.has(l.name)) gl.drawArrays(gl.TRIANGLES, l.tri[0] * 3, l.tri[1] * 3);
		gl.disable(gl.POLYGON_OFFSET_FILL);

		if (b.pickCount) {
			gl.uniform1i(this.uniforms.uMode, 2);
			gl.uniform4f(this.uniforms.uFlat, ...rgb01(ink.pick), 0.6);
			gl.bindVertexArray(b.pick);
			gl.drawArrays(gl.TRIANGLES, 0, b.pickCount);
		}

		gl.uniform1i(this.uniforms.uMode, 1);
		gl.uniform4f(this.uniforms.uFlat, ...rgb01(ink.edge), 0.7);
		gl.bindVertexArray(b.lines);
		for (const l of mesh.layers) if (l.seg[1] && !this.hidden.has(l.name)) gl.drawArrays(gl.LINES, l.seg[0] * 2, l.seg[1] * 2);
		gl.bindVertexArray(null);
	}

	/** Frees the GPU resources; the canvas can be removed afterwards. */
	dispose(): void {
		if (this.frame) cancelAnimationFrame(this.frame);
		this.frame = 0;
		this.release();
		if (this.program) this.gl.deleteProgram(this.program);
		this.program = null;
		this.mesh = null;
	}
}
