// Module map. Source of truth: README.md "Modules".
export interface ModuleEntry {
	crates: string[];
	title: string;
	scope: string;
}

export const MODULES: ModuleEntry[] = [
	{ crates: ['cadkit'], title: 'Open and export drawings', scope: 'Format detection, inspection, I/O and access to the neutral document model.' },
	{ crates: ['cadkit-core'], title: 'Work with drawing data', scope: 'Documents, layers, blocks, entities, transforms, units, traversal, limits, JSON and SVG.' },
	{ crates: ['cadkit-dwg', 'cadkit-dgn', 'cadkit-dxf', 'cadkit-gml'], title: 'Use a format directly', scope: 'The readers and supported writers, one crate per format.' },
	{ crates: ['cadkit-geom'], title: 'Compute geometry', scope: 'Analytic curves, polygon booleans, trim/offset, calibration and supported NURBS operations. Depends on no other cadkit crate.' },
	{ crates: ['cadkit-scene'], title: 'Prepare scene data without a GPU', scope: 'Chunked 2D plans, precision budgets, CPU handoff and f64 triangle meshes.' },
	{ crates: ['cadkit-gpu'], title: 'Draw with a GPU', scope: 'wgpu plan and mesh renderers, cameras, render targets and readback. Opens no window; the host supplies the UI.' },
	{ crates: ['cadkit-cli', 'cadkit-capi'], title: 'Integrate from another interface', scope: 'The cadkit CLI and the C ABI with a C++17 wrapper. Python (pycadkit) and JavaScript (cadkit-wasm) packages cover file I/O and documented subsets of the document operations.' },
];
