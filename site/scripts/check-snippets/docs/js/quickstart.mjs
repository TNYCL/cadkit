import { readFileSync, writeFileSync } from "node:fs";
import { read } from "cadkit-wasm/node";

const doc = read(readFileSync("plan.dxf")); // DWG, DGN, DXF or CityGML; the format is detected
const info = doc.info();
console.log(info.format, info.version, info.units);
for (const model of info.models) {
  console.log(`${model.name}: ${model.entities} entities`);
}
for (const warning of doc.warnings()) {
  console.log("warning:", warning.code, warning.message);
}
writeFileSync("plan.svg", doc.toSvg({ widthPx: 1200 }));
doc.free(); // release the WebAssembly memory of this document
