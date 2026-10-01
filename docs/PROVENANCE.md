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

## CityGML and DGN writing additions

- [OGC CityGML 2.0, OGC 12-019](https://docs.ogc.org/is/12-019/12-019.pdf)
  and [official OGC schemas](https://schemas.opengis.net/citygml/2.0/): namespaces,
  feature/property ordering, geometry, generic attributes and XSD validation.
  Schema license/copyright notices remain in the downloaded files; they are not
  redistributed. OGC's `citygml/xAL/xAL.xsd` supplies the catalog mapping for the
  legacy OASIS import. Cache manifest records source URLs and SHA-256 digests.
- [quick-xml 0.41.0](https://github.com/tafia/quick-xml/tree/v0.41.0), MIT:
  bounded XML reading/writing dependency, Rust 1.79 minimum, compatible with the
  workspace's Rust 1.85. The 0.42 series requires Rust 1.86 and is excluded.
- [rust-cfb / cfb 0.14](https://github.com/mdsteele/rust-cfb), MIT:
  pure Rust CFB writing and independent strict-container validation. Rust 1.74
  minimum; WASM compilation was checked. No vendor SDK is used.
- DGN V8 record encoders follow cadkit's existing attributed native decoders and
  own public/private corpus analysis. The public GDAL V8 sample and its checked-in
  reference CSV are test data; no ODA SDK or V8 driver implementation was used.
- `cadkit-core`'s existing serde_json workspace dependency is now used for a
  bounded streaming writer preflight; no new serialization format is introduced.
- The default polygon planarity tolerance (0.01 coordinate units) matches the
  documented default distance-to-plane tolerance of the
  [val3dity](https://github.com/tudelft3d/val3dity) validator. Only that public
  parameter value was used; no val3dity code was read or ported. The planarity
  measurements behind it come from own analysis of private CityGML files.

Private source files and derived drawing data are not copied into the repository,
examples, schema cache or acceptance kit. The kit uses public and synthetic data.
External vendor application and receiving-system acceptance remain untested.
