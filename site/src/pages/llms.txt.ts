// llms.txt (https://llmstxt.org): a short plain-language summary with the key links.
import type { APIRoute } from 'astro';
import { SITE, REPO, LINKS, CADKIT_VERSION, LICENSE } from '../config';

export const GET: APIRoute = () => {
	const body = `# cadkit

> Open-source CAD engine in pure Rust (${LICENSE}). Reads DWG, DGN (V7 and V8), DXF and CityGML 2.0 into one format-neutral document; writes DXF, seed-based DGN V8, CityGML 2.0, SVG and JSON. Interfaces: Rust crates, a CLI, Python (pycadkit, imports as cadkit), JavaScript/WebAssembly (cadkit-wasm) and a C/C++ API. Current release: ${CADKIT_VERSION}.

Status: DWG is read-only (AC1014 R14 to AC1032 2018; R13 untested). DGN V7 read. DGN V8 read plus a seed-based V8 write subset; external application acceptance pending. DXF ASCII and binary read R12 to 2018, ASCII write R12 to 2018. CityGML 2.0 native read/write and generic geometry; new geometry needs an explicit CRS; optional local structural TKGM preflight (not institutional acceptance). Developed clean-room; no vendor SDK.

## Docs

- [Documentation](${SITE}/docs/): getting started, guides and reference
- Getting started: [Rust](${SITE}/docs/getting-started/rust/), [Python](${SITE}/docs/getting-started/python/), [JavaScript](${SITE}/docs/getting-started/javascript/), [C and C++](${SITE}/docs/getting-started/c/), [CLI](${SITE}/docs/getting-started/cli/)
- API reference: [Rust on docs.rs](${LINKS.docsRs}), [Python](${SITE}/docs/reference/python/), [JavaScript](${SITE}/docs/reference/javascript/), [C and C++](${SITE}/docs/reference/c-api/), [document JSON schema](${SITE}/docs/reference/document-schema/)
- [Handling untrusted files](${SITE}/docs/guides/untrusted-files/)
- [Changelog](${SITE}/changelog/)

## Formats

- [Format support matrix](${SITE}/formats/)
- [DWG](${SITE}/formats/dwg/), [DGN](${SITE}/formats/dgn/), [DXF](${SITE}/formats/dxf/), [CityGML 2.0](${SITE}/formats/citygml/)
- [Interchange status and local test evidence](${SITE}/docs/reference/interchange-status/)
- [In-browser demo](${SITE}/demo/): files are read locally and not uploaded

## Packages

- [cadkit on crates.io](${LINKS.crates})
- [pycadkit on PyPI](${LINKS.pypi})
- [cadkit-wasm on npm](${LINKS.npm})
- [Releases: CLI and C API archives](${LINKS.releases})

## Project

- [Source repository](${REPO})
- [Community and contributing](${SITE}/community/), [security policy](${SITE}/security/)
- [Format status and local test evidence](${LINKS.interchangeStatus})
- [Sources of format knowledge (provenance)](${LINKS.provenanceSource})
`;
	return new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
