// llms.txt (https://llmstxt.org): a plain-language summary and a link to every page. Docs
// pages are linked as Markdown (the ".md" files of src/pages/[...doc].md.ts); the list follows
// the docs sidebar, so a page added there is listed here without further registration.
import type { APIRoute } from 'astro';
import { SITE, REPO, LINKS, CADKIT_VERSION, LICENSE } from '../config';
import { docPages, sidebarSections } from '../lib/llms';

/** What a site page (not a docs page) offers, where its sidebar label does not say it. */
const NOTES: Record<string, string> = {
	'/demo/': 'in-browser viewer; files are read locally and never uploaded',
	'/formats/': 'read and write support matrix for every format and version',
};

export const GET: APIRoute = async () => {
	const pages = new Map((await docPages()).map((p) => [p.id, p]));
	const sections = sidebarSections().map(({ label, entries }) => {
		const lines = entries.map((e) => {
			const page = e.slug ? pages.get(e.slug) : undefined;
			if (page) return `- [${page.title}](${SITE}${page.mdPath}): ${page.description}`;
			if (!e.link) throw new Error(`llms.txt: sidebar entry "${e.label}" has no page`);
			const note = NOTES[e.link];
			return `- [${e.label}](${e.link.startsWith('/') ? SITE + e.link : e.link})${note ? ': ' + note : ''}`;
		});
		return `## ${label}\n\n${lines.join('\n')}`;
	});

	const body = `# cadkit

> Open-source CAD engine in pure Rust (${LICENSE}). Reads DWG, DGN (V7 and V8), DXF and CityGML 2.0 into one format-neutral document; writes DXF, seed-based DGN V8, CityGML 2.0, SVG and JSON. Interfaces: Rust crates, a CLI, Python (pycadkit, imports as cadkit), JavaScript/WebAssembly (cadkit-wasm) and a C/C++ API. Current release: ${CADKIT_VERSION}.

Status: DWG is read-only (AC1014 R14 to AC1032 2018; R13 untested). DGN V7 read. DGN V8 read plus a seed-based V8 write subset; external application acceptance pending. DXF ASCII and binary read R12 to 2018, ASCII write R12 to 2018. CityGML 2.0 native read/write and generic geometry; new geometry needs an explicit CRS; optional local structural TKGM preflight (not institutional acceptance). Developed clean-room; no vendor SDK.

Every documentation page is also served as Markdown: replace the trailing slash of its address with ".md" (${SITE}/docs/getting-started/rust.md). The whole documentation in one file: [llms-full.txt](${SITE}/llms-full.txt).

${sections.join('\n\n')}

## Packages

- [cadkit on crates.io](${LINKS.crates}): the Rust crate; API documentation on [docs.rs](${LINKS.docsRs})
- [pycadkit on PyPI](${LINKS.pypi}): the Python package
- [cadkit-wasm on npm](${LINKS.npm}): the JavaScript and WebAssembly package
- [Releases](${LINKS.releases}): CLI and C API archives

## Source and community

- [Source repository](${REPO})
- [Community and contributing](${SITE}/community/)
- [Security policy](${SITE}/security/)
- [Project statistics](${SITE}/stats/)
`;
	return new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
