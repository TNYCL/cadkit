import { readFileSync, writeFileSync } from "node:fs";
import { CadDocument, inspect, read, stripDgnSeed } from "cadkit-wasm/node";

// 1. A seed: a DGN V8 file whose tables you may hand on, without its drawing content.
const { seed, report } = stripDgnSeed(readFileSync("template.dgn"), { modelName: "Model" });
console.log("kept:", report.named_levels, "named levels,", report.tag_sets, "tag sets");

// 2. cadkit-wasm has no seed preparation (Rust and Python do): match the seed by hand.
const target = inspect(seed); // the seed's units and 2D/3D
const source = read(readFileSync("plan.dxf"));
source.convertUnits(target.units.unit); // mutates the document
const json = source.toJSON();
source.free();
const model = { ...json.models[0], is_3d: target.is_3d }; // one model, the seed's dimension
for (const { kind, props } of model.entities) {
  props["dgn.color_index"] ??= 0; // a seed palette index: map your colours here
  if (kind.type === "text" && kind.halign !== "left") {
    // Centred and right-aligned text needs its advance in drawing units: measure it with
    // your fonts. This is the estimate prepare_document applies.
    props["dgn.text_length"] ??= [...kind.value].length * kind.height * kind.width_factor * 0.6;
  }
}
json.models = [model];

// 3. Write, then read the result back with cadkit's own reader.
const doc = CadDocument.fromJsonString(JSON.stringify(json));
writeFileSync("converted.dgn", doc.toDgn(seed));
doc.free();
const check = read(readFileSync("converted.dgn"));
console.log(check.info().format, check.info().models[0].entities, "entities");
check.free();
