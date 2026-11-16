import { readFileSync } from "node:fs";
import { read } from "cadkit-wasm/node";

const doc = read(readFileSync("plan.dgn")); // V7 or V8
console.log(doc.info().format, doc.layers().map((layer) => layer.name));
doc.free();
