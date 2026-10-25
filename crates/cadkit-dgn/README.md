# cadkit-dgn

Clean-room reader for DGN V7 (ISFF) and DGN V8 (CFB container) files, including tags, and a
seed-based DGN V8 writer for a documented geometry and tag subset. Part of
[cadkit](https://github.com/TNYCL/cadkit); format notes are in
[docs/dgn/FORMAT_NOTES.md](https://github.com/TNYCL/cadkit/blob/main/docs/dgn/FORMAT_NOTES.md).

## Seed-based V8 output

Build DGN import/export into a CAD application using a neutral document and a compatible
seed. `V8Seed` parses a single-design-model seed once for reuse; `prepare_document` handles
explicit unit, dimension and symbology conversion. Rust accessors expose the seed palette,
levels, font names and timestamp, and `Symbology` provides typed access to DGN property keys.
The `write` Cargo feature is enabled by default; disable default features for reading only.

There is no DGN V7 writer. V8 output covers the documented geometry/tag/raster subset;
full format coverage and external application acceptance remain open. See the
[writer contract](https://github.com/TNYCL/cadkit/blob/main/docs/dgn/FORMAT_NOTES.md#seed-based-v8-writer)
and [host integration guide](https://github.com/TNYCL/cadkit/blob/main/docs/CADKIT_INTEGRATION.md#dgn-export).

Most applications should depend on the [`cadkit`](https://crates.io/crates/cadkit) facade crate, which detects the format and re-exports this crate.

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
