// Smoke test of the packed npm package (both entry points), run by the release workflow:
//   npm install /path/to/cadkit-wasm-X.Y.Z.tgz && node package_smoke.mjs
// It must run from the directory where the tarball is installed, not from the repository.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import init, { detect, read } from "cadkit-wasm";

const require = createRequire(import.meta.url);
const DXF = new TextEncoder().encode([
  "0", "SECTION", "2", "ENTITIES",
  "0", "LINE", "8", "0", "10", "0.0", "20", "0.0", "30", "0.0", "11", "10.0", "21", "5.0", "31", "0.0",
  "0", "ENDSEC", "0", "EOF", "",
].join("\n"));

// Web build: Node cannot fetch file URLs, so the module bytes are passed in.
await init({ module_or_path: await readFile(require.resolve("cadkit-wasm/cadkit_wasm_bg.wasm")) });
assert.equal(detect(DXF), "dxf");
const web = read(DXF);
assert.equal(web.info().format, "dxf");
assert.match(web.toSvg(), /<svg/);
assert.equal(web.toSvgBounded({ maxOutputBytes: 4096 }), web.toSvg());
assert.throws(() => web.toSvgBounded({ maxOutputBytes: 1 }), { kind: "limit_exceeded" });
web.free();

// Node build: CommonJS, initialized on load.
const node = require("cadkit-wasm/node");
assert.equal(node.detect(DXF), "dxf");
const doc = node.read(DXF);
assert.equal(doc.info().format, "dxf");
assert.equal(doc.toSvgBounded({ maxOutputBytes: 4096 }), doc.toSvg());
assert.throws(() => doc.toSvgBounded({ maxOutputBytes: 1 }), { kind: "limit_exceeded" });
doc.free();

const { version } = require("cadkit-wasm/package.json");
console.log(`cadkit-wasm ${version}: web and node entry points work`);
