import { readFileSync, writeFileSync } from "node:fs";
import { read, readCityGml } from "cadkit-wasm/node";

const CRS = "urn:ogc:def:crs:EPSG::32635"; // the CRS your coordinates are in; cadkit never guesses one

const doc = read(readFileSync("surfaces.dxf")); // 3D faces or polygons
const { xml, report } = JSON.parse(doc.toCityGmlWithReport(CRS, 1, false));
doc.free();
console.log(report.geometry_entities, "of", report.entities, "entities written as geometry");
writeFileSync("model.gml", xml);

const native = readCityGml(readFileSync("model.gml"));
console.log(native.validate()); // structural and geometric checks; returns counters
const preflight = native.preflightTkgm("city-model-tender"); // local structural checks only
console.log("acceptance_verified:", preflight.acceptance_verified); // always false
for (const issue of preflight.issues) console.log(issue.severity, issue.code, issue.field);
native.free();
