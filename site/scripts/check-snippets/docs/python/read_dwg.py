import cadkit

doc = cadkit.read("plan.dwg")
print(doc.format, doc.version, len(doc.layers), "layers")
with open("plan.svg", "w", encoding="utf-8") as f:
    f.write(doc.to_svg(width=1200))
