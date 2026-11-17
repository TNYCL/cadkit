// Smoke test of the npm package as it is installed.
//
// From the repository, after build.py (packs crates/cadkit-wasm/npm, installs the tarball into a
// temporary project and runs this script there):
//   node crates/cadkit-wasm/tests/package_smoke.mjs --pack crates/cadkit-wasm/npm
// In a directory where the tarball is installed (the release workflow):
//   npm install /path/to/cadkit-wasm-X.Y.Z.tgz && node package_smoke.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const self = fileURLToPath(import.meta.url);
const packAt = process.argv.indexOf("--pack");

if (packAt !== -1) {
  const npmDir = path.resolve(process.argv[packAt + 1] ?? "");
  const work = mkdtempSync(path.join(tmpdir(), "cadkit-wasm-smoke-"));
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  const run = (args, cwd) => execFileSync(npm, args, { cwd, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });
  try {
    const tarball = run(["pack", npmDir, "--pack-destination", work, "--silent"], work).trim().split("\n").pop();
    writeFileSync(path.join(work, "package.json"), JSON.stringify({ name: "smoke", private: true, type: "module" }));
    run(["install", "--no-audit", "--no-fund", "--silent", path.join(work, tarball)], work);
    copyFileSync(self, path.join(work, "package_smoke.mjs"));
    execFileSync(process.execPath, [path.join(work, "package_smoke.mjs")], { cwd: work, stdio: "inherit" });
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  process.exit(0);
}

const require = createRequire(import.meta.url);
const DXF = new TextEncoder().encode([
  "0", "SECTION", "2", "ENTITIES",
  "0", "LINE", "8", "0", "10", "0.0", "20", "0.0", "30", "0.0", "11", "10.0", "21", "5.0", "31", "0.0",
  "0", "ENDSEC", "0", "EOF", "",
].join("\n"));

function exercise(api, label) {
  assert.equal(api.detect(DXF), "dxf", label);
  const doc = api.read(DXF.buffer.slice(0));
  try {
    assert.equal(doc.info().format, "dxf", label);
    assert.match(doc.toSvg(), /<svg/);
    assert.equal(doc.toSvgBounded({ maxOutputBytes: 4096 }), doc.toSvg());
    assert.throws(() => doc.toSvgBounded({ maxOutputBytes: 1 }), { kind: "limit_exceeded" });
    assert.throws(() => doc.toDxf(123), { kind: "bad_argument" });
    assert.equal(doc.pick(0, 5, 2.5, { tolerance: 0.5 })?.kind, "line");
    assert.equal(doc.entity(0, 0).kind.type, "line");
  } finally {
    doc.free();
  }
  assert.ok(api.memoryBytes() > 0);
}

// The package ships one module.
const pkgDir = path.dirname(require.resolve("cadkit-wasm/package.json"));
const wasmFiles = [];
const walk = (dir) => {
  for (const n of readdirSync(dir)) {
    const p = path.join(dir, n);
    if (statSync(p).isDirectory()) walk(p);
    else if (n.endsWith(".wasm")) wasmFiles.push(path.relative(pkgDir, p));
  }
};
walk(pkgDir);
assert.deepEqual(wasmFiles, [path.join("web", "cadkit_wasm_bg.wasm")]);
const wasmBytes = readFileSync(require.resolve("cadkit-wasm/cadkit_wasm_bg.wasm"));

// Plain import under Node: the "node" condition gives the Node build, ready on import; init()
// does nothing, so code written for the browser runs unchanged.
const root = await import("cadkit-wasm");
await root.default();
root.initSync();
exercise(root, "import cadkit-wasm");

// require() of the package root and of the node subpath: the same CommonJS module.
const cjs = require("cadkit-wasm");
assert.equal(cjs, require("cadkit-wasm/node"));
exercise(cjs, "require cadkit-wasm");
assert.equal(root.read, cjs.read, "ESM and require share one module instance");
exercise(await import("cadkit-wasm/node"), "import cadkit-wasm/node");

// The web build, given the module bytes (Node cannot fetch file URLs).
const web = await import("cadkit-wasm/web");
await web.default({ module_or_path: wasmBytes });
exercise(web, "web build");

const { version } = require("cadkit-wasm/package.json");
console.log(`cadkit-wasm ${version}: import, require, node and web entry points work with one ${wasmBytes.length.toLocaleString("en-US")}-byte module`);
