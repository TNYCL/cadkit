import { readFileSync } from "node:fs";
import { read } from "cadkit-wasm/node";

const doc = read(readFileSync("plan.dwg"));
console.log(doc.info().version, doc.layers().map((layer) => layer.name));
doc.free();
