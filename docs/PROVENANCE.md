# Provenance

cadkit is a clean-room implementation. This ledger records every source of format
knowledge. Contributors add a row when they rely on a new source.

## Allowed sources

| Source | License | Used for |
|---|---|---|
| Open Design Alliance, *Open Design Specification for .dwg files* v5.4.1 | Public document (not redistributed; fetched into `refs/`) | DWG container, bit codes, objects |
| [ACadSharp](https://github.com/DomCR/ACadSharp) | MIT | DWG/DXF cross-check, sample files |
| [ezdgn](https://github.com/monozukuri-ai/ezdgn) | MIT | DGN V8 format notes cross-check |
| [GDAL](https://github.com/OSGeo/gdal) `frmts/dgn` (dgnlib) and autotest data | MIT/X | DGN V7 structure, sample files, V8 reference CSV |
| [ezdxf](https://github.com/mozman/ezdxf) | MIT | DXF structure cross-check; dev-time validator (audit) of written DXF and reader comparison — not a runtime dependency |
| Autodesk DXF Reference (public documentation) | Public document | DXF group codes per entity |
| Own hex analysis of public and private corpus files | — | All formats |

## Excluded sources

LibreDWG, libdxfrw, LibreCAD, QCAD and any other GPL/LGPL/AGPL code; ODA SDK headers,
binaries or wrappers; decompiled vendor code; Autodesk/Bentley SDKs.

## Test corpus

Fetched by `scripts/fetch-corpus.py` into `corpus/public/` (not committed):

- ACadSharp `samples/` — one drawing saved as DWG AC1014…AC1032 plus ASCII/binary DXF.
- GDAL `autotest/ogr/data/dgn` and `dgnv8` — DGN V7 and V8 samples, V8 reference CSV;
  GDAL `ogr/ogrsf_frmts/dgn/data/seed_{2d,3d}.dgn`.
- Apache Tika `tika-parser-cad-module` test documents (Apache-2.0) — 19 DWG files
  AC1015…AC1032 for structure-only validation.
- Safe Software public support-article attachments (`TreeTextNodeLabelsWithTags.dgn`,
  `Water_distribution_mains.dgn`) — public downloads without an explicit license: fetched for
  local testing only, never redistributed.

Private client drawings in `corpus/private/` are used only for local, structural tests.

## Attributions

<!-- Add one line per ported algorithm or table: what, from where, which file/section. -->

- `cadkit-dwg`: R2007 (AC1021) LZ77 decompressor ported from ACadSharp `DwgLZ77AC21Decompressor.cs` (MIT, © DomCR); R2007 container read flow follows ACadSharp `DwgReader.cs`. Spec errata found during implementation are listed in `docs/dwg/FORMAT_NOTES.md`.
- `cadkit-dgn`: default 256-color DGN palette ported from GDAL dgnlib `abyDefaultPCT` (MIT/X) → `src/palette.rs`; V7 element layouts, TCB, tag set/tag value layouts follow GDAL dgnlib; V7 description-length rule and shared-cell offsets from ezdgn (MIT) → `src/native/v7.rs`; V8 model-header search from ezdgn → `src/native/v8.rs`. Full source table in `docs/dgn/FORMAT_NOTES.md`.
- `cadkit-core`: ACI color table from ACadSharp `Color.cs` (MIT), entries 250–254 corrected to the published AutoCAD greys.
