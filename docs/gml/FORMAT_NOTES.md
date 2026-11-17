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
| Curves, other GML patches | preserved as XML; no full geometric validation | single `gml:Circle` segment → `Circle`; other curves/patches unknown |

Local references must resolve. Geometry reference cycles are rejected; semantic
parent/group cycles are not recursively expanded. Expanded work and vertices
have independent limits. No external link is fetched; external xlinks are
currently rejected. Referenced polygon rings are not implemented: polygon
boundaries must contain inline `LinearRing` elements.

`to_document` is a projection, recorded as `gml.neutral_projection`. Select a LoD
with `ImportOptions`; otherwise all LoDs appear. It cannot reconstruct original
semantic relationships after arbitrary CAD edits. Use the native document for
preserving CityGML.

`from_document` / `write_document` (and `from_document_with_report` /
`write_document_with_report`) require an explicit CRS identifier and LoD.
They produce `GenericCityObject` instances, not inferred buildings or rooms. Points,
straight lines/polylines (including closed curves), exact circles (`gml:Curve` with
a three-point `gml:Circle` segment), closed polygons, faces, meshes, and groups are
supported. Bulges, widths, arcs, splines, text, images, and other unmapped kinds fail
explicitly under `UnsupportedGeometry::Reject` (the default), or can be retained
explicitly as `cadkit.entity` JSON metadata with `cadkit.geometry_status` and an
`ExportReport` under `UnsupportedGeometry::MetadataOnly`.
`cadkit.entity` generic metadata retains the source entity JSON, including styles
and nested metadata, as an annotation; it is not authoritative geometry on import.
Document-wide CAD tables and layout semantics are not CityGML semantics. Generic
CAD export never infers TKGM/CityMax semantics; separately, `cadkit::gml::tkgm::preflight`
provides optional, read-only local structural/field preflight checks on native
`CityGmlDocument` models for explicit public TKGM profiles (`CityModelTender` and
`DigitalBuildingRegistration`) without certifying official acceptance.

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
  orientation. Edge matching welds vertices within `ValidationOptions::tolerance`;
  differently subdivided edges still require explicit repair. Spatial-grid neighbor
  arithmetic is checked even when very large finite coordinates saturate an integer
  cell key; the existing geometry work budget also bounds crowded buckets.
- These checks do not prove a solid has no intersecting faces, that separate
  cavities are contained, or that buildings satisfy an external delivery profile.
- `geometry_issues` collects offset/code/message diagnostics separately from
  structural counters. It does not include source IDs or coordinates.
  `located_geometry_issues` returns the same findings in the same order with the
  element index and the `gml:id`s of the polygon or solid, of the nearest enclosing
  city object and of the enclosing `bldg:Building`. Those ids are written by the file's
  author and may encode client data, so this variant is opt-in.
- XSD validation is independent. `scripts/validate-citygml.py --fetch` retrieves
  official public OGC schemas into the ignored public corpus, records URLs and
  hashes, and builds a local catalog (including OGC's xAL copy). Later validation
  runs `xmllint --nonet`; the Rust reader never invokes it or downloads schemas.
  The dedicated `citygml-schema` CI job installs `xmllint`, fetches the official
  schemas and sets `CADKIT_REQUIRE_GML_XSD=1`. With that environment variable,
  a missing cache fails the test instead of returning early. Local tests remain
  optional when the cache and requirement are both absent.

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

## CAD export coverage and diagnostics (2026-10-01 follow-up)

- Closed neutral polylines serialize as closed `gml:LineString` curves. Closure
  alone does not establish a planar surface: crossing or nonplanar CAD paths are
  valid curves. Explicit `Polygon`, `Face` and mesh surfaces retain strict checks.
  `ExportOptions::closed_polylines_as_surfaces` (default off) opts in to surfaces; see
  the next section.
- Circles use `gml:Curve/gml:segments/gml:Circle` with three distinct, non-collinear
  positions, `numArc="1"` and `interpolation="circularArc3Points"`. This is exact
  geometry, not tessellation. The neutral importer decodes single explicit Circle
  segments; other Curve forms remain outside that subset. Circle positions are
  bounded before allocation and their circumcenter is calculated in local,
  scaled coordinates. Evidence: the public GML 3.1.1 geometryPrimitives schema,
  invented tilted/negative-normal circles, mutation tests and corpus comparison.
- `write_document_with_report` adds explicit `MetadataOnly` handling. Unsupported
  entities remain in the existing `cadkit.entity` JSON attribute. Their geometry
  is absent, the containing object gets `cadkit.geometry_status` and an embedded
  issue list, and the API returns index-based diagnostics. Groups retain the
  supported children. The ordinary API and all new bindings default to `Reject`.
- Invalid coordinates, invalid explicit polygons, invalid circles and exhausted
  limits still fail. JSON metadata is checked for non-finite-number loss before
  serialization. External image pixels and referenced block definitions are not
  embedded by this generic projection.
- `tkgm::preflight` requires an explicit architectural city-model tender or Digital
  Building registration profile. Neither subset certifies acceptance;
  see [TKGM notes](TKGM.md) for checked rules and guide ambiguities.
- Synthetic schema tests may use `CADKIT_XSD_VALIDATOR_SCRIPT` to invoke a local
  Python validator over stdin (exit code only). The default CI path remains
  `xmllint` with the official schema cache and mandatory missing-cache failure.

```sh
cadkit convert drawing.dgn geometry.gml --crs YOUR_EXPLICIT_CRS --lod 2 --gml-metadata-only
cadkit validate-gml native-building.gml --tkgm-profile city-model-tender
```

The first command produces a generic CAD projection with diagnostics, not a
TKGM building model. CLI diagnostics go to stderr; the XML keeps an embedded
record of each unsupported geometry item even if that report is separated.

## Surfaces and buildings from CAD faces (2026-11 follow-up)

3D building models delivered as DGN arrive as hundreds to thousands of closed,
planar, straight polylines (DGN shape elements; in the drawings this was designed for
about 81 % vertical, 19 % horizontal and under 1 % sloped). Written as generic
objects they are line strings, so such a model had no surface at all.

### Closed polylines as surfaces

With `ExportOptions::closed_polylines_as_surfaces` a polyline is a surface candidate
when it is flagged closed or ends on its first vertex, has no bulge and no width, and
keeps at least three distinct vertices after consecutive duplicates are dropped (a
zero-length edge is invalid GML but carries no geometry). It is written as
`gml:Polygon` only when all of these hold; otherwise the generic export keeps the
line string and reports it (`ExportReport::issues`, code below), and the building
export skips it:

| Check | Rule | Code |
|---|---|---|
| Degenerate | fewer than 3 distinct vertices, zero bounding-box diagonal, or area at most `tolerance x diagonal` (narrower than the planarity tolerance, so no reliable normal) | `gml.face_degenerate` |
| Planar | every ring point within `min(planarity_tolerance x diagonal, max_planarity_deviation)` of the plane through the first vertex with the Newell normal | `gml.face_not_planar` |
| Simple | `cadkit_core::polygon::validate` with that tolerance and the 1e-6 crossing tolerance | `gml.face_self_intersecting`, `gml.face_invalid` |
| Straight | no bulge / no width (building export only; the generic export keeps its own bulge handling) | `gml.face_curved`, `gml.face_wide` |

Defaults: `planarity_tolerance = 1e-3` of the face's bounding-box diagonal, capped
by `max_planarity_deviation = 0.01` coordinate units. The cap equals the default
`ValidationOptions::planarity_tolerance`, and the plane test uses the same reference
point and normal as validation, so every accepted face passes default validation
after a write/read round trip (`f64` text is the shortest round-trip representation).
A cap above 0.01 needs the same planarity tolerance when the file is validated;
`write_document`, `write_document_with_report` (with the option on) and `write_buildings`
validate with `max(0.01, max_planarity_deviation)`, so a face the export accepted does
not make the whole write fail.

### Buildings

`buildings_from_document(doc, model, &BuildingOptions)` and `write_buildings` turn
the surfaces of one model (`Polygon` with holes, `Face`, `Mesh` faces and, with the
option above, closed polylines) into `bldg:Building` objects. They use
`Document::walk`, so geometry inside block references and groups is placed in model
space, hidden layers and invisible entities are skipped, and the walk is bounded by
`BuildingOptions::max_work`. LoD must be 2, 3 or 4: CityGML 2.0 has thematic boundary
surfaces only from LoD 2. The CRS is an explicit identifier; nothing is reprojected.

Classification uses the tilt, the angle between the face normal and the vertical axis,
so winding does not matter:

| Tilt | Lowest point of the face | Class |
|---|---|---|
| >= `wall_min_tilt_degrees` (80) | any | `WallSurface` |
| <= `horizontal_max_tilt_degrees` (10) | within `ground_tolerance` (0.01) of the building's lowest point | `GroundSurface` |
| <= `horizontal_max_tilt_degrees` | above that band | `RoofSurface` |
| between | above that band | `RoofSurface` |
| between | within the band | unclassified, `gml.face_sloped_at_base` |

Both thresholds are inclusive: the cosines are compared with a margin of 1e-9, because
the normal of a face drawn exactly at a threshold, or of a vertical wall placed at map
coordinates through a rotated block (vertical to about 1e-11), is decided by rounding
otherwise. The margin is about 6e-8 degrees at 90 degrees and 0.003 degrees at 0.

The base is per building, so grouping changes it. A building of one horizontal face
(a roof drawn as its own top-level entity under `TopLevel`) is a ground surface. Ground faces are rewound so their
normal points down and roof faces up; walls keep the drawn winding (outward needs a
closed shell, which CAD models do not guarantee). Holes are wound against their
exterior. Unclassified faces are written into the building's own
`bldg:lodNMultiSurface` and reported, never dropped silently.

Limits of the rule: it knows geometry, not use. Interior floor slabs, soffits and the
undersides of overhangs are horizontal faces above the base and become roofs. A ramp,
a battered wall or an A-frame roof plane reaching the ground is unclassified. Faces
of DGN complex shapes (chains of separate segments) are not surfaces yet, and no
`lod2Solid` is written, since closure of the faces is not checked.

Grouping (`BuildingGrouping`): `Single` (default, all faces), `Layer` (effective
layer; layer `0` inside a block takes the inserting entity's layer), `TopLevel` (one
building per model entity: a cell, group or block reference, or a single top-level
face) and `Connected` (faces sharing a vertex within `weld_tolerance`, 0.001 by
default; a spatial hash grid with one representative per welded vertex and
union-find, linear in the number of vertices and charged to the work budget).
Vertices are welded to the first representative within the tolerance, so a chain of
vertices each just under the tolerance apart may split. Grid cells are one tolerance
wide and the 27 surrounding cells are searched, so vertices on both sides of a cell
border weld. With a tolerance of 0 the grid keys on the exact coordinate bits: a
coordinate divided by the minimum cell (1e-12) leaves the `i64` range above 9.2e6
(UTM northings, Web Mercator), and clamping put a whole axis into one cell (3,000
triangles in a row at x = 1.5e7 exceeded a 1,000,000 work budget).

Output layout: a root 3D `gml:Envelope` (`srsName` from the options,
`srsDimension="3"`), and per building an envelope, then in CityGML 2.0
`_AbstractBuilding` order the unclassified `lodNMultiSurface` and the `bldg:boundedBy`
surfaces (for LoD 3/4 `boundedBy` comes first). One thematic surface per face, each
with `bldg:lodNMultiSurface/gml:MultiSurface/gml:surfaceMember/gml:Polygon` and a
`gml:posList` ring (`srsDimension="3"`; fewer XML nodes than one `gml:pos` per
vertex). Ids are `cadkit_bldg_{n}`, `cadkit_bldg_{n}_s{k}` (surface) and
`cadkit_bldg_{n}_p{k}` (polygon), numbered in walk order: equal input gives equal
bytes. The XML node count (10 per face, 10 per building, 4 per hole) is checked against
`limits.max_objects` before the tree is built; it is an upper bound of what
`validate` counts, so a document the builder returns is also written.

Schema order was checked against the official `building.xsd` (CityGML 2.0
`AbstractBuildingType`: ... `lod2MultiSurface`, `lod2MultiCurve`,
`lod2TerrainIntersection`, `outerBuildingInstallation`, `interiorBuildingInstallation`,
`boundedBy`, `lod3Solid`, `lod3MultiSurface`, ... `lod4MultiSurface`; boundary surfaces:
`lod2MultiSurface`, `lod3MultiSurface`, `lod4MultiSurface`, `opening`). The tests in
`tests/buildings.rs` pass every written building file through the same official XSD
check as the other exports (`tests/support/xsd.rs`; required in CI by
`CADKIT_REQUIRE_GML_XSD`), for LoD 2, 3 and 4 and every grouping mode, and assert the
property order of every `bldg:Building` independently of the validator.

Ring orientation: ground faces point down and roof faces up after the write; wall
winding is left as drawn, and neither `validate` nor `geometry_issues` checks the
orientation of a multi-surface member, so mixed wall windings pass. Polygons carry no
`srsName`; they inherit the CRS of the root and building envelopes, as in the generic
export. No `xlink:href` is written.

`BuildingReport` gives per-building and total surface counts, unclassified and
skipped faces, skipped faces per code, ignored non-surface entities, closed polylines
ignored because the option is off, and one `BuildingIssue` per skipped or
unclassified face (at most min(max_objects, 100,000); `issues_truncated` beyond):
code, `WalkContext::path` (resolve with `Document::entity_at`; every cell of an
insert array shares it), `model_path_len`, mesh face index, native entity id,
effective layer, entity kind, and for written faces the building and polygon ids.
The report holds the caller's own layer names and ids; it is not meant for
third parties.

Evidence (synthetic tests in `tests/buildings.rs`): a box and a gable house drawn as
closed polylines in both windings classify as 4 walls/1 roof/1 ground and 4 walls/2
roofs/1 ground; all four grouping modes; welding within and outside the tolerance;
non-planar, degenerate, self-intersecting, curved and out-of-range mesh faces skipped
with resolving paths; faces inside two block references placed in model space with
paths `[insert, 6]`; an A-frame at LoD 2, 3 and 4. Every output is read with
`read_native` and `read` and passes `validate` with default geometry checks.
Measured in a debug build: 20,000 boxes (120,000 faces, `Connected`) write 58.8 MB in
9.1 s and read and validate in 9.3 s. Split (debug, at map coordinates, 63 MB):
collecting and checking the faces 0.8 s, welding and union-find 1.7 s, the geometry
validation `write_buildings` runs on its own output 4.2 s, serialization 2.6 s;
`read_native` 6.3 s. The validation pass is the largest single cost; it is kept because
it guarantees that the output passes the crate's own checks.

### Reader node count

The reader used to count every XML event as a node, while the writer's preflight
counts tree nodes. A text with entity references is several events (text, reference,
text, ...) but one node, and `quick_xml` escapes every `"` of the `cadkit.entity`
JSON as `&quot;`. Large generic exports could therefore exceed `max_objects` on
read although the writer had checked the same limit (reported: 66,053 polylines,
144 MB, "limit exceeded: XML nodes"). The reader now counts nodes as the tree holds
them: elements, comments inside the root, and one node per run of adjacent text,
CDATA and references. A synthetic 66,053-polyline generic export (113.8 MB with
6.6 million `&quot;` references) now reads with default limits. Escaping quotes in
text content is not required by XML and roughly doubles the size of the JSON
metadata; using a text-only escape in the writer would shrink such files but changes
their bytes, so it is left for a separate change.
