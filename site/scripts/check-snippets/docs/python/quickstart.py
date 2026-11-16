import cadkit

doc = cadkit.read("plan.dxf")  # DWG, DGN, DXF or CityGML; the format is detected
print(doc.format, doc.version, doc.units["unit"])
for model in doc.models:
    print(f"{model['name']}: {len(model['entities'])} entities")
for warning in doc.warnings:
    print("warning:", warning["code"], warning["message"])

with open("plan.svg", "w", encoding="utf-8") as f:
    f.write(doc.to_svg(width=1200))
