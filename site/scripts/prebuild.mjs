// Runs every build-time generation step before dev, check and build.
// Each step is a standalone Node script; steps in scripts/prebuild.d/ run in file-name order,
// then the hero render. A failing step stops the build (no silent fallbacks).

import { readdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const dir = join(here, 'prebuild.d');
const steps = existsSync(dir)
	? readdirSync(dir)
			.filter((f) => f.endsWith('.mjs'))
			.sort()
			.map((f) => join(dir, f))
	: [];
steps.push(join(here, 'hero.mjs'));

for (const step of steps) {
	const result = spawnSync(process.execPath, [step], { stdio: 'inherit' });
	if (result.status !== 0) {
		console.error(`prebuild: ${step} failed with exit code ${result.status}`);
		process.exit(result.status ?? 1);
	}
}
