# DGN V8 and CityGML implementation status

Local validation record: 2026-10-01. This change is confined to cadkit. No Cady
integration or application-specific TKGM/CityMax generation rules are included.

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
- That source has **363 polygon geometry diagnostics**, specifically interior
  rings outside their exterior ring's plane. The diagnostics are unchanged by
  native preservation. Strict writing rejects these defects. An explicit
  `geometry=false` / `--preserve-invalid-geometry` option preserves them; it
  does not certify or repair them.
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
fully mapped to DGN. V7 writing, raster attachments and vendor SDKs are excluded.

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
