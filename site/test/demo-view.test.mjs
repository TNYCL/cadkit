// Pure helpers of the /demo/ viewer: pan/zoom math and reader-facing text.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as V from '../src/demo/view.ts';
import { cleanMessage, explainError, extensionHint, formatBytes, formatMs, formatWithVersion, plural, sourceTypeName, unitName, unitSymbol, versionLabel } from '../src/demo/text.ts';
import {
	DXF_CODEPAGES,
	DXF_VERSIONS,
	LEGACY_DXF_VERSIONS,
	MAX_ENTITY_JSON_BYTES,
	MAX_FILE_BYTES,
	MAX_HIGHLIGHT_BYTES,
	MAX_PICK_BYTES,
	MAX_SVG_BYTES,
	TARGET_UNITS,
	TKGM_PROFILES,
} from '../src/demo/protocol.ts';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const load = createRequire(import.meta.url);
const site = fileURLToPath(new URL('..', import.meta.url));

const close = (a, b, eps = 1e-9) => assert.ok(Math.abs(a - b) <= eps * Math.max(1, Math.abs(a), Math.abs(b)), `${a} != ${b}`);

test('parseViewBox accepts valid boxes and rejects the rest', () => {
	assert.deepEqual(V.parseViewBox('-2.5 -10 16 14'), { x: -2.5, y: -10, w: 16, h: 14 });
	assert.deepEqual(V.parseViewBox('1,2,3,4'), { x: 1, y: 2, w: 3, h: 4 });
	for (const bad of [null, '', '0 0 0 1', '0 0 1', 'a b c d', '0 0 -1 1']) assert.deepEqual(V.parseViewBox(bad), { x: 0, y: 0, w: 1, h: 1 });
});

test('fit centres the drawing and keeps the margin', () => {
	const home = { x: 0, y: -9, w: 12, h: 9 };
	const v = V.fit(home, 800, 600);
	close(v.cx, 6);
	close(v.cy, -4.5);
	const [x, y, w, h] = V.viewBoxOf(v, 800, 600).split(' ').map(Number);
	assert.ok(x <= home.x && y <= home.y && x + w >= home.x + home.w && y + h >= home.y + home.h);
	close(w / h, 800 / 600);
});

test('fitBox keeps the drawing clear of the controls and can start on a part of it', () => {
	const home = { x: 0, y: -9, w: 12, h: 9 };
	// No insets: the same view as fit().
	assert.deepEqual(V.fitBox(home, home, 800, 600), V.fit(home, 800, 600));
	// A tool column on the right and a note on top: the drawing is centred in what is left.
	const insets = { left: 0, top: 60, right: 100, bottom: 0 };
	const v = V.fitBox(home, home, 800, 600, insets);
	const [x, y, w, h] = V.viewBoxOf(v, 800, 600).split(' ').map(Number);
	const px = (wx) => ((wx - x) / w) * 800;
	const py = (wy) => ((wy - y) / h) * 600;
	assert.ok(px(home.x) >= 0 && px(home.x + home.w) <= 700 + 1e-9);
	assert.ok(py(home.y) >= 60 - 1e-9 && py(home.y + home.h) <= 600);
	close(px(home.x + home.w / 2), 350);
	close(py(home.y + home.h / 2), 330);
	// A part of the drawing: that part fills the stage, the zoom range still follows the whole.
	const part = V.fitBox(home, { x: 2, y: -4, w: 3, h: 2 }, 800, 600);
	assert.equal(part.home, home);
	assert.ok(part.s < V.fit(home, 800, 600).s);
	close(part.cx, 3.5);
	close(part.cy, -3);
});

test('world coordinates flip Y and follow pan', () => {
	const v = V.fit({ x: 0, y: -10, w: 10, h: 10 }, 100, 100);
	const c = V.toWorld(v, 100, 100, 50, 50);
	close(c.x, 5);
	close(c.y, 5);
	const moved = V.pan(v, 10, 0);
	const c2 = V.toWorld(moved, 100, 100, 60, 50);
	close(c2.x, 5);
	close(c2.y, 5);
});

test('zoomAt keeps the point under the cursor and clamps the range', () => {
	const v = V.fit({ x: 0, y: -10, w: 10, h: 10 }, 200, 100);
	const before = V.toWorld(v, 200, 100, 30, 70);
	const z = V.zoomAt(v, 200, 100, 3, 30, 70);
	const after = V.toWorld(z, 200, 100, 30, 70);
	close(before.x, after.x);
	close(before.y, after.y);
	close(z.s, v.s / 3);
	let far = v;
	for (let i = 0; i < 200; i++) far = V.zoomAt(far, 200, 100, 10);
	close(far.s, V.fitScale(v.home, 200, 100) / V.MAX_ZOOM_IN);
	let out = v;
	for (let i = 0; i < 200; i++) out = V.zoomAt(out, 200, 100, 0.1);
	close(out.s, V.fitScale(v.home, 200, 100) * V.MAX_ZOOM_OUT);
});

test('coordinate readout precision follows the scale', () => {
	assert.equal(V.formatCoord(1.23456, 0.01), '1.23');
	assert.equal(V.formatCoord(1234.5, 10), '1235');
	assert.equal(V.formatCoord(-0.0001, 0.01), '0.00');
	assert.equal(V.decimalsFor(0), 3);
});

test('errors: every cadkit error kind has text, unknown kinds fall back', () => {
	for (const kind of ['unknown_format', 'unsupported', 'invalid', 'truncated', 'limit_exceeded', 'io', 'bad_argument', 'trap', 'too_large', 'unknown_sample', 'network', 'cancelled']) {
		const e = explainError(kind);
		assert.ok(e.title && e.hint, kind);
		assert.notEqual(e, explainError('internal'), kind);
	}
	assert.equal(explainError('something_new'), explainError('internal'));
});

test('format, version, unit and size labels', () => {
	assert.equal(formatWithVersion('dxf', 'AC1032'), 'DXF AC1032 (2018)');
	assert.equal(formatWithVersion('dwg', 'AC1014'), 'DWG AC1014 (R14)');
	assert.equal(formatWithVersion('dgn_v8', 'V8'), 'DGN V8');
	assert.equal(formatWithVersion('city_gml', '2.0'), 'CityGML 2.0');
	assert.equal(versionLabel('dgn_v7', 'V7'), 'V7');
	assert.equal(unitSymbol('meter'), 'm');
	assert.equal(unitSymbol('unitless'), '');
	assert.equal(unitSymbol('us_survey_foot'), 'US ft');
	assert.equal(unitSymbol('light_year'), 'light year');
	assert.equal(unitName('us_survey_foot'), 'US survey foot');
	assert.equal(formatBytes(512), '512 B');
	assert.equal(formatBytes(32076), '31.3 KB');
	assert.equal(formatBytes(MAX_FILE_BYTES), '64.0 MiB');
	assert.equal(plural(1, 'entity', 'entities'), '1 entity');
	assert.equal(plural(1200, 'layer'), '1,200 layers');
});

test('error details and small labels', () => {
	assert.equal(cleanMessage('limit_exceeded', 'limit exceeded: more than 10 entities'), 'more than 10 entities');
	assert.equal(cleanMessage('unsupported', 'unsupported: unsupported: UTF-8 DXF output requires R2007 or newer'), 'UTF-8 DXF output requires R2007 or newer');
	assert.equal(cleanMessage('invalid', 'invalid data at offset 12: page map'), 'invalid data at offset 12: page map');
	assert.equal(cleanMessage('', 'anything'), 'anything');
	assert.match(extensionHint('site.DGN'), /ends in \.dgn.*DGN file/);
	assert.match(extensionHint('model.gml'), /CityGML/);
	assert.equal(extensionHint('notes.txt'), '');
	assert.equal(extensionHint('README'), '');
	assert.equal(sourceTypeName('dwg.MULTILEADER'), 'MULTILEADER');
	assert.equal(sourceTypeName('type_36'), 'type_36');
	assert.equal(formatMs(0.42), '0.4 ms');
	assert.equal(formatMs(12.4), '12 ms');
	assert.equal(formatMs(1240), '1.24 s');
	assert.equal(formatMs(NaN), '-');
});

test('limits stay within what cadkit-wasm accepts', () => {
	assert.ok(MAX_PICK_BYTES > 0 && MAX_PICK_BYTES <= 65536);
	assert.ok(MAX_FILE_BYTES <= 512 * 1024 * 1024);
	assert.ok(MAX_HIGHLIGHT_BYTES <= MAX_SVG_BYTES);
	assert.ok(MAX_ENTITY_JSON_BYTES >= 16 * 1024);
});

test('the export and conversion choices are ones cadkit-wasm accepts', () => {
	const { read, readCityGml } = load('cadkit-wasm/node');
	const doc = read(new Uint8Array(readFileSync(site + 'public/samples/cadkit-sample-plan.dxf')));
	try {
		for (const version of DXF_VERSIONS) {
			const legacy = LEGACY_DXF_VERSIONS.includes(version);
			assert.ok(doc.toDxfBytes(version, null).byteLength > 1000, version);
			for (const cp of DXF_CODEPAGES.filter(Boolean)) {
				// Code pages are for the versions that predate UTF-8 DXF; newer ones refuse them.
				if (legacy) assert.ok(doc.toDxfBytes(version, cp).byteLength > 1000, `${version} ${cp}`);
				else assert.throws(() => doc.toDxfBytes(version, cp), (e) => e.kind === 'unsupported', `${version} ${cp}`);
			}
		}
		// Converting changes the document, so walk the units on one copy and end where it began.
		for (const unit of [...TARGET_UNITS, 'meter']) assert.ok(doc.convertUnits(unit) > 0, unit);
		assert.equal(doc.info().units, 'meter');
	} finally {
		doc.free();
	}
	const city = readCityGml(new Uint8Array(readFileSync(site + 'public/samples/cadkit-sample-buildings.gml')));
	try {
		for (const profile of TKGM_PROFILES) assert.equal(city.preflightTkgm(profile).acceptance_verified, false, profile);
		assert.throws(() => city.preflightTkgm('nope'));
	} finally {
		city.free();
	}
});
