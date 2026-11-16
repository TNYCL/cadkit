// Which repository documents are published on the site, and where.
// Single source of truth: the files in the repository. The sync step (scripts/prebuild.d/20-docs.mjs)
// copies them into git-ignored Starlight content with frontmatter and rewritten links.

/**
 * @typedef {object} DocEntry
 * @property {string} source       Repository path of the Markdown file.
 * @property {string} route        Site URL path (trailing slash).
 * @property {string} out          Output file, relative to src/content/docs/.
 * @property {string} [description] Meta description when the first sentence of the file does not
 *                                 describe the page well (search snippets need a summary).
 * @property {{link: string, label: string}} [prev]  Pagination override (Starlight frontmatter).
 */

/** @type {DocEntry[]} */
export const DOCS = [
	{
		source: 'docs/python.md',
		route: '/docs/reference/python/',
		out: 'docs/reference/python.md',
		// The sidebar item before this page is the external docs.rs link.
		prev: { link: '/docs/guides/scene-gpu/', label: 'Scene and GPU rendering' },
		description:
			'Python API reference for pycadkit (imports as cadkit): read, inspect and convert DWG, DGN, DXF and CityGML files, with limits, exceptions and DGN seeds.',
	},
	{
		source: 'docs/wasm.md',
		route: '/docs/reference/javascript/',
		out: 'docs/reference/javascript.md',
		description:
			'JavaScript and WebAssembly API reference for cadkit-wasm: read, inspect, render and write CAD files in the browser and Node, with limits and errors.',
	},
	{
		source: 'docs/c-api.md',
		route: '/docs/reference/c-api/',
		out: 'docs/reference/c-api.md',
		description:
			'Reference for the cadkit C ABI and the header-only C++17 wrapper: functions, ownership rules, status codes, options and CMake packaging.',
	},
	{
		source: 'docs/CADKIT_INTEGRATION.md',
		route: '/docs/reference/integration/',
		out: 'docs/reference/integration.md',
		description:
			'How a host application integrates cadkit: choosing modules, scene and GPU rendering, DGN export from a seed, CityGML export, diagnostics and acceptance.',
	},
	{
		source: 'docs/ARCHITECTURE.md',
		route: '/docs/reference/architecture/',
		out: 'docs/reference/architecture.md',
		description:
			'Architecture of cadkit: the file and document stack, the geometry, scene and GPU crates, model rules, versioned JSON, errors, limits and bindings.',
	},
	{
		source: 'docs/INTERCHANGE_STATUS.md',
		route: '/docs/reference/interchange-status/',
		out: 'docs/reference/interchange-status.md',
		description:
			'Implementation status of DGN V8 and CityGML 2.0 interchange in cadkit: local test evidence, open questions and the external acceptance still pending.',
	},
	{
		source: 'docs/dwg/FORMAT_NOTES.md',
		route: '/docs/reference/formats/dwg/',
		out: 'docs/reference/formats/dwg.md',
		description:
			'How cadkit-dwg reads DWG from R14 to 2018 (R13 untested): containers, header variables, objects, mapping, spec errata and the sample evidence for each.',
	},
	{
		source: 'docs/dgn/FORMAT_NOTES.md',
		route: '/docs/reference/formats/dgn/',
		out: 'docs/reference/formats/dgn.md',
		description:
			'How cadkit-dgn reads DGN V7 and V8 and writes seed-based DGN V8: byte layouts, element mapping, the writer, open questions and the evidence for each.',
	},
	{
		source: 'docs/dgn/ACCEPTANCE.md',
		route: '/docs/reference/formats/dgn-acceptance/',
		out: 'docs/reference/formats/dgn-acceptance.md',
		description:
			'Protocol for checking DGN V8 files written by cadkit in another DGN application. No external acceptance is claimed yet; this page says how to record one.',
	},
	{
		source: 'docs/dxf/NOTES.md',
		route: '/docs/reference/formats/dxf/',
		out: 'docs/reference/formats/dxf.md',
		description:
			'How cadkit-dxf reads ASCII and binary DXF and writes ASCII DXF R12 to 2018: tokenizer, encodings, entity mapping, the writer and known gaps.',
	},
	{
		source: 'docs/gml/FORMAT_NOTES.md',
		route: '/docs/reference/formats/citygml/',
		out: 'docs/reference/formats/citygml.md',
		description:
			'The CityGML 2.0 contract of cadkit-gml: native read and write, geometry and xlink handling, the validation boundary, CAD export coverage and evidence.',
	},
	{
		source: 'docs/gml/TKGM.md',
		route: '/docs/reference/formats/tkgm/',
		out: 'docs/reference/formats/tkgm.md',
		description:
			'Notes on the local, structural TKGM preflight in cadkit-gml: the city-model tender and Digital Building profiles, checked rules and what stays unchecked.',
	},
	{
		source: 'docs/geom/NOTES.md',
		route: '/docs/reference/geometry/',
		out: 'docs/reference/geometry.md',
		description:
			'Contracts of cadkit-geom: f64 analytic geometry in caller-defined units with explicit tolerances, supported profiles, error budgets and limits.',
	},
	{
		source: 'CONTRIBUTING.md',
		route: '/docs/project/contributing/',
		out: 'docs/project/contributing.md',
		description:
			'How to contribute to cadkit: the binding rules, clean-room sources, the build lock and the checks a pull request needs before review.',
	},
	{
		source: 'docs/RELEASING.md',
		route: '/docs/project/releasing/',
		out: 'docs/project/releasing.md',
		description:
			'How cadkit releases are made: one tag builds, tests and publishes to crates.io, PyPI, npm and GitHub Releases, with checksums and attestations.',
	},
	{
		source: 'docs/PROVENANCE.md',
		route: '/docs/project/provenance/',
		out: 'docs/project/provenance.md',
		description:
			'The clean-room ledger of cadkit: every source of format knowledge, the allowed and forbidden references, and what each source was used for.',
	},
	{
		source: 'CHANGELOG.md',
		route: '/changelog/',
		out: 'changelog/index.md',
		description:
			'Release history of cadkit: notable changes to the Rust crates, CLI, Python, JavaScript and C packages in each version, with migration notes.',
	},
];

/** The document JSON schema: rendered as a page and served raw at a stable URL. */
export const SCHEMA = {
	source: 'docs/schema/document-v1.schema.json',
	route: '/docs/reference/document-schema/',
	out: 'docs/reference/document-schema.md',
	/** Raw file URL, served by src/pages/schema/document-v1.schema.json.ts. */
	raw: '/schema/document-v1.schema.json',
};

/**
 * Links to repository targets that are not synced but have a page on this site that covers the
 * same content. Key: repository path with optional "#anchor".
 */
export const ALIASES = {
	'README.md#format-support': '/formats/',
};

/** Generated directories (relative to src/content/docs/); each gets a .gitignore. */
export const GENERATED_DIRS = ['docs/reference', 'docs/project', 'changelog'];
