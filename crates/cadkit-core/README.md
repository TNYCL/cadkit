# cadkit-core

The format-neutral drawing model shared by the [cadkit](https://github.com/TNYCL/cadkit) readers and writers:
documents, models, layers, blocks and entities, bounded byte readers, read limits, errors
and warnings, polygon validation and SVG export. This is the document layer of the
cadkit CAD engine; it has no GPU dependency.

`Document` operations include affine transforms, unit conversion, block expansion, group
flattening, layer filtering and unused-definition purging. Coordinates use drawing units;
angles are radians. Version 0.5 adds constructors and setters for non-exhaustive model and
option types, `LimitKind` on limit errors and `Document::schema_version` (currently 1).
Enable `schemars` for JSON Schema; the repository also includes the
[document schema](https://github.com/TNYCL/cadkit/blob/main/docs/schema/document-v1.schema.json).
See the [migration guide](https://github.com/TNYCL/cadkit/blob/main/CHANGELOG.md#migrating-from-04)
for source and JSON compatibility.

Standalone analytic geometry lives in [`cadkit-geom`](https://crates.io/crates/cadkit-geom);
scene preparation and GPU rendering live in separate crates.

Most applications should depend on the [`cadkit`](https://crates.io/crates/cadkit) facade crate, which detects the format and re-exports this crate.

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
