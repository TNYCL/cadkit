// WCAG 2.2 AA contrast for every text/background pair of the design tokens.
// Tokens are parsed from src/styles/tokens.css, the single source of colour values.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const css = readFileSync(new URL('../src/styles/tokens.css', import.meta.url), 'utf8');

function block(selector) {
	const start = css.indexOf(selector + ' {');
	assert.notEqual(start, -1, `selector ${selector} not found`);
	const body = css.slice(start, css.indexOf('}', start));
	return Object.fromEntries([...body.matchAll(/--ck-([a-z-]+):\s*(#[0-9a-f]{6});/gi)].map((m) => [m[1], m[2]]));
}

const themes = { dark: block(':root'), light: block(":root[data-theme='light']") };

const lum = (hex) => {
	const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
	const l = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
	return 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
};
export const contrast = (a, b) => {
	const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
	return (x + 0.05) / (y + 0.05);
};

const SURFACES = ['bg', 'surface', 'raised'];
/** Text-like foregrounds used on every surface: body text, secondary text, links. */
const TEXT = ['text', 'muted', 'accent'];
/**
 * Status chip colours are used on bg and surface only. Light "ok" (#1a7f4b) on raised (#f2f2ec)
 * is 4.47:1, below AA, so chips must not be placed on raised surfaces (code blocks, table heads).
 */
const STATUS = ['ok', 'partial', 'pending'];
const STATUS_SURFACES = ['bg', 'surface'];

for (const [name, t] of Object.entries(themes)) {
	test(`${name}: all tokens defined`, () => {
		for (const k of [...SURFACES, ...TEXT, ...STATUS, 'border', 'grid-minor', 'grid-major', 'on-accent', 'redline'])
			assert.ok(t[k], `--ck-${k} missing in ${name}`);
	});

	for (const fg of TEXT)
		for (const bg of SURFACES)
			test(`${name}: ${fg} on ${bg} >= 4.5:1`, () => {
				const r = contrast(t[fg], t[bg]);
				assert.ok(r >= 4.5, `${fg} ${t[fg]} on ${bg} ${t[bg]} is ${r.toFixed(2)}:1`);
			});

	for (const fg of STATUS)
		for (const bg of STATUS_SURFACES)
			test(`${name}: status ${fg} on ${bg} >= 4.5:1`, () => {
				const r = contrast(t[fg], t[bg]);
				assert.ok(r >= 4.5, `${fg} ${t[fg]} on ${bg} ${t[bg]} is ${r.toFixed(2)}:1`);
			});

	test(`${name}: on-accent on accent >= 4.5:1 (primary button)`, () => {
		assert.ok(contrast(t['on-accent'], t.accent) >= 4.5);
	});

	// Redline is decoration only (dimension lines), never body text: non-text contrast 3:1.
	for (const bg of SURFACES)
		test(`${name}: redline on ${bg} >= 3:1 (graphics)`, () => {
			assert.ok(contrast(t.redline, t[bg]) >= 3, `redline ${contrast(t.redline, t[bg]).toFixed(2)}:1`);
		});

	// Focus ring (accent) must stand out from the page: WCAG 2.4.13 non-text 3:1.
	test(`${name}: focus ring (accent) on bg >= 3:1`, () => {
		assert.ok(contrast(t.accent, t.bg) >= 3);
	});
}
