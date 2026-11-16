// The /brand/ download list matches the generated logo files.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { BRAND_ASSETS } from '../src/pages/brand/_assets.ts';

const pub = new URL('../public/', import.meta.url);

function size(href, format) {
	const buf = readFileSync(new URL('.' + href, pub));
	if (format === 'PNG') return [buf.readUInt32BE(16), buf.readUInt32BE(20)];
	if (format === 'ICO') {
		// Largest image in the directory (0 means 256).
		const n = buf.readUInt16LE(4);
		let best = 0;
		for (let i = 0; i < n; i++) best = Math.max(best, buf.readUInt8(6 + 16 * i) || 256);
		return [best, best];
	}
	const text = buf.toString('utf8');
	const w = /<svg[^>]*\swidth="(\d+)"/.exec(text);
	const vb = /viewBox="0 0 (\d+) (\d+)"/.exec(text);
	return w ? [Number(w[1]), Number(/<svg[^>]*\sheight="(\d+)"/.exec(text)[1])] : [Number(vb[1]), Number(vb[2])];
}

test('every brand download exists with the listed size', () => {
	const seen = new Set();
	for (const a of BRAND_ASSETS) {
		for (const f of [a.preview, ...a.files]) {
			assert.ok(existsSync(new URL('.' + f.href, pub)), `${f.href} missing`);
			assert.deepEqual(size(f.href, f.format), [f.width, f.height], `${f.href} size`);
			seen.add(f.href);
		}
	}
	// Every generated brand file is offered.
	for (const name of ['cadkit-mark.svg', 'cadkit-mark-dark.svg', 'cadkit-mark-mono.svg', 'cadkit-lockup.svg', 'cadkit-lockup-dark.svg', 'cadkit-lockup-mono.svg', 'cadkit-tile.svg', 'cadkit-avatar-512.png', 'cadkit-icon-256.png', 'cadkit-favicon-32.png'])
		assert.ok(seen.has('/brand/' + name), `/brand/${name} not offered on /brand/`);
});
