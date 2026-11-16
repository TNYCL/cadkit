---
title: "Provenance and clean-room development"
description: "How cadkit is built clean-room: the public specifications, MIT-licensed projects and sample files it learns formats from, and the sources it never uses."
---

cadkit reads and writes proprietary CAD formats without any vendor SDK. Every piece of format
knowledge in it comes from a source that may be used for an MIT OR Apache-2.0 project, and
every source is recorded in a public ledger. This page summarises the rules; the
[provenance ledger](/docs/project/provenance/) lists each source, its licence and what it was
used for.

## Allowed sources

- **Public specifications:** the *Open Design Specification for .dwg files* (v5.4.1, a public
  document, not redistributed), the publicly documented DXF group-code reference, Microsoft's
  open specifications for compound files and property sets (`[MS-CFB]`, `[MS-OLEPS]`), the
  OGC CityGML 2.0 and GML 3.1.1 schemas, and published algorithms.
- **MIT-licensed projects:** [ACadSharp](https://github.com/DomCR/ACadSharp) (DWG and DXF
  cross-checks and samples), [ezdgn](https://github.com/monozukuri-ai/ezdgn) (DGN V8 notes),
  [GDAL](https://github.com/OSGeo/gdal)'s DGN driver (DGN V7 structure, samples, a V8 reference
  table) and [ezdxf](https://github.com/mozman/ezdxf) (a development-time validator for written
  DXF, not a runtime dependency). Ported logic is attributed to the exact source file, for
  example ACadSharp's R2007 decompressor and GDAL's default DGN palette.
- **Public sample files** and the maintainers' own hex analysis of sample files.

## Sources that are never used

- Code under the GPL, LGPL or AGPL from any CAD project: it is not read, searched, paraphrased
  or ported.
- Vendor SDKs, their headers, binaries and generated wrappers.
- Anything decompiled or disassembled.

## Test data

Public samples from open-source projects are fetched by a script for local tests and are not
committed. Drawings received from clients may be used on the maintainer's machine for
structural tests only (record counts, "parses without error"); their names, paths, strings and
coordinates never appear in code, tests, documentation or reports. Samples on this site are
listed with their licences on the [demo samples page](/demo/samples/).

## Format names

"DWG", "DGN" and "DXF" name file formats. cadkit is not affiliated with any CAD vendor, and no
vendor has endorsed it; see [trademarks](/trademarks/).

## Sources

- [Provenance ledger](/docs/project/provenance/) (synced from [`docs/PROVENANCE.md`](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md))
- Contributor rules: [`AGENTS.md`](https://github.com/TNYCL/cadkit/blob/main/AGENTS.md) and [Contributing](/docs/project/contributing/)
