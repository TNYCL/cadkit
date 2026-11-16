// Demo samples: the manifest, the generated CityGML sample, the third-party files and their
// licences. Every sample must open with the installed cadkit-wasm (Node build). Third-party files
// are fetched by scripts/prebuild.d/10-samples.mjs; when they are not there yet (fresh checkout,
// tests run before any build) their tests are skipped here, but the build itself requires them.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { SAMPLES, rawUrl, blobUrl, licenseHref, sampleById } from '../src/demo/samples.ts';

const require = createRequire(import.meta.url);
const site = fileURLToPath(new URL('..', import.meta.url));
const { read, readCityGml } = require('cadkit-wasm/node');

const EXPECTED_FORMAT = {
	'plan-dxf': 'dxf',
	'citygml-building': 'city_gml',
	'dwg-acadsharp': 'dwg',
	'dgn-v8-gdal': 'dgn_v8',
	'dgn-v7-gdal': 'dgn_v7',
};

test('manifest: the required sample ids, unique, with public paths', () => {
	assert.deepEqual(SAMPLES.map((s) => s.id).sort(), Object.keys(EXPECTED_FORMAT).sort());
	for (const s of SAMPLES) {
		assert.match(s.path, /^\/samples\/[\w./-]+$/, s.id);
		assert.equal(sampleById(s.id), s);
		assert.ok(s.description.length > 20, s.id);
		if (s.upstream) {
			assert.match(s.path, /^\/samples\/third-party\//, `${s.id} must live in the git-ignored folder`);
			assert.equal(s.license, 'MIT', `${s.id}: only MIT third-party samples`);
		} else {
			assert.ok(s.generator && existsSync(join(site, s.generator)), `${s.id}: generator script`);
			assert.equal(s.license, 'MIT OR Apache-2.0');
		}
	}
	assert.equal(sampleById('nope'), undefined);
});

test('manifest: third-party files are pinned to a commit and a SHA-256', () => {
	for (const s of SAMPLES.filter((x) => x.upstream)) {
		const u = s.upstream;
		assert.match(u.commit, /^[0-9a-f]{40}$/, s.id);
		assert.match(u.sha256, /^[0-9a-f]{64}$/, s.id);
		assert.ok(Number.isInteger(u.bytes) && u.bytes > 0, s.id);
		assert.ok(rawUrl(u).startsWith(`https://raw.githubusercontent.com/${u.repo}/${u.commit}/`), s.id);
		assert.ok(blobUrl(u).includes(`/blob/${u.commit}/`), s.id);
		assert.match(licenseHref(u), /^\/demo\/samples\/licenses\/[a-z0-9-]+\.txt$/);
		assert.ok(!/safe\.com|corpus\/private/i.test(rawUrl(u)), s.id);
	}
});

test('licence copies name their pinned source and contain the licence grant', () => {
	for (const s of SAMPLES.filter((x) => x.upstream)) {
		const u = s.upstream;
		const text = readFileSync(join(site, u.licenseCopy), 'utf8');
		assert.ok(text.startsWith(`Source: ${blobUrl(u, u.licensePath)}\n`), `${u.licenseCopy} source line`);
		assert.match(text, /Permission is hereby granted, free of charge/);
	}
});

test('CityGML sample is up to date with scripts/samples/buildings.mjs', () => {
	const out = execFileSync(process.execPath, ['scripts/samples/buildings.mjs', '--check'], { cwd: site, encoding: 'utf8' });
	assert.match(out, /up to date/);
});

test('CityGML sample: explicit compound CRS, two LoD2 solids that pass cadkit validation', () => {
	const bytes = readFileSync(join(site, 'public/samples/cadkit-sample-buildings.gml'));
	const xml = bytes.toString('utf8');
	assert.match(xml, /srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837"/);
	assert.equal((xml.match(/<bldg:Building /g) ?? []).length, 2);
	assert.equal((xml.match(/<bldg:lod2Solid>/g) ?? []).length, 2);
	const native = readCityGml(new Uint8Array(bytes));
	try {
		const report = native.validate({ geometry: true });
		assert.equal(report.polygons, 13);
	} finally {
		native.free();
	}
});

for (const s of SAMPLES) {
	const path = join(site, 'public', s.path);
	const missing = s.upstream && !existsSync(path);
	test(`sample ${s.id} opens with cadkit-wasm`, { skip: missing ? 'not fetched yet (run the prebuild step)' : false }, () => {
		const bytes = readFileSync(path);
		if (s.upstream) {
			assert.equal(createHash('sha256').update(bytes).digest('hex'), s.upstream.sha256);
			assert.equal(bytes.length, s.upstream.bytes);
		}
		const doc = read(new Uint8Array(bytes));
		try {
			const info = doc.info();
			assert.equal(info.format, EXPECTED_FORMAT[s.id]);
			assert.ok(info.models.some((m) => m.entities > 0), 'has entities');
			const model = info.models.findIndex((m) => m.entities > 0);
			const svg = doc.toSvgBounded({ model, maxOutputBytes: 32 * 1024 * 1024 });
			assert.match(svg, /^<svg[^>]*viewBox="[-\d.e]+ [-\d.e]+ [\d.e]+ [\d.e]+"/);
			assert.match(svg, /data-layer="/);
			for (const w of doc.warnings()) assert.match(w.code, /^[a-z0-9_]+(\.[a-z0-9_]+)+$/, 'stable warning code');
		} finally {
			doc.free();
		}
	});
}
