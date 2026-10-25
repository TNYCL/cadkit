# pycadkit

Python file I/O and document operations for [cadkit](https://github.com/TNYCL/cadkit),
an open-source CAD engine for building CAD applications. Read DWG, DGN (V7 and V8), DXF
and CityGML 2.0 into one format-neutral model; write SVG, JSON, DXF, seed-based DGN V8 and
CityGML 2.0. DWG is read-only; DGN output requires a compatible seed and covers a V8 subset.
The parsers are written in Rust; there is no vendor SDK, and one wheel per platform serves
Python 3.9 and newer.

```sh
pip install pycadkit
```

The package installs as `pycadkit` and imports as `cadkit`:

```python
import cadkit

doc = cadkit.read("plan.dxf")    # path, os.PathLike or bytes
print(doc)                       # <cadkit.Document format=dxf version="AC1032" ...>
open("plan.svg", "w").write(doc.to_svg())
```

Use `inspect` for header-only metadata, `Document.convert_units` and `explode_inserts`
for new documents without changing the source, and `V8Seed` for reusable DGN output seeds.
Native CityGML read/write and explicit generic export are also available. CityGML generic
export requires a CRS; DGN and CityGML export limits are linked from the
[format matrix](https://github.com/TNYCL/cadkit#format-support).

This package exposes the file/document APIs in the Python guide. The standalone Rust
geometry, scene and GPU rendering crates are separate modules, without Python bindings.

The API reference is [docs/python.md](https://github.com/TNYCL/cadkit/blob/main/docs/python.md).

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor.

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of the Apache
License, Version 2.0 or the MIT license at your option (`LICENSE-APACHE`, `LICENSE-MIT`).
