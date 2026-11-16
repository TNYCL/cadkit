import cadkit

CRS = "urn:ogc:def:crs:EPSG::32635"  # the CRS your coordinates are in; cadkit never guesses one

doc = cadkit.read("surfaces.dxf")  # 3D faces or polygons; text and curves need a mapping
xml, report = doc.to_citygml_with_report(CRS, lod=1)
print(report["geometry_entities"], "of", report["entities"], "entities written as geometry")
with open("model.gml", "wb") as f:
    f.write(xml)

with open("model.gml", "rb") as f:
    native = cadkit.read_citygml(f.read())
print(native.validate())  # structural and geometric checks; returns counters

preflight = native.preflight_tkgm("city-model-tender")  # local structural checks only
print("acceptance_verified:", preflight["acceptance_verified"])  # always False
for issue in preflight["issues"]:
    print(issue["severity"], issue["code"], issue["field"])
