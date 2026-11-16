// Runs the JavaScript code samples of the docs and format pages (scripts/check-snippets/docs/js)
// with the installed cadkit-wasm package, and checks that every sample file is shown on a page.
// The DWG and DGN samples need corpus/public/ (scripts/fetch-corpus.py) and are skipped without it.
// Python, Rust, C and CLI samples: scripts/check-snippets/run.sh.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, copyFileSync, readFileSync, readdirSync, rmSync, symlinkSync, existsSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const site = fileURLToPath(new URL('..', import.meta.url));
const repo = join(site, '..');
const docs = join(site, 'scripts/check-snippets/docs');
const corpus = join(repo, 'corpus/public');
const dwgSample = join(corpus, 'acadsharp/sample_AC1032.dwg');
const dgnSample = join(corpus, 'gdal/test_dgnv8.dgn');
const haveCorpus = existsSync(dwgSample) && existsSync(dgnSample);

function fixtureDir() {
	const dir = mkdtempSync(join(tmpdir(), 'cadkit-docs-'));
	copyFileSync(join(site, 'public/samples/cadkit-sample-plan.dxf'), join(dir, 'plan.dxf'));
	copyFileSync(join(site, 'public/samples/cadkit-sample-plan.dxf'), join(dir, 'upload.dxf'));
	copyFileSync(join(docs, 'fixtures/surfaces.dxf'), join(dir, 'surfaces.dxf'));
	copyFileSync(join(docs, 'fixtures/building.gml'), join(dir, 'building.gml'));
	if (haveCorpus) {
		copyFileSync(dwgSample, join(dir, 'plan.dwg'));
		copyFileSync(dgnSample, join(dir, 'plan.dgn'));
		copyFileSync(dgnSample, join(dir, 'template.dgn'));
	}
	symlinkSync(join(site, 'node_modules'), join(dir, 'node_modules'), 'dir');
	return dir;
}

/** Runs one sample in a fresh fixture directory; returns stdout and the directory. */
function run(name, args = []) {
	const dir = fixtureDir();
	copyFileSync(join(docs, 'js', name), join(dir, name));
	try {
		const out = execFileSync(process.execPath, [name, ...args], { cwd: dir, encoding: 'utf8' });
		return { out, file: (f) => readFileSync(join(dir, f)), done: () => rmSync(dir, { recursive: true, force: true }) };
	} catch (e) {
		rmSync(dir, { recursive: true, force: true });
		throw e;
	}
}

const withRun = (name, args, check) => () => {
	const r = run(name, args);
	try {
		check(r);
	} finally {
		r.done();
	}
};

test('quickstart.mjs prints models and writes SVG', withRun('quickstart.mjs', [], ({ out, file }) => {
	assert.match(out, /^dxf AC1032 meter/m);
	assert.match(out, /^Model: 161 entities$/m);
	assert.match(file('plan.svg').toString(), /^<svg[^>]*width="1200"/);
}));

test('dxf-to-svg.mjs writes a transparent 1600 px SVG', withRun('dxf-to-svg.mjs', [], ({ file }) => {
	const svg = file('plan.svg').toString();
	assert.match(svg, /^<svg[^>]*width="1600"/);
	assert.match(svg, /data-layer="WALLS"/);
}));

test('inspect-drawing.mjs lists layers with counts (DXF)', withRun('inspect-drawing.mjs', ['plan.dxf'], ({ out }) => {
	assert.match(out, /format: 'dxf'/);
	assert.match(out, /^WALLS\s+on\s+27 entities$/m);
}));

for (const f of ['plan.dwg', 'plan.dgn']) {
	test(`inspect-drawing.mjs reads ${f}`, { skip: !haveCorpus && 'corpus/public missing' }, withRun('inspect-drawing.mjs', [f], ({ out }) => {
		assert.match(out, f.endsWith('dwg') ? /format: 'dwg'/ : /format: 'dgn_v8'/);
	}));
}

test('write-dgn.mjs writes DGN V8 that reads back', { skip: !haveCorpus && 'corpus/public missing' }, withRun('write-dgn.mjs', [], ({ out, file }) => {
	assert.match(out, /^kept: \d+ named levels/m);
	assert.match(out, /^dgn_v8 161 entities$/m);
	assert.ok(file('converted.dgn').length > 0);
}));

test('citygml.mjs writes CityGML and reports the local preflight', withRun('citygml.mjs', [], ({ out, file }) => {
	assert.match(out, /^6 of 6 entities written as geometry$/m);
	assert.match(out, /^acceptance_verified: false$/m);
	assert.match(out, /tkgm\.missing_building/);
	assert.match(file('model.gml').toString(), /srsName="urn:ogc:def:crs:EPSG::32635"/);
}));

test('untrusted.mjs reads within limits and writes a bounded SVG', withRun('untrusted.mjs', [], ({ out, file }) => {
	assert.doesNotMatch(out, /rejected/);
	assert.match(file('upload.svg').toString(), /^<svg/);
}));

test('read-dxf.mjs writes R2000', withRun('read-dxf.mjs', [], ({ file }) => {
	assert.match(file('plan-r2000.dxf').toString(), /AC1015/);
}));

test('read-citygml.mjs validates the native model', withRun('read-citygml.mjs', [], ({ out }) => {
	assert.match(out, /polygons: 6/);
}));

for (const f of ['read-dwg.mjs', 'read-dgn.mjs']) {
	test(`${f} runs`, { skip: !haveCorpus && 'corpus/public missing' }, withRun(f, [], ({ out }) => {
		assert.ok(out.trim().length > 0);
	}));
}

test('browser samples are valid modules', () => {
	for (const f of ['browser-quickstart.js', 'viewer/worker.js', 'viewer/main.js']) {
		execFileSync(process.execPath, ['--check', join(docs, 'js', f)]);
	}
});

test('every sample file is shown on a page, and every imported sample exists', () => {
	const pages = [];
	const walk = (dir) => {
		for (const name of readdirSync(dir)) {
			const p = join(dir, name);
			if (statSync(p).isDirectory()) walk(p);
			else if (/\.(mdx|astro)$/.test(name)) pages.push(p);
		}
	};
	walk(join(site, 'src/content/docs'));
	walk(join(site, 'src/pages/formats'));
	const used = new Set();
	for (const page of pages) {
		for (const m of readFileSync(page, 'utf8').matchAll(/['"][^'"]*check-snippets\/docs\/([^'"?]+)\?raw['"]/g)) {
			assert.ok(existsSync(join(docs, m[1])), `${relative(site, page)} imports missing ${m[1]}`);
			used.add(m[1]);
		}
	}
	const files = [];
	const list = (dir) => {
		for (const name of readdirSync(dir)) {
			const p = join(dir, name);
			if (statSync(p).isDirectory()) list(p);
			else files.push(relative(docs, p).split(sep).join('/'));
		}
	};
	list(docs);
	const unused = files.filter((f) => !f.startsWith('fixtures/') && !used.has(f));
	assert.deepEqual(unused, [], 'sample files not shown on any page');
});
