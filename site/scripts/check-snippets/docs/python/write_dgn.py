import cadkit

# 1. A seed: a DGN V8 file whose tables you may hand on, without its drawing content.
with open("template.dgn", "rb") as f:
    seed_bytes, report = cadkit.strip_dgn_seed(f.read(), model_name="Model")
print("kept:", report["named_levels"], "named levels,", report["tag_sets"], "tag sets")
seed = cadkit.V8Seed(seed_bytes)

# 2. Adapt the drawing to the seed (model, units, 2D to 3D, colours), then write.
doc = seed.prepare_document(cadkit.read("plan.dxf"))
with open("converted.dgn", "wb") as f:
    f.write(doc.to_dgn(seed))

# 3. Read the result back with cadkit's own reader.
print(cadkit.read("converted.dgn"))
