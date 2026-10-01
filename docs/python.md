# cadkit for Python

Python bindings for cadkit (`crates/cadkit-py`), built with PyO3 (`abi3-py39`, one wheel per
platform for Python 3.9+) and maturin. The extension module is `cadkit._cadkit`; the public
package is `cadkit`. The distribution is published on PyPI as `pycadkit` (the name `cadkit`
is taken there), so it installs as `pycadkit` and imports as `cadkit`.

## Install

From PyPI: `pip install pycadkit`. Wheels cover Linux x86_64 and
aarch64 (manylinux), macOS arm64 and x86_64, and Windows x64; other platforms build from the
sdist, which needs a Rust toolchain.

From a checkout:

```sh
pip install maturin
cd crates/cadkit-py
maturin develop --release      # editable install into the active virtualenv
maturin build --release        # or build a wheel into target/wheels/
```

In this repository, route builds through the lock: `python scripts/buildlock.py .venv/Scripts/maturin develop -m crates/cadkit-py/Cargo.toml`.

## Quick start

```python
import cadkit

doc = cadkit.read("plan.dgn")          # str, os.PathLike or bytes
print(doc)                             # <cadkit.Document format=dgn_v8 version="V8" models=1 entities=1234>

for layer in doc.layers:
    print(layer["name"], layer["visible"])

for e in doc.entities():               # model 0
    if e["kind"]["type"] == "line":
        print(e["kind"]["start"], e["kind"]["end"])

open("plan.svg", "w").write(doc.to_svg(width=1200))
data = doc.to_dict()                   # nested dicts/lists
text = doc.to_json(pretty=True)

try:
    cadkit.read(b"not a drawing")
except cadkit.UnknownFormatError:
    ...
```

## API summary

| Item | Description |
| --- | --- |
| `read(source, *, keep_raw=False, codepage=None, max_input_bytes=None, max_decompressed_bytes=None, max_objects=None, max_depth=None) -> Document` | `source` is a path (`str`/`PathLike`) or `bytes`/`bytearray`/`memoryview`. `keep_raw` keeps original record bytes in entities (`raw`). `codepage` is a WHATWG label (e.g. `"windows-1254"`) used for 8-bit text when the file declares none. The `max_*` keywords override the resource limits (`None` = default); exceeding one raises `LimitExceededError`. |
| `detect(data: bytes) -> str \| None` | `"dwg"`, `"dxf"`, `"dgn_v7"`, `"dgn_v8"` or `None`. |
| `Document.format / version / application / codepage` | Source info (`format` as above, `version` e.g. `"AC1032"`). |
| `Document.units` | `{"unit": "meter", "meters_per_unit": 1.0}`. |
| `Document.layers / blocks / models / warnings` | Lists of dicts, shaped like the Rust model serialized with serde. |
| `Document.to_dict()` | Whole document as nested dicts. |
| `Document.to_json(pretty=False)` | Whole document as JSON text. |
| `Document.to_svg(model=0, width=1600, stroke=1.0, background="#ffffff", text=True, expand_blocks=True, images=True)` | SVG text; `background=None` is transparent. `width` and `stroke` must be finite and > 0 (`ValueError`). Raises `IndexError` for a bad model index. |
| `Document.to_dxf(version="r2018")` | ASCII DXF text; versions `r12`, `r2000`, `r2004`, `r2007`, `r2010`, `r2013`, `r2018`. |
| `Document.entities(model=0)` | Iterator of entity dicts (`{"id", "layer", "color", "kind": {"type": ...}, "props", ...}`). |

Conventions follow the model: coordinates in drawing units, angles in radians (see
`docs/ARCHITECTURE.md`). Format-specific data is under `props` keys prefixed `dgn.`, `dwg.`, `dxf.`.

### Exceptions

`CadkitError` is the base class.

| Exception | Raised for |
| --- | --- |
| `UnknownFormatError` | input is not a recognized drawing |
| `UnsupportedError` | valid input using an unimplemented feature |
| `LimitExceededError` | input exceeds a resource limit |
| `InvalidDataError` | truncated or malformed data |

Missing or unreadable files raise the usual `OSError` subclasses (`FileNotFoundError`, ...).

## Design notes

- The model stays in Rust. Dict conversion serializes with serde to JSON (GIL released) and
  decodes with the C `json` module. This avoids a second mapping layer (and a `pythonize`
  dependency) that would have to track every model change.
- The GIL is released while parsing, serializing, rendering and writing, so threads can read
  files in parallel.
- `Document` is immutable; dict-valued properties are rebuilt on each access, so cache them in a
  local variable inside hot loops.
- Type information ships as `.pyi` stubs plus `py.typed`.

## Tests

```sh
.venv/Scripts/python -m pytest crates/cadkit-py/tests -rs
```

Corpus tests skip when `corpus/public` is absent.

## DGN V8 and native CityGML

`Document.from_json(text)` constructs a neutral document. `doc.to_dgn(seed_bytes,
options_json=None)` returns bytes; options follow Rust `dgn::WriteOptions`.
`doc.to_citygml(srs_name, lod=1)` returns generic CityGML geometry as UTF-8 bytes.

For semantic preservation use `read_citygml(bytes, options_json=None)` instead of
the neutral reader. Its `CityGmlDocument` has `to_gml(options_json=None)`,
`to_json()`, `from_json(text)`, `validate(options_json=None)` and
`to_document(lod=None)`. Native read options use Rust `ReadOptions`; native write
and validation options use `gml::ValidationOptions`. Defaults validate geometry;
`{"geometry": false}` explicitly permits preserving existing geometry defects.
No schema or CRS is downloaded. See [GML limits](gml/FORMAT_NOTES.md) and
[DGN limits](dgn/FORMAT_NOTES.md) before choosing an export path.
