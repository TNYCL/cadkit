// Docs sidebar. astro.config.mjs imports it; Starlight derives prev/next links from this order.
// Item shapes follow Starlight's sidebar config (https://starlight.astro.build/guides/sidebar/).
// Reference and project pages are synced from the repository docs by
// scripts/prebuild.d/20-docs.mjs (see scripts/docs/manifest.mjs).
// Do not import src/config.ts here: astro.config.mjs loads this file, and a second import path
// of config.ts breaks the dynamic import in scripts/postbuild.mjs (module runner closed).

import type { StarlightUserConfig } from '@astrojs/starlight/types';

export const sidebar: NonNullable<StarlightUserConfig['sidebar']> = [
	{
		label: 'Start here',
		items: [
			{ label: 'Overview', slug: 'docs' },
			{ label: 'Try the demo', link: '/demo/' },
		],
	},
	{
		label: 'Getting started',
		items: [
			{ label: 'Rust', slug: 'docs/getting-started/rust' },
			{ label: 'Python', slug: 'docs/getting-started/python' },
			{ label: 'JavaScript', slug: 'docs/getting-started/javascript' },
			{ label: 'C and C++', slug: 'docs/getting-started/c' },
			{ label: 'Command line', slug: 'docs/getting-started/cli' },
		],
	},
	{
		label: 'Guides',
		items: [
			{ label: 'Convert DXF to SVG', slug: 'docs/guides/dxf-to-svg' },
			{ label: 'Inspect a drawing and its layers', slug: 'docs/guides/inspect-drawing' },
			{ label: 'Write DGN V8 from a seed', slug: 'docs/guides/write-dgn-v8' },
			{ label: 'CityGML 2.0 and TKGM preflight', slug: 'docs/guides/citygml-tkgm' },
			{ label: 'Browser viewer with cadkit-wasm', slug: 'docs/guides/browser-viewer' },
			{ label: 'Handling untrusted files', slug: 'docs/guides/untrusted-files' },
			{ label: 'Geometry with cadkit-geom', slug: 'docs/guides/geometry' },
			{ label: 'Scene and GPU rendering', slug: 'docs/guides/scene-gpu' },
		],
	},
	{
		label: 'Reference',
		items: [
			{ label: 'Rust API (docs.rs)', link: 'https://docs.rs/cadkit', attrs: { rel: 'external' } },
			{ label: 'Python API', slug: 'docs/reference/python' },
			{ label: 'JavaScript API', slug: 'docs/reference/javascript' },
			{ label: 'C and C++ API', slug: 'docs/reference/c-api' },
			{ label: 'Document JSON schema', slug: 'docs/reference/document-schema' },
			{ label: 'Architecture', slug: 'docs/reference/architecture' },
			{ label: 'Host integration', slug: 'docs/reference/integration' },
			{ label: 'Interchange status', slug: 'docs/reference/interchange-status' },
			{ label: 'Geometry contracts', slug: 'docs/reference/geometry' },
			{
				label: 'Format notes',
				collapsed: true,
				items: [
					{ label: 'DWG', slug: 'docs/reference/formats/dwg' },
					{ label: 'DGN', slug: 'docs/reference/formats/dgn' },
					{ label: 'DGN acceptance protocol', slug: 'docs/reference/formats/dgn-acceptance' },
					{ label: 'DXF', slug: 'docs/reference/formats/dxf' },
					{ label: 'CityGML 2.0', slug: 'docs/reference/formats/citygml' },
					{ label: 'TKGM preflight', slug: 'docs/reference/formats/tkgm' },
				],
			},
		],
	},
	{
		label: 'Formats',
		items: [
			{ label: 'All formats', link: '/formats/' },
			{ label: 'DWG', link: '/formats/dwg/' },
			{ label: 'DGN', link: '/formats/dgn/' },
			{ label: 'DXF', link: '/formats/dxf/' },
			{ label: 'CityGML', link: '/formats/citygml/' },
		],
	},
	{
		label: 'Project',
		items: [
			{ label: 'Changelog', slug: 'changelog' },
			{ label: 'Clean-room statement', slug: 'provenance' },
			{ label: 'Provenance ledger', slug: 'docs/project/provenance' },
			{ label: 'Contributing', slug: 'docs/project/contributing' },
			{ label: 'Releasing', slug: 'docs/project/releasing' },
		],
	},
];
