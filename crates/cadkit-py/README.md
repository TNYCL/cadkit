# cadkit (Python)

Python bindings for cadkit: read DWG, DGN and DXF drawings into one format-neutral model.

```python
import cadkit

doc = cadkit.read("plan.dxf")
print(doc)                       # <cadkit.Document format=dxf version="AC1032" ...>
open("plan.svg", "w").write(doc.to_svg())
```

See `docs/python.md` in the repository for the API reference.
