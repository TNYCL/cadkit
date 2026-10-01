# TKGM workflow preflight

## City-model tender: the selected delivery

The requested workflow is **3D city-model / tender delivery**, specifically the
architectural models exercised here. Its public reference is the
[TKGM 3D Cadastre Production Guide](https://cbs.tkgm.gov.tr/3d/html/giris.html).
The [revision history](https://cbs.tkgm.gov.tr/3d/html/VersiyonTakibi.html) lists
v2.50, dated 2025-05-06, as the latest entry on retrieval (2026-10-01).
This is separate from the registration guide below. Both use OGC CityGML 2.0.

Select `Profile::CityModelTender` in Rust, or `city-model-tender` in bindings:

```sh
cadkit validate-gml model.gml --tkgm-profile city-model-tender
```

This local architectural subset checks common building, storey and independent
section field presence/types, LoD property presence, storey reference targets,
and an explicit three-dimensional TUREF envelope. It accepts multiple building
`geometrySuitability` values and common-area `Room` references. It checks
`buildingHeight` as a double, as specified by the
[architectural building chapter](https://cbs.tkgm.gov.tr/3d/html/MimariBina.html).
It does not replace supplied identity, integration or suitability values with
registration constants, or require registration-only fields unconditionally.

The [MAKS matrix](https://cbs.tkgm.gov.tr/3d/html/MAKSKontrolTablosu.html) varies
field requirements by tender period; sustainability extensions add further rules.
The common subset checks types when MAKS fields exist, but does **not** check
those conditional presence/value rules. Reports explicitly list
`maks_rules_by_tender_year` and `sustainability_extensions_by_contract` as
unchecked. A passing report is not approval for every tender year or contract.
Photogrammetric models, delivery-folder naming and external CSV/SHP matching are
outside this architectural subset. No source values are rewritten by preflight.

An absent `groupMember` on a storey declaring zero independent sections is
reported as `tkgm.empty_storey_membership` with severity `review`, rather than a
definite error. Common-area Rooms may already refer to that storey through
`storeyObjectReference`; preflight does not invent extra ownership links. Other
local mismatches have severity `error`. CLI returns nonzero for either severity
so a review finding cannot silently become acceptance. These are local diagnostic
categories, not error codes from the receiving authority.

Native CityGML remains the editable source of IDs, typed attributes and links.
The generic CAD-to-GML conversion API cannot infer this semantic structure.
The guide requires [final validation by the administration's software](https://cbs.tkgm.gov.tr/3d/html/TKGM3DValidation.html).
No private files were submitted to an external service in this work.

## Separate Digital Building registration profile

Public reference: [TKGM Digital Building Production Guide](https://cbs.tkgm.gov.tr/surdurulebilirlik/),
retrieved 2026-10-01. Its latest listed revision is TKGMCityGML 3.0.3, dated
2025-03-19. That profile uses OGC CityGML 2.0; the version numbers are different
contracts. No vendor SDK, third-party CAD source or private field values were
used for these rules.

The checked workflow is Digital Building registration for condominium/easement
transactions. A TKGM city-model procurement may use different rules; do not apply
these registration constants merely because both recipients are TKGM. Reports
state `workflow: digital_building_registration` explicitly.

## Data supplied by the consuming application

| Meaning | Native CityGML representation |
|---|---|
| Building | `bldg:Building`, class `MimariBina`, typed identification fields, LoD0/1/2 geometry |
| Storey | `grp:CityObjectGroup`, class `Kat`, parent building and member links, storey geometry |
| Independent section | `gen:GenericCityObject`, class `BagimsizBolum`, typed identity/area/usage fields and LoD2 geometry |
| Room/part | Explicit native objects and ownership from the production model; never inferred from a rectangle |
| Coordinates | Appropriate TUREF 3-degree zone EPSG 5253 through 5259, metres, orthometric elevations; explicit 3D envelope |

The library does not determine the correct geographic zone, convert a local CAD
origin into a surveyed CRS, calculate an orthometric height, query MAKS/TAKBIS,
or invent legal ownership. The consuming application must supply those values.
IDs declared `gen:intAttribute` must remain integers through the native model;
using a `doubleAttribute` merely because the value fits f64 is a type mismatch.

## Implemented subset

`cadkit::gml::tkgm::preflight(doc, Profile::DigitalBuildingRegistration, limits)`
validates native structure and collects missing
required building/storey/section fields, wrong declared generic types, duplicate
single-valued fields, missing geometry properties, wrong storey parent/member
targets, missing explicit 3D TUREF envelope, and four fixed independent-section
codes stated in the guide. It also flags generic conversion metadata where GML
geometry is absent. Diagnostics contain only public field names and traversal
indices. Output is capped at min(max_objects, 10,000) findings and marks truncation.

An empty finding list means only that these local checks found no issue. The
report always has `acceptance_verified: false`. It does not implement the entire
online rule engine, every code list, name/ID naming conventions, geometry-property
content constraints, all room/opening/installation rules, external identity
existence, or topological intersection tests. XSD and geometric validation must
be run separately. The app must retain the native CityGML model for editing.

There is an unresolved inconsistency in the public guide: the `propertyLot`
table calls the field an integer, but describes a numerator/denominator fraction
and shows `stringAttribute` in its XML example. Preflight checks presence only
and lists `property_lot_type_ambiguity` among its unchecked rules; it does not
coerce source values or claim this ambiguity is resolved.
Similarly, the `buildingHeight` table requests a string, while the official
downloadable examples use a double. Its presence is checked and
`building_height_type_ambiguity` is reported as unchecked. Receiver clarification
is required before forcing either representation on an application.

## Acceptance

Use the [official TKGM validator](https://3dbinadogrula.tkgm.gov.tr/) for final
acceptance. The guide explicitly distinguishes other viewers/validators from
TKGM's own acceptance. No client data was uploaded and no official validation
code was requested in this work. Public sample roundtrips and synthetic tests
are local interoperability evidence, not institutional certification.

```sh
cadkit validate-gml model.gml --tkgm-profile digital-building-registration
```

This command returns nonzero for geometry findings or local profile findings;
success is not an official TKGM approval. Cady integration remains a separate
application change, using these cadkit APIs directly.
