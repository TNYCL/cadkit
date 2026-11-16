#!/usr/bin/env node
// Prebuild step for the demo samples (runs before dev, check and build; see scripts/prebuild.mjs).
//
//   1. Third-party samples (src/demo/samples.ts, "upstream"): download each file from its pinned
//      upstream commit into public/samples/third-party/ (git-ignored) and verify its SHA-256 and
//      size. A file already present with the right hash is not downloaded again. Any mismatch or
//      download failure stops the build: there is no fallback.
//   2. Own samples marked "atBuild" (the large contour map) are not committed: their generator
//      script is run here and the result written to public/samples/generated/ (git-ignored). A
//      file already present with the same bytes is left alone.
//   3. Every sample (own and third-party) is read with cadkit-wasm (Node build); the facts shown on
//      /demo/samples/ (size, SHA-256, format, version, entity and layer counts, warnings) are
//      written to src/demo/generated/samples.json (git-ignored). A sample that does not open
//      stops the build. Cached by content hash and cadkit-wasm version.

import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { SAMPLES, rawUrl } from '../../src/demo/samples.ts';

const require = createRequire(import.meta.url);
const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const publicDir = join(root, 'public');
const outDir = join(root, 'src', 'demo', 'generated');
const outPath = join(outDir, 'samples.json');
const wasmVersion = require('cadkit-wasm/package.json').version;

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const fail = (msg) => {
	console.error('samples: ' + msg);
	process.exit(1);
};

async function download(url, attempts = 3) {
	let last;
	for (let i = 1; i <= attempts; i++) {
		try {
			const res = await fetch(url, { signal: AbortSignal.timeout(60_000), redirect: 'follow' });
			if (!res.ok) throw new Error(`HTTP ${res.status}`);
			return new Uint8Array(await res.arrayBuffer());
		} catch (e) {
			last = e;
			if (i < attempts) await new Promise((r) => setTimeout(r, 1000 * i));
		}
	}
	throw new Error(`${url}: ${last?.message ?? last}`);
}

// 1. Third-party files.
let fetched = 0;
for (const s of SAMPLES) {
	const u = s.upstream;
	if (!u) continue;
	const target = join(publicDir, s.path);
	if (existsSync(target) && sha256(readFileSync(target)) === u.sha256) continue;
	const url = rawUrl(u);
	let bytes;
	try {
		bytes = await download(url);
	} catch (e) {
		fail(`could not download ${s.id}: ${e.message}`);
	}
	const got = sha256(bytes);
	if (got !== u.sha256 || bytes.length !== u.bytes) {
		fail(`${s.id}: ${url} has SHA-256 ${got} (${bytes.length} bytes), expected ${u.sha256} (${u.bytes} bytes)`);
	}
	mkdirSync(dirname(target), { recursive: true });
	const tmp = target + '.part';
	writeFileSync(tmp, bytes);
	renameSync(tmp, target);
	fetched++;
	console.log(`samples: fetched ${s.id} (${bytes.length} bytes) from ${u.repo}@${u.commit.slice(0, 12)}`);
}
// public/samples/third-party/ and public/samples/generated/ are git-ignored by site/.gitignore
// (no ignore file inside public/, which would be published).

// Licence copies must exist for every upstream sample.
for (const s of SAMPLES) {
	if (s.upstream && !existsSync(join(root, s.upstream.licenseCopy))) fail(`missing licence copy ${s.upstream.licenseCopy}`);
}

// 2. Own samples that are built here instead of being committed.
let built = 0;
for (const s of SAMPLES) {
	if (!s.atBuild) continue;
	if (!s.generator) fail(`${s.id}: an atBuild sample needs a generator`);
	const target = join(publicDir, s.path);
	let text;
	try {
		const mod = await import(pathToFileURL(join(root, s.generator)).href);
		text = mod.buildSample();
	} catch (e) {
		fail(`could not generate ${s.id} with ${s.generator}: ${e.message}`);
	}
	if (existsSync(target) && readFileSync(target, 'utf8') === text) continue;
	mkdirSync(dirname(target), { recursive: true });
	const tmp = target + '.part';
	writeFileSync(tmp, text);
	renameSync(tmp, target);
	built++;
	console.log(`samples: generated ${s.id} (${Buffer.byteLength(text)} bytes) with ${s.generator}`);
}

// 3. Facts.
const files = SAMPLES.map((s) => {
	const p = join(publicDir, s.path);
	if (!existsSync(p)) fail(`missing sample file public${s.path}`);
	const bytes = readFileSync(p);
	return { s, bytes, hash: sha256(bytes) };
});
const key = sha256(JSON.stringify([wasmVersion, SAMPLES, files.map((f) => f.hash)]));
if (existsSync(outPath)) {
	try {
		if (JSON.parse(readFileSync(outPath, 'utf8')).key === key) {
			console.log(`samples: ${files.length} samples up to date${fetched ? `, ${fetched} fetched` : ''}${built ? `, ${built} generated` : ''}`);
			process.exit(0);
		}
	} catch {
		// Regenerate below.
	}
}

const { read } = require('cadkit-wasm/node');
const facts = {};
for (const { s, bytes, hash } of files) {
	let doc;
	try {
		doc = read(new Uint8Array(bytes));
	} catch (e) {
		fail(`${s.id} does not open with cadkit-wasm ${wasmVersion}: ${e.kind ?? 'error'}: ${e.message}`);
	}
	try {
		const info = doc.info();
		const codes = {};
		for (const w of doc.warnings()) codes[w.code] = (codes[w.code] ?? 0) + 1;
		facts[s.id] = {
			bytes: bytes.length,
			sha256: hash,
			format: info.format,
			version: info.version,
			units: info.units,
			entities: info.models.reduce((n, m) => n + m.entities, 0),
			models: info.models.length,
			layers: info.layerCount,
			warnings: info.warningCount,
			warningCodes: codes,
		};
	} finally {
		doc.free();
	}
}

mkdirSync(outDir, { recursive: true });
const ignoreGen = join(outDir, '.gitignore');
if (!existsSync(ignoreGen)) writeFileSync(ignoreGen, '*\n!.gitignore\n');
writeFileSync(outPath, JSON.stringify({ key, cadkitWasmVersion: wasmVersion, samples: facts }, null, '\t') + '\n');
console.log(`samples: ${files.length} samples read with cadkit-wasm ${wasmVersion}${fetched ? `, ${fetched} fetched` : ''}${built ? `, ${built} generated` : ''}`);
