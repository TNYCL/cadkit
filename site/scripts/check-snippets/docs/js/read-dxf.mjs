import { readFileSync, writeFileSync } from "node:fs";
import { read } from "cadkit-wasm/node";

const doc = read(readFileSync("plan.dxf")); // ASCII or binary
writeFileSync("plan-r2000.dxf", doc.toDxf("r2000"));
doc.free();
