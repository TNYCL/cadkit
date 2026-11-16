import { readFileSync } from "node:fs";
import { inspect, read } from "cadkit-wasm/node";

const bytes = readFileSync(process.argv[2]);
console.log(inspect(bytes)); // header only: no entity decoding

const doc = read(bytes);
const perLayer = new Map();
for (const entity of doc.toJSON().models[0]?.entities ?? []) {
  perLayer.set(entity.layer, (perLayer.get(entity.layer) ?? 0) + 1);
}
for (const layer of doc.layers()) {
  const state = layer.visible ? "on" : "off";
  console.log(layer.name.padEnd(24), state.padEnd(4), perLayer.get(layer.name) ?? 0, "entities");
}
for (const warning of doc.warnings().slice(0, 20)) {
  console.log("warning:", warning.code, warning.message);
}
doc.free();
