import cadkit

with open("building.gml", "rb") as f:
    native = cadkit.read_citygml(f.read())  # keeps IDs, references, attributes
print(native.validate())
doc = native.to_document()  # neutral geometry view, e.g. for SVG or DXF
print(doc)
