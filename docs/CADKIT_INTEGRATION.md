# Direct library integration

Cady can call cadkit directly through its Rust, Python, WASM or C/C++ bindings.
Format parsing, DGN record encoding, CityGML XML writing and format validation
belong in cadkit. The consuming application supplies its drawing data, export
settings and file destination. No separate conversion service is required.

## DGN export

The application supplies a neutral `Document`, a compatible DGN V8 seed and
`dgn::WriteOptions` to `cadkit::to_dgn`. Bindings expose the same operation; the
existing JSON options accept `preserve_seed_rasters` without a new C ABI function.
Units, dimension, palette and font tables must match the seed. A production seed
should be an approved blank template rather than an arbitrary client drawing.

When opening and rewriting an existing DGN, passing its original bytes as the
seed can preserve unchanged raster attachments. New or edited images need a
native attachment encoder that has not yet been implemented. Callers must retain
external image files; copying the DGN alone does not embed those files.

Center/right text requires measured `dgn.text_length` in drawing units. The
application must update that value when text or its font metrics change.
Identifier attributes should use the receiving profile's declared data type;
numeric identifiers must not be routed through an inexact floating-point type.

## CityGML export

The two public paths have different data contracts:

- `cadkit::to_citygml(Document, ExportOptions)` creates generic geometry objects
  with an explicit CRS and LoD. It does not infer buildings or independent units.
- `cadkit::gml::write(CityGmlDocument, ValidationOptions)` writes an explicit native
  CityGML model, including object IDs, building/room/surface relationships, shared
  references and typed attributes. Native JSON and the existing object/polygon
  builders are available to applications without implementing an XML writer.

The application supplies the meaning of its own objects and the receiving
institution's field mappings. A drawing of a rectangle alone cannot determine
whether it represents a room, floor, roof or parcel. Imported CityGML should keep
its native model as the source of identity and relationships; the neutral drawing
view is a geometric projection, not a replacement for that model.

No Cady source files were changed in this cadkit correction work. The eventual
adapter needs to marshal application objects, select the appropriate library
path, surface export errors, and save the returned bytes.

## Conversion diagnostics and TKGM preflight

`gml::write_document_with_report` accepts `UnsupportedGeometry::Reject` (the
existing default) or the explicit `MetadataOnly` option. The latter retains the
original neutral entity JSON in `cadkit.entity`, leaves unsupported GML geometry
absent and returns model/child-index diagnostics. Nested groups keep supported
child geometry and report the missing parts. External raster pixels and block
definitions are not embedded. This generic conversion is not a TKGM delivery.
Closed polylines remain closed curves; callers must create explicit `Polygon`
entities for surfaces. Circles use exact three-point GML Circle segments.

The same report path is available through Python `to_citygml_with_report`, WASM
`toCityGmlWithReport`, C `cadkit_document_to_citygml_with_report`, the matching C++
method and CLI `convert --gml-metadata-only`. Python returns `(bytes, report)`;
C/C++ and WASM return JSON with `xml` and `report` fields. Metadata-only mode is
opt-in in every interface. Display the report to the application user.

For the requested city-model tender, the [production guide](https://cbs.tkgm.gov.tr/3d/html/giris.html)
uses CityGML **2.0**; its latest listed revision is **v2.50** (6 May 2025).
The separate Digital Building registration guide is TKGMCityGML 3.0.3.
Buildings are `Building`,
storeys are `CityObjectGroup` and independent sections are `GenericCityObject`
with the required class, typed attributes and links. Supplying only generic
geometry does not populate that model. See [TKGM profile notes](gml/TKGM.md).

`gml::tkgm::preflight(doc, Profile::CityModelTender, limits)` checks a documented
architectural subset on the native model. The distinct
`Profile::DigitalBuildingRegistration` applies registration-specific rules.
Python exposes `CityGmlDocument.preflight_tkgm`, WASM `preflightTkgm`, and C
`cadkit_citygml_preflight_tkgm` accepts native JSON. Each requires the profile
string `city-model-tender` or `digital-building-registration`; there is no default.
CLI uses `validate-gml --tkgm-profile city-model-tender`.
The report always states `acceptance_verified: false` and names unchecked rules.
Tender-year-dependent MAKS rules and sustainability extensions are explicitly
unchecked in the common tender subset; select the contract's rules at acceptance.
The app should run this before export in addition to native geometry and official
XSD checks; the receiving TKGM system remains the final acceptance authority.

## Acceptance

Local write/read comparisons, independent CFB opening and official XSD checks
are necessary checks. They do not establish recipient acceptance. Release gates
also need files created by the consuming application, inspection in the target
CAD application, save/reopen checks, and validation by the receiving system.
See [implementation status](INTERCHANGE_STATUS.md) and the
[DGN](dgn/FORMAT_NOTES.md) / [CityGML](gml/FORMAT_NOTES.md) contracts for current
coverage and unresolved cases.
