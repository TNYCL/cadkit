# cadkit-dgn

Clean-room reader for DGN V7 (ISFF) and DGN V8 (CFB container) files, including tags, and a
seed-based DGN V8 writer for a documented geometry and tag subset. Part of
[cadkit](https://github.com/TNYCL/cadkit); format notes are in
[docs/dgn/FORMAT_NOTES.md](https://github.com/TNYCL/cadkit/blob/main/docs/dgn/FORMAT_NOTES.md).

Most applications should depend on the [`cadkit`](https://crates.io/crates/cadkit) facade crate, which detects the format and re-exports this crate.

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor. Sources of format knowledge are listed in
[docs/PROVENANCE.md](https://github.com/TNYCL/cadkit/blob/main/docs/PROVENANCE.md).

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of
[Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
