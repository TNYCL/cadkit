// Generated, committed assets must match their generators (deterministic outputs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { ogImagePath } from '../src/lib/og-path.mjs';
import { WEIGHTS, markElements } from '../src/lib/brand-mark.mjs';
import { inlineScripts, metaContent, urlPathOf } from '../scripts/postbuild.mjs';

const site = fileURLToPath(new URL('..', import.meta.url));
const run = (script, ...args) => execFileSync(process.execPath, [script, ...args], { cwd: site, encoding: 'utf8' });

test('sample plan DXF is up to date with scripts/samples/plan.mjs', () => {
	assert.match(run('scripts/samples/plan.mjs', '--check'), /up to date/);
});

test('brand SVGs are up to date with scripts/brand.mjs', () => {
	assert.match(run('scripts/brand.mjs', '--check'), /up to date/);
});

test('brand centre mark cannot read as a plus sign', () => {
	for (const [name, w] of Object.entries(WEIGHTS)) {
		// A centre point, never a stroke through the centre.
		assert.ok(w.dot > 0 && w.dot < w.stroke, `${name}: centre point smaller than the arc stroke`);
		if (!w.dashes) continue;
		// Dashes start clear of the centre point (open gap) and are thinner than the arc.
		assert.ok(w.dashes.from - w.dot >= 1.5, `${name}: gap between centre point and dashes`);
		assert.ok(w.dashes.width < w.stroke / 1.5, `${name}: dashes thinner than the arc`);
		// Dashes end inside the arc and clear of the midpoint grip.
		const gripInner = 7 * Math.SQRT2 - w.grip / 2;
		assert.ok(w.dashes.to <= gripInner - 1, `${name}: dashes clear of the midpoint grip`);
	}
	assert.equal(WEIGHTS.icon.dashes, null, 'favicon weight keeps only the centre point');
	// The centre mark uses the arc colour, not the grip accent.
	const svg = markElements({ arc: 'ARC', grip: 'GRIP' });
	assert.match(svg, /<circle cx="16" cy="16" r="[\d.]+" fill="ARC"\/>/);
});

test('OG image paths', () => {
	assert.equal(ogImagePath('/'), '/og/index.png');
	assert.equal(ogImagePath('/docs/'), '/og/docs.png');
	assert.equal(ogImagePath('/docs/getting-started/rust/'), '/og/docs/getting-started/rust.png');
});

test('post-build helpers', () => {
	const html =
		'<meta property="og:title" content="A &amp; B"><script>a()</script><script type="module">b()</script>' +
		'<script type="application/ld+json">{}</script><script src="/x.js"></script>';
	assert.equal(metaContent(html, 'property', 'og:title'), 'A & B');
	assert.deepEqual(inlineScripts(html), ['a()', 'b()']);
	assert.equal(urlPathOf('/d', '/d/index.html'), '/');
	assert.equal(urlPathOf('/d', '/d/docs/x/index.html'), '/docs/x/');
	assert.equal(urlPathOf('/d', '/d/404.html'), '/404.html');
});
