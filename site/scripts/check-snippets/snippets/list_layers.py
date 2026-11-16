import cadkit

doc = cadkit.read("cadkit-sample-plan.dxf")
for layer in doc.layers:
    print(layer["name"])
with open("plan.svg", "w", encoding="utf-8") as f:
    f.write(doc.to_svg(width=1200))
