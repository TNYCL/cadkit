// Runs the JavaScript snippet shown on the landing page ("same task in four languages")
// against the committed sample plan with the installed cadkit-wasm package.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, copyFileSync, readFileSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const site = fileURLToPath(new URL('..', import.meta.url));

test('JavaScript snippet lists the layers and writes SVG', () => {
	const dir = mkdtempSync(join(tmpdir(), 'cadkit-snippet-'));
	try {
		copyFileSync(join(site, 'public/samples/cadkit-sample-plan.dxf'), join(dir, 'cadkit-sample-plan.dxf'));
		copyFileSync(join(site, 'scripts/check-snippets/snippets/list-layers.cjs'), join(dir, 'list-layers.cjs'));
		symlinkSync(join(site, 'node_modules'), join(dir, 'node_modules'), 'dir');
		const out = execFileSync(process.execPath, ['list-layers.cjs'], { cwd: dir, encoding: 'utf8' });
		const layers = out.trim().split('\n');
		for (const name of ['WALLS', 'DOORS', 'WINDOWS', 'DIMENSIONS', 'TEXT', 'GRID']) assert.ok(layers.includes(name), `missing layer ${name}`);
		const svg = readFileSync(join(dir, 'plan.svg'), 'utf8');
		assert.match(svg, /^<svg[^>]*width="1200"/);
		assert.match(svg, /data-layer="WALLS"/);
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
});
