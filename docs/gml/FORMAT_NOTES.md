# CityGML 2.0

`cadkit-gml` reads and writes UTF-8 CityGML 2.0 (GML 3.1.1). This is an
application-schema profile, not an arbitrary GML or CityGML 3.0 implementation.
The runtime is pure Rust. It performs no network requests, schema downloads,
CRS transformations, axis swapping, or inferred vertical-datum assignment.

## Native contract

`read_native` produces `CityGmlDocument`: an expanded-namespace XML tree plus
builders for CityObject kinds, polygons and typed generic attributes. Building,
BuildingPart, Room, installations, openings, boundary surfaces, generic objects
and object groups retain their original XML structure. IDs, references, LoDs,
CRS attributes, scalar attribute types, unknown extension elements and child
order survive native read/write. This is semantic XML preservation, not original
byte preservation: declarations, entity spelling, CDATA representation and empty
tag syntax may change. Comments outside the root are not retained. Processing
instructions, DTDs and non-UTF-8 declarations are rejected explicitly.

`write` validates before serialization. `ValidationOptions.geometry` defaults
to true. Setting it false explicitly permits preserving source geometry defects;
it does not disable ID, reference, numeric or resource-limit checks. No geometry
is repaired automatically. The native JSON representation is exposed through
Python, WASM and C for creating or editing native CityGML without reducing it to
CAD geometry.

## Geometry and references

| Structure | Native preservation | Neutral geometry |
|---|---|---|
| Polygon / inline LinearRing / pos / posList | yes, including interior rings | `EntityKind::Polygon` |
| Point, LineString | yes | Point, Polyline |
| MultiSurface, CompositeSurface, Solid, CompositeSolid | yes, including ownership | constituent polygons |
| Shared geometry by local xlink | target and identity retained | expanded, within shared budgets |
| Semantic group/parent xlink | retained | not expanded into geometry |
| Curves, other GML patches | preserved as XML; no full geometric validation | explicitly unknown for unsupported geometry families |

Local references must resolve. Geometry reference cycles are rejected; semantic
parent/group cycles are not recursively expanded. Expanded work and vertices
have independent limits. No external link is fetched; external xlinks are
currently rejected. Referenced polygon rings are not implemented: polygon
boundaries must contain inline `LinearRing` elements.

`to_document` is a projection, recorded as `gml.neutral_projection`. Select a LoD
with `ImportOptions`; otherwise all LoDs appear. It cannot reconstruct original
semantic relationships after arbitrary CAD edits. Use the native document for
preserving CityGML.

`from_document` / `write_document` require an explicit CRS identifier and LoD.
They produce GenericCityObject instances, not inferred buildings or rooms. Points,
straight lines/polylines, closed polygons, faces, meshes and groups are supported.
Bulges, widths, arcs, splines, text and other unmapped kinds fail explicitly.
`cadkit.entity` generic metadata retains the source entity JSON, including styles
and nested metadata, as an annotation; it is not authoritative geometry on import.
Document-wide CAD tables/layout semantics are not CityGML semantics. There are
no TKGM/CityMax codes or generation rules in this crate.

## Validation boundary

- Structural validation checks resource budgets, XML names/characters, IDs,
  local reference targets/cycles, finite coordinates, coordinate dimensions,
  ring closure and integer/double generic values.
- Polygon checks cover degeneracy, coplanarity, crossings, hole containment,
  hole overlap and opposing interior/exterior winding.
- Coplanarity uses `ValidationOptions::planarity_tolerance`, the largest distance of
  any ring point from the exterior ring's plane (default 0.01 coordinate units, 1 cm
  in a metric projected CRS; val3dity's default distance-to-plane tolerance is also
  1 cm). Crossing checks keep the separate, tight `tolerance` (1e-6), so a larger
  planarity tolerance does not turn nearby edges into intersections. Evidence: 56
  private CityGML files write coordinates with four decimals. Their holes sit off the
  exterior plane by a median 0.04 mm, at most 3 mm. With the former shared 1e-6
  tolerance this rounding was reported as 45,012 "not coplanar" diagnostics, and strict
  writing rejected every file; with 0.01 none remain. A geographic (degree) CRS needs
  an explicit value, since planarity in mixed degree/metre coordinates is not meaningful.
- Solid checks follow shared polygon references and OrientableSurface reversal,
  require paired opposing edges and nonzero signed volume with exterior/interior
  orientation. Edge matching uses exact coordinate equality (signed zero is
  normalized); differently subdivided or almost-equal edges require explicit
  repair before validation.
- These checks do not prove a solid has no intersecting faces, that separate
  cavities are contained, or that buildings satisfy an external delivery profile.
- `geometry_issues` collects offset/code/message diagnostics separately from
  structural counters. It does not include source IDs or coordinates.
- XSD validation is independent. `scripts/validate-citygml.py --fetch` retrieves
  official public OGC schemas into the ignored public corpus, records URLs and
  hashes, and builds a local catalog (including OGC's xAL copy). Later validation
  runs `xmllint --nonet`; the Rust reader never invokes it or downloads schemas.

Every configured limit is applied in addition to a hard XML nesting ceiling of
256. Writer preflight walks the native/neutral tree without cloning it. Output
bytes are bounded during serialization. A shared geometry-work budget bounds
quadratic intersection tests.

The `gml` fuzz target exercises native parsing, reference expansion and writing.
Short coverage-instrumented AddressSanitizer runs are part of local verification;
these are smoke tests, not a claim of exhaustive malformed-input coverage.

## Evidence and reproduction

Synthetic tests cover holes, shared surfaces, Unicode, XML escaping and
attribute whitespace, namespace prefixes, malformed/truncated input, cycles,
strict rejection of noncoplanar holes, cube shell closure, native and generic
XSD validation. Schema tests run when the public schema cache exists.

A private sample can be checked in memory using `CADKIT_PRIVATE_GML` and
`private_roundtrip_when_supplied`. The test checks structure, stable native
serialization and unchanged diagnostic categories without writing the input
or output to disk. A structurally valid source can contain geometric defects;
structural preservation and strict geometry acceptance are separate assertions.

```sh
python3 scripts/validate-citygml.py --fetch
python3 scripts/buildlock.py cargo test -p cadkit-gml
cadkit validate-gml input.gml
cadkit validate-gml input.gml --planarity-tolerance 0.001
cadkit convert input.gml output.gml
cadkit convert input.gml preserved.gml --preserve-invalid-geometry
cadkit convert geometry.dxf output.gml --crs 'YOUR_EXPLICIT_CRS' --lod 1
```

Schema validity and local geometry checks do not constitute receiving-system
acceptance. No external receiving-system test has been performed.
