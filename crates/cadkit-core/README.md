# cadkit-core

The format-neutral drawing model shared by the [cadkit](https://github.com/TNYCL/cadkit) readers and writers:
documents, models, layers, blocks and entities, bounded byte readers, read limits, errors
and warnings, polygon validation and SVG export.

Most applications should depend on the [`cadkit`](https://crates.io/crates/cadkit) facade crate, which detects the format and re-exports this crate.

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
