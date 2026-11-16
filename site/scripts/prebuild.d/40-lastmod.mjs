#!/usr/bin/env node
// Writes src/generated/lastmod.json: site path -> date of the last commit that changed the
// page's content sources. The sitemap (<lastmod>) and the TechArticle JSON-LD (dateModified)
// read it, so both state the same, real modification date.
//
// Sources per page:
//   - synced docs (scripts/docs/manifest.mjs): the repository file they are generated from;
//   - hand-written docs (src/content/docs/**): the file plus the code samples it imports;
//   - hand-written pages (src/pages/**/*.astro): the file plus imported data and "_" partials.
// Site chrome (layouts, components, styles, src/config.ts) is deliberately not a source.
//
// A date is only as good as the history it is read from. In a shallow clone every file looks
// changed by the newest commit, so CI checkouts are deepened first; when the history is still
// incomplete (or there is no git checkout at all) the manifest is empty and the pages are
// published without a date, which search engines prefer to a wrong one.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { DOCS, SCHEMA, GENERATED_DIRS } from '../docs/manifest.mjs';
import { routeOf, relativeImports, isContentSource, toUtcSeconds } from '../lib/lastmod.mjs';

const site = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const repoRoot = join(site, '..');
const outPath = join(site, 'src', 'generated', 'lastmod.json');
const posixPath = (p) => p.split(sep).join('/');
const repoPath = (abs) => posixPath(relative(repoRoot, abs));

const git = (...args) => {
	try {
		return execFileSync('git', ['-C', repoRoot, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], timeout: 120_000 }).trim();
	} catch {
		return undefined;
	}
};

/** @param {string} dir @param {(abs: string) => boolean} keep @returns {string[]} */
function walk(dir, keep) {
	const out = [];
	if (!existsSync(dir)) return out;
	for (const name of readdirSync(dir).sort()) {
		const abs = join(dir, name);
		if (statSync(abs).isDirectory()) out.push(...walk(abs, keep));
		else if (keep(abs)) out.push(abs);
	}
	return out;
}

/** The file and, transitively, the content sources it imports (repository paths). */
function contentSources(entry) {
	const seen = new Set();
	const visit = (abs) => {
		const key = repoPath(abs);
		if (seen.has(key)) return;
		seen.add(key);
		if (!/\.(astro|mdx?|ts|mjs)$/.test(abs)) return;
		for (const spec of relativeImports(readFileSync(abs, 'utf8'))) {
			const base = join(dirname(abs), spec);
			const target = [base, base + '.ts', base + '.mjs', base + '.astro'].find((c) => existsSync(c) && statSync(c).isFile());
			if (target && isContentSource(repoPath(target))) visit(target);
		}
	};
	visit(entry);
	return [...seen];
}

/** @type {Map<string, string[]>} site path -> repository paths */
const sources = new Map();
for (const doc of DOCS) sources.set(doc.route, [doc.source]);
sources.set(SCHEMA.route, [SCHEMA.source]);

const contentRoot = join(site, 'src', 'content', 'docs');
const generated = GENERATED_DIRS.map((d) => join(contentRoot, d) + sep);
for (const abs of walk(contentRoot, (f) => /\.mdx?$/.test(f) && !generated.some((g) => f.startsWith(g)))) {
	sources.set(routeOf(posixPath(relative(contentRoot, abs))), contentSources(abs));
}
const pagesRoot = join(site, 'src', 'pages');
for (const abs of walk(pagesRoot, (f) => f.endsWith('.astro') && !/(^|[\\/])(_[^\\/]*|404\.astro)$/.test(f))) {
	sources.set(routeOf(posixPath(relative(pagesRoot, abs))), contentSources(abs));
}

let reason;
if (git('rev-parse', '--is-inside-work-tree') !== 'true') reason = 'not a git checkout';
else if (git('rev-parse', '--is-shallow-repository') === 'true') {
	// CI checkouts (GitHub Actions, Cloudflare builds) are shallow; the full history is small.
	if (process.env.CI || process.env.CF_PAGES || process.env.WORKERS_CI) git('fetch', '--unshallow', '--quiet');
	if (git('rev-parse', '--is-shallow-repository') !== 'false') reason = 'shallow git history';
}

/** @type {Record<string, string>} */
const lastmod = {};
if (!reason) {
	for (const route of [...sources.keys()].sort()) {
		const date = toUtcSeconds(git('log', '-1', '--format=%cI', '--', ...sources.get(route)) ?? '');
		if (date) lastmod[route] = date;
	}
}

const text = JSON.stringify(lastmod, null, '\t') + '\n';
if (!existsSync(outPath) || readFileSync(outPath, 'utf8') !== text) {
	mkdirSync(dirname(outPath), { recursive: true });
	writeFileSync(outPath, text);
}
console.log(
	reason
		? `lastmod: ${reason}; pages are published without a modification date`
		: `lastmod: ${Object.keys(lastmod).length} of ${sources.size} pages dated from git history`
);
