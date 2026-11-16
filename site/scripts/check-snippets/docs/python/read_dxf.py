import cadkit

doc = cadkit.read("plan.dxf")  # ASCII or binary
print(doc.format, doc.version, len(doc.layers), "layers")
with open("plan-r2000.dxf", "w", encoding="utf-8") as f:
    f.write(doc.to_dxf(version="r2000"))
