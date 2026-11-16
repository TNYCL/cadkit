import cadkit

doc = cadkit.read("plan.dgn")  # V7 or V8
print(doc.format, doc.version, [layer["name"] for layer in doc.layers])
with open("plan.svg", "w", encoding="utf-8") as f:
    f.write(doc.to_svg(width=1200))
