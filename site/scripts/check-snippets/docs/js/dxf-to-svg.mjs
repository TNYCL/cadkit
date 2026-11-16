import { readFileSync, writeFileSync } from "node:fs";
import { read } from "cadkit-wasm/node";

const doc = read(readFileSync("plan.dxf"));
const svg = doc.toSvg({
  model: 0, // model space; paper layouts follow
  widthPx: 1600, // the height follows the drawing's aspect ratio
  strokePx: 1,
  background: null, // transparent; or a CSS colour such as "#ffffff"
  text: true,
  includeHiddenLayers: false,
});
writeFileSync("plan.svg", svg);
doc.free();
