// The CLI installers served at /install.sh and /install.ps1 stay in step with the release
// workflow (targets, archive names), the site config and each other. They are run for real
// in CI (.github/workflows/ci.yml, job "installer").
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { REPO, SITE, RELEASE_HOST } from '../src/config.ts';

const read = (rel) => readFileSync(new URL(rel, import.meta.url), 'utf8');
const sh = read('../public/install.sh');
const ps1 = read('../public/install.ps1');
const release = read('../../.github/workflows/release.yml');
const releasePy = read('../../scripts/release.py');

/** Targets of the CLI archives: the matrix rows of the release workflow's `cli` job. */
const cliJob = release.slice(release.indexOf('\n  cli:\n'), release.indexOf('\n  capi:\n'));
const targets = [...cliJob.matchAll(/target: ([a-z0-9_-]+)/g)].map((m) => m[1]);

test('the release workflow builds the CLI for the expected platforms', () => {
	assert.equal(targets.length, 5, `CLI targets: ${targets.join(', ')}`);
});

test('every released CLI target is installable', () => {
	for (const target of targets) {
		const script = target.includes('windows') ? ps1 : sh;
		assert.ok(script.includes(target), `${target} is built by release.yml but no installer picks it`);
	}
	// And nothing else: an installer must not ask for an archive that is never built.
	for (const m of sh.matchAll(/echo "([a-z0-9_]+-[a-z]+-[a-z0-9-]+)"/g)) assert.ok(targets.includes(m[1]), m[1]);
});

test('both installers use the mirror the release workflow uploads to, and the repository', () => {
	const mirror = /^MIRROR_URL = "([^"]+)"$/m.exec(releasePy)?.[1];
	assert.ok(mirror, 'MIRROR_URL in scripts/release.py');
	assert.equal(RELEASE_HOST, mirror, 'RELEASE_HOST in src/config.ts');
	for (const [name, script] of [['install.sh', sh], ['install.ps1', ps1]]) {
		assert.ok(script.includes(mirror), `${name} does not download from ${mirror}`);
		assert.ok(script.includes(REPO), `${name} does not fall back to ${REPO}`);
		assert.ok(script.includes(`${SITE}/install.`), `${name} does not show its own URL`);
		assert.ok(script.includes('SHA256SUMS'), `${name} does not verify checksums`);
		for (const v of ['CADKIT_VERSION', 'CADKIT_INSTALL_DIR', 'CADKIT_DOWNLOAD_URL'])
			assert.ok(script.includes(v), `${name} does not honour ${v}`);
	}
});

test('install.sh is LF-only POSIX sh whose last line starts everything', () => {
	assert.ok(sh.startsWith('#!/bin/sh\n'));
	assert.ok(!sh.includes('\r'), 'CRLF would break sh');
	assert.equal(sh.trimEnd().split('\n').at(-1), 'main "$@"', 'a truncated download must not run anything');
	if (process.platform === 'win32') return;
	const check = spawnSync('sh', ['-n', fileURLToPath(new URL('../public/install.sh', import.meta.url))], { encoding: 'utf8' });
	assert.equal(check.status, 0, check.stderr);
});

test('install.ps1 is ASCII (Windows PowerShell 5.1 reads BOM-less files in the ANSI code page)', () => {
	assert.ok(!/[^\x00-\x7f]/.test(ps1));
	assert.ok(!/^\s*exit\b/m.test(ps1), '"exit" would close the terminal when run through iex');
});

test('the site points to the installers', () => {
	const tabs = read('../src/components/InstallTabs.astro');
	assert.ok(tabs.includes('/install.sh') && tabs.includes('/install.ps1'), 'home page install tabs');
	// /install is the short address; it must land on the Install section of the CLI page.
	const target = /^\/install\s+(\S+)\s+302$/m.exec(read('../public/_redirects'))?.[1];
	assert.ok(target, '/install redirect in public/_redirects');
	const [path, anchor] = target.split('#');
	const page = read(`../src/content/docs${path.replace(/\/$/, '')}.mdx`);
	assert.ok(new RegExp(`^## ${anchor}$`, 'im').test(page), `${path} has no "${anchor}" section`);
	assert.ok(page.includes(`${SITE}/install.sh`) && page.includes(`${SITE}/install.ps1`));
});
