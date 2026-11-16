import sys
from collections import Counter

import cadkit

path = sys.argv[1]
header = cadkit.inspect(path)  # header only: no entity decoding
print(header["format"], header["version"], header["units"])

doc = cadkit.read(path)
per_layer = Counter(entity["layer"] for entity in doc.entities())  # model 0
for layer in doc.layers:
    state = "on" if layer["visible"] else "off"
    if layer["frozen"]:
        state += ", frozen"
    print(f"{layer['name']:<24} {state:<12} {per_layer[layer['name']]} entities")
for warning in doc.warnings[:20]:
    print("warning:", warning["code"], warning["message"])
