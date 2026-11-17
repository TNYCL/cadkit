// The TypeScript declarations agree with the runtime: the same exports and methods, and every
// entity type and key the samples produce is declared in the document model.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { wasm, pkgNodeDir, crateDir, sampleFiles, readBytes } from "./helpers.mjs";

const dts = readFileSync(path.join(pkgNodeDir, "cadkit_wasm.d.ts"), "utf8");
const webDts = readFileSync(path.join(crateDir, "pkg", "cadkit_wasm.d.ts"), "utf8");

/** Interfaces of a declaration file: name -> { keys, tag } (tag: the literal of a `type` field). */
function interfaces(text) {
  const out = new Map();
  const re = /^export (?:interface (\w+)(?: extends ([\w, ]+))? \{|type (\w+) = \{)\n([\s\S]*?)^\}/gm;
  for (const m of text.matchAll(re)) {
    const keys = new Set();
    let tag;
    for (const k of m[4].matchAll(/^ +(\w+)\??: ([^;]+);/gm)) {
      keys.add(k[1]);
      if (k[1] === "type" && /^"[^"]+"$/.test(k[2])) tag = JSON.parse(k[2]);
    }
    out.set(m[1] ?? m[3], { keys, tag, parents: m[2] ? m[2].split(",").map((s) => s.trim()) : [] });
  }
  return out;
}

/** Classes of a declaration file: name -> { methods, statics }. */
function classes(text) {
  const out = new Map();
  for (const m of text.matchAll(/^export class (\w+) \{\n([\s\S]*?)^\}/gm)) {
    const methods = new Set();
    const statics = new Set();
    for (const f of m[2].matchAll(/^ {4}(static )?(\w+)\(/gm)) (f[1] ? statics : methods).add(f[2]);
    out.set(m[1], { methods, statics });
  }
  return out;
}

test("declared exports and methods equal the runtime ones", () => {
  const functions = new Set([...dts.matchAll(/^export function (\w+)\(/gm)].map((m) => m[1]));
  const declared = classes(dts);
  const runtime = Object.keys(wasm).filter((k) => !k.startsWith("__"));
  assert.deepEqual(new Set(runtime), new Set([...functions, ...declared.keys()]));
  for (const [name, { methods, statics }] of declared) {
    const cls = wasm[name];
    const proto = Object.getOwnPropertyNames(cls.prototype).filter(
      (k) => k !== "constructor" && !k.startsWith("__") && typeof cls.prototype[k] === "function",
    );
    // wasm-bindgen adds free() and [Symbol.dispose]; the latter is not a string key.
    assert.deepEqual(new Set(proto), methods, name);
    const own = Object.getOwnPropertyNames(cls).filter((k) => typeof cls[k] === "function" && !k.startsWith("__"));
    assert.deepEqual(new Set(own), statics, name);
  }
  // The web build declares the same API plus its init functions.
  const webFunctions = new Set([...webDts.matchAll(/^export function (\w+)\(/gm)].map((m) => m[1]));
  assert.deepEqual(webFunctions, new Set([...functions, "initSync"]));
  assert.match(webDts, /export default function __wbg_init/);
  // Every type a signature names is declared.
  const declaredTypes = new Set([
    ...[...dts.matchAll(/^export (?:interface|type|class) (\w+)/gm)].map((m) => m[1]),
  ]);
  const used = new Set();
  for (const m of dts.matchAll(/^ {4}(?:static )?\w+\(([^)]*)\): ([^;]+);/gm)) {
    for (const t of `${m[1]} ${m[2]}`.matchAll(/\b([A-Z]\w+)\b/g)) used.add(t[1]);
  }
  for (const m of dts.matchAll(/^export function \w+\(([^)]*)\): ([^;]+);/gm)) {
    for (const t of `${m[1]} ${m[2]}`.matchAll(/\b([A-Z]\w+)\b/g)) used.add(t[1]);
  }
  const builtins = new Set(["Uint8Array", "ArrayBuffer", "SharedArrayBuffer", "ArrayBufferView", "Symbol", "Promise", "Array", "Record", "Partial"]);
  for (const t of used) assert.ok(declaredTypes.has(t) || builtins.has(t), `type ${t} is not declared`);
  assert.doesNotMatch(dts, /: any\b|\(any\)|<any>|any\[\]/, "no any in the declarations");
});

test("ts/document.d.ts is generated from the current schema", () => {
  const r = spawnSync(process.execPath, [path.join(crateDir, "scripts", "schema-to-ts.mjs"), "--check"], { encoding: "utf8" });
  assert.equal(r.status, 0, r.stderr);
});

test("every entity type and key in the samples is declared", () => {
  const ifaces = interfaces(dts);
  const keysOf = (name) => {
    const i = ifaces.get(name);
    assert.ok(i, `interface ${name}`);
    const keys = new Set(i.keys);
    for (const p of i.parents) for (const k of keysOf(p)) keys.add(k);
    return keys;
  };
  const byTag = new Map();
  for (const [name, i] of ifaces) if (name.startsWith("EntityKind") && i.tag) byTag.set(i.tag, name);
  const check = (what, value, iface) => {
    const keys = keysOf(iface);
    for (const k of Object.keys(value)) assert.ok(keys.has(k), `${what}: key ${k} is not declared in ${iface}`);
  };
  const seenTypes = new Set();
  let entities = 0;
  const visit = (e) => {
    entities++;
    check("entity", e, "Entity");
    const iface = byTag.get(e.kind.type);
    assert.ok(iface, `entity type ${e.kind.type} is not declared`);
    seenTypes.add(e.kind.type);
    check(e.kind.type, e.kind, iface);
    for (const a of e.attributes) check("attribute", a, "Attribute");
    if (e.kind.type === "group") e.kind.children.forEach(visit);
  };
  for (const file of sampleFiles()) {
    let doc;
    try {
      doc = wasm.read(readBytes(file));
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) throw e;
      continue;
    }
    try {
      const json = doc.toJSON();
      check("document", json, "DocumentJson");
      check("info", doc.info(), "DocumentInfo");
      for (const m of json.models) {
        check("model", m, "Model");
        m.entities.forEach(visit);
      }
      for (const b of json.blocks) {
        check("block", b, "Block");
        b.entities.forEach(visit);
      }
      for (const l of json.layers) check("layer", l, "Layer");
      for (const s of doc.models()) check("models()", s, "ModelSummary");
      for (const b of doc.blocks()) check("blocks()", b, "BlockSummary");
      check("stats()", doc.stats(), "DocumentStats");
    } finally {
      doc.free();
    }
  }
  assert.ok(entities > 100 && seenTypes.size >= 8, `${entities} entities, types ${[...seenTypes]}`);
});
