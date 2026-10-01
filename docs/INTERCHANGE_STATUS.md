# DGN V8 and CityGML implementation status

Local validation record: 2026-10-01. This change is confined to cadkit. No Cady
integration or application-specific TKGM/CityMax generation rules are included.

Latest coverage follow-up: all 112 supplied DGN files now rewrite/read back with
the compared geometry, attributes and layers preserved. All 56 CityGML-to-DGN
projections also pass again. This includes unchanged seed-raster retention, not
creation or modification of arbitrary raster attachments. See the coverage
follow-up below and [direct library integration](CADKIT_INTEGRATION.md).

## Implemented contracts

| Surface | Implemented behavior |
|---|---|
| DGN V8 | Seed-based new geometry export, named levels, tag sets and attached tags; separate stream-preserving CFB repacking |
| CityGML 2.0 | Native read/write preserving XML objects and relationships; typed object/polygon builders; structural and geometric diagnostics |
| Neutral geometry | CityGML projection with LoD selection; new GenericCityObject export with an explicit CRS |
| Public interfaces | Rust facade, CLI, Python, WASM, C ABI and C++ convenience methods |
| Supporting tools | Official XSD catalog/validator, fuzz targets, reproducible public/synthetic acceptance kit |

The neutral model now includes `Polygon { exterior, interiors }` as well as
`Format::CityGml`. A format enum alone could not represent the source's surface
holes. SVG renders these with even-odd fill; modern DXF writes HATCH boundaries,
and DXF R12 writes boundary outlines. CityGML ownership, IDs and shared references
remain in the native model rather than being reconstructed from edited CAD data.

See [CityGML format notes](gml/FORMAT_NOTES.md) and
[DGN writer notes](dgn/FORMAT_NOTES.md#seed-based-v8-writer) for exact coverage.

## Evidence

- Rust 1.85 tests passed for core, facade, DGN, GML, DXF, CLI and C API. Corpus
  tests whose optional input was absent return without testing that corpus; the
  explicitly exercised public DGN sample was `gdal/test_dgnv8.dgn`, including
  its independent GDAL CSV reader oracle.
- Four private DGN seeds passed in-memory repacking, new geometry export,
  deterministic output, strict CFB opening and cadkit readback. Private file
  names, geometry and tag data were not stored in fixtures or acceptance files.
- The private CityGML source and rewritten output both passed independent
  official XSD validation. Full native trees matched, excluding source byte
  offsets. Counts remained 5,580 IDs, 816 references, 2,336 polygons, 596 interior
  rings, 15,092 positions and 815 generic attributes.
- Under the former shared 1e-6 tolerance, that source showed **363 polygon
  geometry diagnostics**: interior rings outside their exterior ring's plane. The
  Windows validation below shows this pattern is four-decimal coordinate rounding,
  not a defect, and the default planarity tolerance is now 0.01 coordinate units.
  That source has not been re-checked under the new default. Strict writing still
  rejects real defects; `geometry=false` / `--preserve-invalid-geometry` preserves
  them without certifying or repairing them.
- Synthetic generic geometry, native Building/shared surfaces and a closed cube
  passed independent CityGML XSD tests. Schema and geometry tests are separate.
- Python runtime tests: **21 passed**. Actual WASM/Node runtime tests: **7 passed,
  0 skipped**. WASM was built with Rust 1.85. C11 and C++17 headers passed compiler
  syntax checks; C ABI ownership and new export calls passed runtime tests.
- Coverage-instrumented AddressSanitizer fuzz smoke runs completed without a
  crash: **5,435 GML executions** and **2,000 DGN writer executions**. These are
  bounded smoke runs, not exhaustive security or compatibility proofs.
- Final Clippy checks use the default Rust 1.98.1 toolchain, all affected packages
  and all targets with `-D warnings`. Rustfmt and whitespace checks pass. A
  separate Clippy 1.85 attempt reported existing lifetime, precedence, Boolean
  expression and formatting lints in older core/DWG/DXF/DGN code; it is not a
  passing check. Rust 1.85 compilation and tests passed independently.

## Windows validation with a private sample set (2026-10-01)

Windows 11, release CLI, default stable toolchain. The private set has 56 records,
each with one CityGML file, one plan DGN and one 3D DGN: 168 files, 372 MB. Only
structural results are recorded here. No run crashed or panicked.

Four defects were found and fixed:

1. **DGN writer on Windows.** Replacement keys came from the `cfb` crate's path string,
   which uses `\` on Windows. Every nested seed stream was copied unchanged, and new
   pages that collided with seed pages were dropped. 45 of 50 seemingly successful
   self-seed outputs were the seed with a new file header. 5 of 7 writer unit tests
   failed on Windows. Keys are now `/`-joined CFB names.
2. **Short final CFB sector.** Two 3D DGN files end inside their last sector. The
   reader skipped that graphic page, losing 751 and 784 elements. The page is now
   read, and the writer refuses seed pages it cannot decode.
3. **Planarity tolerance.** All 56 CityGML files failed strict validation with 45,012
   diagnostics caused by 0.1 mm coordinate rounding (median 0.04 mm, maximum 3 mm).
   Planarity now has its own tolerance, `planarity_tolerance` / `--planarity-tolerance`,
   with a default of 0.01. Intersection checks keep 1e-6.
4. **Integer tags above 32 bits.** 419 generic integer values in 37 files exceeded
   the DGN integer tag range. Such tags are now written as doubles, exact up to 2^53.

Results after the fixes:

| Path | Result |
|---|---|
| Read DGN / CityGML | 112/112 and 56/56 |
| DGN repack | 112/112, entity histograms identical |
| CityGML validate, strict rewrite | 56/56 clean; structure counters unchanged |
| CityGML → DGN, into each record's plan and 3D DGN as seed | 112/112. Group, shape and hole counts and all attribute values match. 511,753 rings match in order with a maximum coordinate deviation of 4.7e-10. Readback has no warnings. |
| 3D DGN → DGN (own file as seed) | 49/56 with identical histograms; 6 non-baseline-left texts and 1 raster attachment are `Unsupported` |
| Plan DGN → DGN / CityGML | 0/56: every plan has raster attachments (`Unsupported`) |
| 3D DGN → CityGML | 26/56. Failures: 13 text, 11 self-intersecting rings, 5 degenerate exteriors and 1 raster. Every output passes `validate-gml`. |
| DGN / CityGML → DXF, SVG | 168/168 |

The DGN files' summary property set declares code page 1200. That is the UTF-16
encoding of the property set's own strings, not the encoding of 8-bit element text,
so the reader falls back to windows-1252. Their 8-bit tag text is Turkish, so these
files need `--codepage windows-1254`: the default decoding garbles 557 tag values in
100 files. XSD validation was not run on this machine because `xmllint` is not
installed.

## Export regression follow-up (2026-10-01)

The follow-up work applies the earlier Windows/CFB/planarity/wide-integer fixes to
the current working tree and fixes two additional DGN correctness defects:

- Conjugate stored 3D quaternions. Synthetic text at +30 degrees now reads back at
  +30 degrees; rotated ellipse axes and tilted normals also retain their direction.
  Regression coverage exercises every matrix-to-quaternion branch and explicitly
  compares text angles, which the older text test omitted.
- Share deterministic neutral level names between reader and writer. Duplicate
  raw names, unnamed levels and literal names resembling generated aliases retain
  distinct names and their existing seed IDs. Level membership and table counts
  are checked with fully synthetic records.

Before these corrections, the added/strengthened checks produced five test
failures. Afterward, 202 tests pass on both the default Windows toolchain and Rust
1.85 for core, DGN, GML, facade, CLI and C API. Clippy (`--all-targets -D warnings`)
and workspace rustfmt checks pass. No new Rust dependency was added.

An in-memory C ABI retest, with explicit Windows-1254 fallback for legacy Turkish
element text, reads 112/112 supplied DGN files. The 49 supported direct rewrites
now preserve compared geometry, attached values, entity counts, entity-layer
membership and layer names/IDs in all 49 files. The remaining 57 raster-bearing
and 6 unsupported-text-alignment inputs still return `Unsupported`; no objects
were filtered to improve those counts.

All 56 supplied CityGML files also pass strict native rewrite, semantic XML
comparison, stable serialization and independent official XSD validation of both
input and output. Conversion of all 56 to DGN with a compatible 3D seed preserves
the compared primitive geometry and all 93,228 attached attribute values. This
neutral-model projection does not preserve CityGML object identity or hierarchy
in DGN; matching geometry and tags is not full CityGML equivalence.

The dedicated `citygml-schema` CI job installs `xmllint`, fetches official schemas
and requires their use with `CADKIT_REQUIRE_GML_XSD=1`. The missing-cache failure
was reproduced locally. Fresh synthetic point, line and polygon-with-hole output
also passed an independent local libxml2/lxml XSD check. The DGN writer and GML
targets now participate in the existing CI fuzz smoke loop. These workflow edits
have not been pushed or executed on hosted CI as part of this work.

## Export coverage follow-up (2026-10-01)

The writer now supports all 15 documented text justification codes, using the
measured baseline advance for center/right anchors. Zero-width bulged polylines
become exact line/arc complex groups, including clockwise 2D arcs. Existing seed
rasters can be retained unchanged with their original frame/control IDs and
auxiliary payloads. New or edited attachments remain unsupported.

Removing the raster rejection exposed two additional issues: ten standalone
type-37 tags in eight files were unsupported, and four attributes in one file
had their unnamed set replaced with `CADKIT`. Standalone tags now have no invented
owner, and unnamed sets remain distinct from every named set.

The final in-memory retest uses explicit Windows-1254 for legacy strings:

| Check | Result |
|---|---|
| DGN read, write and readback | 112/112 |
| Compared primitive geometry and attached attribute values/set names | 112/112 |
| Entity counts, entity-layer membership and layer names/IDs | 112/112 |
| Independent strict CFB opening | 112/112 |
| Raster frame bytes, control record bytes and graphic auxiliary payloads | Equal in all 57 raster-bearing files, containing 458 frames |
| CityGML to DGN geometry/attribute comparison | 56/56, all 93,228 attribute values preserved |

208 tests pass on both the default Windows toolchain and Rust 1.85. Clippy for the
six affected packages, all targets with warnings denied, and workspace rustfmt
pass. Native CityGML code was unchanged in this follow-up; its previous 56-file
strict/XSD checks were not re-run here. DGN-to-CityGML was not re-tested in this
follow-up and its limitations still apply.

`cadkit-dgn`'s `raster_structure` example can verify structural preservation in
memory with `CADKIT_RASTER_CORPUS` and `CADKIT_RASTER_VERIFY=1`. It prints only
aggregate structural results and never writes source or derived drawing files.
No Cady source files, branches or commits were created or changed by this work.

## External acceptance and limitations

No external test environment is currently available. MicroStation opening,
CityMax tag interpretation/control and save/reopen acceptance remain **untested**.
Local reader agreement and strict CFB validation do not establish these claims.
Vendor-written element byte identity is not claimed.

The DGN writer requires one seed model, matching dimensions and known units. It
supports the documented entity subset, not every neutral CAD entity. Unknown seed
streams are retained and can contain opaque references. Unsupported geometry,
unrepresentable colors/text and invalid options return errors instead of silently
discarding those entities. Arbitrary CAD tables and application metadata are not
fully mapped to DGN. V7 writing and new/edited raster attachments are unsupported;
vendor SDKs are excluded. Retaining an unchanged attachment does not embed its
external image or prove that the referenced image is available to a recipient.

CityGML native preservation supports the documented CityGML 2.0 profile. It does
not imply arbitrary GML support or automatic building semantics from linework.
Generic geometry export requires an explicit CRS; no CRS/axis/Z transformation is
inferred. Geometric validation does not prove global solid self-intersection or
cavity containment. Receiving-system/TKGM acceptance remains a separate test.

## Reproduction

All build commands use the shared workspace build lock.

```sh
python3 scripts/buildlock.py cargo +1.85.0 test --locked \
  -p cadkit-core -p cadkit -p cadkit-dgn -p cadkit-gml \
  -p cadkit-dxf -p cadkit-cli -p cadkit-capi
python3 scripts/buildlock.py cargo clippy --locked \
  -p cadkit-core -p cadkit -p cadkit-dgn -p cadkit-gml \
  -p cadkit-dxf -p cadkit-cli -p cadkit-capi -p cadkit-py -p cadkit-wasm \
  --all-targets -- -D warnings
python3 scripts/buildlock.py cargo run -p cadkit --example interchange -- \
  corpus/public/gdal/test_dgnv8.dgn target/interchange-validation
python3 scripts/validate-citygml.py target/interchange-validation/synthetic.gml
```

`target/interchange-validation` contains a repacked public DGN, new synthetic
geometry, moved geometry with Turkish tags and a synthetic CityGML file. It
contains no private drawing data. The three DGN files are ready for a future
external opening, geometry/tag inspection and save/reopen test.
