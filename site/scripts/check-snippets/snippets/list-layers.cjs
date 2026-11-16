const { read } = require("cadkit-wasm/node");
const fs = require("node:fs");

const doc = read(fs.readFileSync("cadkit-sample-plan.dxf"));
for (const layer of doc.layers()) {
  console.log(layer.name);
}
fs.writeFileSync("plan.svg", doc.toSvg({ widthPx: 1200 }));
doc.free();
