# pycadkit

Python bindings for [cadkit](https://github.com/TNYCL/cadkit): read DWG, DGN (V7 and V8),
DXF and CityGML 2.0 into one format-neutral model, and write SVG, JSON, DXF, seed-based
DGN V8 and CityGML 2.0. The parsers are written in Rust; there is no vendor SDK, and one
wheel per platform serves Python 3.9 and newer.

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

The API reference is [docs/python.md](https://github.com/TNYCL/cadkit/blob/main/docs/python.md).

"DWG", "DGN" and "DXF" name file formats only; cadkit is not affiliated with or certified by
any CAD vendor.

## License

Copyright 2026 TNYCL ([tnycl.com](https://tnycl.com)). Licensed under either of the Apache
License, Version 2.0 or the MIT license at your option (`LICENSE-APACHE`, `LICENSE-MIT`).
