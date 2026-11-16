import { readFileSync } from "node:fs";
import { readCityGml } from "cadkit-wasm/node";

const native = readCityGml(readFileSync("building.gml")); // keeps IDs, references, attributes
console.log(native.validate());
native.free();
