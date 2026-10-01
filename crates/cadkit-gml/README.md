# cadkit-gml

Bounded CityGML 2.0 reader and writer with native XML preservation, geometry validation and
a bridge to the [cadkit](https://github.com/TNYCL/cadkit) neutral model. No network access, schema download or CRS
transformation happens at run time. Details are in
[docs/gml/FORMAT_NOTES.md](https://github.com/TNYCL/cadkit/blob/main/docs/gml/FORMAT_NOTES.md).

Most applications should depend on the [`cadkit`](https://crates.io/crates/cadkit) facade crate, which detects the format and re-exports this crate.

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
