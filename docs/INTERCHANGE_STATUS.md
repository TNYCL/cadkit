# DGN V8 and CityGML implementation status

Latest DGN record: [2026-11-15](#dgn-reader-structure-and-seed-follow-up-2026-11-15). No
external application acceptance is claimed; the protocol for it is
[docs/dgn/ACCEPTANCE.md](dgn/ACCEPTANCE.md).

Local validation record: 2026-10-01. Automatic TKGM building-model generation from
generic CAD geometry is out of scope for cadkit; host applications supply domain
semantics explicitly. An optional local TKGM preflight requires an explicit
architectural city-model tender or Digital Building registration profile. Both
check documented subsets and neither establishes official acceptance. Tender-year
conditions remain unchecked.

Latest coverage follow-up: all 112 supplied DGN files now rewrite/read back with
the compared geometry, attributes and layers preserved. All 56 CityGML-to-DGN
projections also pass again. This includes unchanged seed-raster retention, not
creation or modification of arbitrary raster attachments. See the coverage
follow-up below and [direct library integration](CADKIT_INTEGRATION.md). 0.3.1
corrects raster frame placement and adds new attachments cloned from a seed
attachment, with a built-in fallback template added in 0.3.3; see the
[raster follow-up](#raster-attachment-follow-up-031).

The subsequent generic GML follow-up adds closed-curve handling, exact circles,
and explicit conversion diagnostics. All 112 DGN files produced XSD-valid generic
GML with metadata-only handling; 70 files contain 576 metadata-only entities
(458 images, 108 texts, 10 unknown/tag records). Supported geometry and source
entity metadata compared equal in every file. Strict mode completes 42/112 and
rejects unsupported annotations in the remaining files. Those two results are
different contracts. See the ignored aggregate report in
`out/export-readiness/REPORT.md` and [TKGM profile notes](gml/TKGM.md).

Final local checks: **225 Rust tests** on both the default toolchain and Rust
1.85, **33 Python tests**, and **54 WASM/Node tests** passed, with zero failures.
Clippy (`--all-targets -D warnings`), rustfmt, wasm32 compilation and release
builds passed. The synthetic GML tests required offline validation against the
official XSD cache. The current optional tender preflight read all 56 native GML
files: no definite local-rule errors, 34 without findings, and 22 with 24
`review` findings for storeys with zero independent sections and no groupMember.
Those storeys have 90 common-area Room backreferences in aggregate. No ownership
links were invented or source files changed. This review condition and unchecked
contract-specific/online rules prevent a claim of universal TKGM acceptance.

## DGN reader, structure and seed follow-up (2026-11-15)

What changed (details in the CHANGELOG "Unreleased" section and
[DGN format notes](dgn/FORMAT_NOTES.md)):

- Reader corrections for issues #24–#30: flat ellipses read as lines or points, hidden V8
  graphics (`0x0080`) read as not visible, a complex header closes an unfinished chain,
  byte-order-marked UTF-16 names, type 11 curve end tangents, 2D tag angles, the tag
  element's own color and weight (`Attribute::color` / `lineweight`).
- `check_structure` / `cadkit check-dgn`: a structural report of the compound file directory
  and the pages of any V8 file, with stable warning codes.
- Writer fidelity: unused directory slots in the MS-CFB free form, the model header page
  counter set to the pages holding records, empty pages as a bare header, level-sentinel
  symbology written back as read, palette and font table helpers for hosts, a font name
  stored twice is an error unless `dgn.font_number` decides, non-integer `dgn.style` is an
  error. Every written file's bytes change.
- Level colours from 240-byte level entries and range-derived V8 text lengths (reader);
  `ByLayer` entities written with the layer's `dgn.color_index`.
- `strip_seed_v8` with CLI, Python, WASM, C and C++ bindings; no built-in blank seed can be
  built under the current provenance rules ("Built-in blank seed" in the format notes).
- An acceptance kit generator (`crates/cadkit/examples/interchange.rs`) and the protocol
  [docs/dgn/ACCEPTANCE.md](dgn/ACCEPTANCE.md).

What the writer takes from the seed now: every stream it does not write is copied with its
bytes, CLSID, state bits and times (unknown streams included, uninterpreted); unchanged
dictionary pages keep their bytes; the file header keeps every field but the id high-water
mark (`0x128`); the model header keeps every field but the extents and the first dword of
its stream prefix (the page counter, which 0.5.2 copied). The directory is not copied: the
writer rebuilds the slot order, the sibling trees and the unused slots.

Local evidence, reproduced on 2026-11-15 on Linux with the default stable toolchain
(Rust 1.99.0), the public corpus fetched by `scripts/fetch-corpus.py` and the private
corpus present (its tests assert structural facts only):

| Command (through `python3 scripts/buildlock.py`) | Result |
|---|---|
| `cargo test -p cadkit-core` | 108 passed, 0 failed |
| `cargo test -p cadkit-dgn` | 185 passed, 0 failed (includes `tests/private_corpus.rs`, `private_dictionary.rs`, `strip_seed.rs`, `structure.rs`, `blank_seed_evidence.rs`) |
| `cargo test -p cadkit-dxf` | 67 passed, 0 failed |
| `cargo test -p cadkit` | 9 passed, 0 failed (includes `tests/acceptance_kit.rs`) |
| `cargo test -p cadkit-cli` | 22 passed, 0 failed |
| `cargo test -p cadkit-capi` | 19 passed, 0 failed |
| `cargo run -p cadkit --example interchange -- corpus/public/gdal/test_dgnv8.dgn DIR` | 18 files: 14 DGN, the TIFF, the GML, the manifest pair; every DGN read back equal, `check_structure` clean, strict `cfb` open |

The structural counts behind the changes (private corpus: 567 V8 files strip, 552 of 567
fit the page counter relation, 52 of 54 level-sentinel files now write back, 303 of 303
stored text lengths equal the range width) are recorded with their tests in the format
notes. The public seed `test_dgnv8.dgn` reports `dgn.cfb_free_entry` (3 unused slots not in
the free form); its repack and every kit file report no warning.

Not tested externally: no output of this work (written files, repacks, stripped seeds, the
acceptance kit) has been opened, edited or saved in MicroStation, CityMax or any other DGN
application. The questions such a pass can settle are listed in
[ACCEPTANCE.md](dgn/ACCEPTANCE.md#7-open-questions-this-pass-can-settle). The kit uses a 3D
seed only; 2D-seed paths (2D tag angles, issue #29) have synthetic tests and no external
check. Python and WASM runtime tests, wasm32 builds and the MSRV toolchain for the
bindings were not re-run in this follow-up.

## Implemented contracts

| Surface | Implemented behavior |
|---|---|
| DGN V8 | Seed-based new geometry export, named levels, tag sets and attached tags; separate stream-preserving CFB repacking; structural report (`check_structure`); seed stripping (`strip_seed_v8`) |
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

## Earlier implementation evidence

These records describe earlier stages. The final counts and full-corpus checks
above supersede earlier sample-only results where applicable.

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
| 3D DGN → DGN (own file as seed) | 49/56 with identical histograms; 6 non-baseline-left texts and 1 raster attachment are `Unsupported` (before seed-raster retention and 0.3.1 new attachments; not re-run) |
| Plan DGN → DGN / CityGML | 0/56: every plan has raster attachments (`Unsupported` at the time; not re-run with 0.3.1) |
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

## Raster attachment follow-up (0.3.1)

| Item | Status |
|---|---|
| Raster frame placement (read) | Fixed. `0x108`/`0x110` are edge lengths along the image axes; `u = â·ext_x`, `v = b̂·ext_y`. All 2,029 frames of the 567 private DGN files are placed from their matrix inside their own range; `dgn.raster_outside_range` and `dgn.raster_range_placement` no longer occur (0.3.0: 1,635 of the 1,742 frames of the 2D files). |
| Raster pixel size, resolution, rotation, skew (read) | `size_px` from the edge lengths and pixel axes; `dgn.raster_dpi`, `dgn.raster_rotation`, `dgn.raster_skew` props. |
| Unchanged seed attachments (write) | Retained byte for byte, as before. |
| New attachments (write) | Supported. A complete attachment group of the seed (`seed_has_raster_template`) is cloned, else the built-in group (corpus-invariant bytes, bitonal display, 200 DPI); only documented fields are rewritten. Synthetic round trips (2D/3D, rotated, sheared, shared file, non-ASCII path) with both, one private seed with its own group and one private seed without any read back at the written placement with no raster warning. Display of the built-in group in MicroStation not yet confirmed. |
| Relative attachment paths (write, 0.3.4) | A `path` naming a file beside the DGN stores no machine location: a bare name writes an empty full path, `.\name` is stored as given; absolute paths keep the `file://` URL. Synthetic round trips with a template and the built-in group. MicroStation resolution of these forms from the DGN's folder not yet confirmed. |
| Edited seed attachments, mixing kept and new attachments, new attachments inside groups, relative paths into other folders | `Unsupported`. |
| Seeds with a second level table (0.3.2) | 261 of the 293 private 3D DGN files and 3 of the 274 plan files hold a second level table that repeats level codes. The writer keeps the first definition of each code, moves later ones to free codes and keeps one table header, so new elements read back on their own level (synthetic regression). |
| MicroStation / CityMax opening of new attachments | **Untested.** The cloned records keep every undecoded template field; whether MicroStation re-derives anything from them is unknown. |

## External acceptance and limitations

No external test environment is currently available. MicroStation opening,
CityMax tag interpretation/control and save/reopen acceptance remain **untested**.
Local reader agreement, a clean `check_structure` report and strict CFB validation do
not establish these claims. Vendor-written element byte identity is not claimed. The
protocol for an external pass, its results table and the open questions it can settle
are in [docs/dgn/ACCEPTANCE.md](dgn/ACCEPTANCE.md); until a pass is recorded there and
here, no external acceptance is claimed.

The DGN writer requires one seed model, matching dimensions and known units
(`V8Seed::prepare_document` promotes a 2D document to a 3D seed). cadkit ships no seed;
`strip_seed_v8` prepares one from a drawing whose tables may be handed on, and its result
keeps the names in those tables. It
supports the documented entity subset, not every neutral CAD entity. Unknown seed
streams are retained and can contain opaque references. Unsupported geometry,
unrepresentable colors/text and invalid options return errors instead of silently
discarding those entities. Arbitrary CAD tables and application metadata are not
fully mapped to DGN. V7 writing, edited seed raster attachments, and mixing
preserved and new raster attachments are unsupported; new attachments clone a seed
attachment group (`seed_has_raster_template`) or fall back to the built-in template
when the seed has none; vendor SDKs are excluded. Writing or retaining an attachment
does not embed its external image or prove that the referenced image is available to a recipient.

CityGML native preservation supports the documented CityGML 2.0 profile. It does
not imply arbitrary GML support or automatic building semantics from linework.
Generic geometry export requires an explicit CRS; no CRS/axis/Z transformation is
inferred. Geometric validation does not prove global solid self-intersection or
cavity containment. Receiving-system/TKGM acceptance remains a separate test.

## Reproduction

All build commands use the shared workspace build lock; run them one at a time. The
minimum supported Rust version is 1.88 (the 1.85 commands of the earlier records no
longer apply).

```sh
python3 scripts/fetch-corpus.py
for p in cadkit-core cadkit-dgn cadkit-gml cadkit-dxf cadkit cadkit-cli cadkit-capi; do
  python3 scripts/buildlock.py cargo test -p "$p"
  python3 scripts/buildlock.py cargo clippy -p "$p" --all-targets -- -D warnings
done
python3 scripts/buildlock.py cargo +1.88 check -p cadkit-core -p cadkit-dgn -p cadkit -p cadkit-cli
python3 scripts/buildlock.py cargo run -p cadkit --example interchange -- \
  corpus/public/gdal/test_dgnv8.dgn target/interchange-validation
python3 scripts/validate-citygml.py target/interchange-validation/synthetic.gml
```

`target/interchange-validation` is the acceptance kit: 14 small DGN V8 files written into
the public GDAL seed (each exercising one thing, listed in
[ACCEPTANCE.md](dgn/ACCEPTANCE.md#1-generate-the-kit)), the synthetic TIFF the raster file
references, a synthetic CityGML file, and `MANIFEST.json` / `README.md` with the expected
content and checks per file. Names, texts, tag values and coordinates are invented; it
contains no private drawing data and nothing derived from the Safe Software samples. The
generator fails when a file does not read back equal, reports a `check_structure` warning
or fails a strict `cfb` open.
