// Pure helpers of the /demo/ viewer: pan/zoom math and reader-facing text.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as V from '../src/demo/view.ts';
import { explainError, formatBytes, formatWithVersion, plural, unitName, unitSymbol, versionLabel } from '../src/demo/text.ts';
import { MAX_FILE_BYTES, MAX_PICK_BYTES } from '../src/demo/protocol.ts';

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

test('limits stay within what cadkit-wasm accepts', () => {
	assert.ok(MAX_PICK_BYTES > 0 && MAX_PICK_BYTES <= 65536);
	assert.ok(MAX_FILE_BYTES <= 512 * 1024 * 1024);
});
