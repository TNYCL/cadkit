// The docs sync (scripts/prebuild.d/20-docs.mjs): Markdown transforms on synthetic input, the
// manifest against the repository, and the rules for hand-written docs pages (frontmatter,
// description length, the project's wording rules, which verify.mjs skips under /docs/).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { extractTitle, firstSentence, rewriteLinks, makeResolver, frontmatter, tagLines } from '../scripts/docs/transform.mjs';
import { DOCS, SCHEMA, GENERATED_DIRS } from '../scripts/docs/manifest.mjs';
import { schemaPage, typeOf } from '../scripts/docs/schema-page.mjs';

const site = fileURLToPath(new URL('..', import.meta.url));
const repo = join(site, '..');
const content = join(site, 'src/content/docs');
const REPO = 'https://github.com/example/cadkit';
const SITE = 'https://example.org';

test('extractTitle takes the first H1 outside code and removes it', () => {
	const md = '```sh\n# not a title\n```\n\n# The `cadkit` title\n\nBody.\n';
	const { title, body } = extractTitle(md);
	assert.equal(title, 'The cadkit title');
	assert.equal(body, '```sh\n# not a title\n```\n\nBody.\n');
	assert.throws(() => extractTitle('## only h2\n'));
});

test('tagLines tracks fenced code', () => {
	const tags = tagLines('a\n````md\n```\nstill code\n````\nb');
	assert.deepEqual(tags.map((t) => t.code), [false, true, true, true, true, false]);
});

test('firstSentence skips structure and strips inline Markdown', () => {
	const body = '## Heading\n\n| a | b |\n\nThe [`cadkit`](x.md) reader, e.g. the **DGN** one. Second sentence.\n';
	assert.equal(firstSentence(body), 'The cadkit reader, e.g. the DGN one.');
	const long = 'word '.repeat(60) + 'end.';
	const cut = firstSentence(long);
	assert.ok(cut.length <= 160 && cut.endsWith('…'));
});

test('rewriteLinks maps published targets, falls back to GitHub and keeps code', () => {
	const errors = [];
	const routes = new Map([['docs/python.md', '/docs/reference/python/'], ['CHANGELOG.md', '/changelog/']]);
	const files = new Set(['docs/python.md', 'CHANGELOG.md', 'SECURITY.md', 'docs/assets/a.png']);
	const exists = (p) => (files.has(p) ? 'file' : p === 'crates' ? 'dir' : undefined);
	const resolve = makeResolver({ source: 'docs/wasm.md', repo: REPO, site: SITE, routes, aliases: { 'README.md#format-support': '/formats/' }, exists, errors });
	const md = [
		'See [Python](python.md#install), [log](../CHANGELOG.md#migrating-from-04), [sec](../SECURITY.md).',
		`Also [matrix](../README.md#format-support), [crates](../crates/), [abs](${REPO}/blob/main/docs/python.md), [home](${SITE}/demo/).`,
		'![logo](assets/a.png) and `[not](python.md)` and [anchor](#here).',
		'```md',
		'[inside](python.md)',
		'```',
		'[ref]: python.md',
	].join('\n');
	const out = rewriteLinks(md, resolve).split('\n');
	assert.equal(out[0], `See [Python](/docs/reference/python/#install), [log](/changelog/#migrating-from-04), [sec](${REPO}/blob/main/SECURITY.md).`);
	assert.equal(out[1], `Also [matrix](/formats/), [crates](${REPO}/tree/main/crates), [abs](/docs/reference/python/), [home](/demo/).`);
	assert.equal(out[2], `![logo](${REPO}/raw/main/docs/assets/a.png) and \`[not](python.md)\` and [anchor](#here).`);
	assert.equal(out[4], '[inside](python.md)');
	assert.equal(out[6], '[ref]: /docs/reference/python/');
	assert.deepEqual(errors, []);
	rewriteLinks('[gone](missing.md)', resolve);
	assert.equal(errors.length, 1);
});

test('frontmatter quotes strings and nests objects', () => {
	assert.equal(frontmatter({ title: 'A "b"', prev: { link: '/x/', label: 'X' }, skip: undefined }), '---\ntitle: "A \\"b\\""\nprev:\n  link: "/x/"\n  label: "X"\n---\n');
});

test('schema page renders every definition with links', () => {
	const schema = JSON.parse(readFileSync(join(repo, SCHEMA.source), 'utf8'));
	const page = schemaPage(schema, { raw: SCHEMA.raw, source: 's', sourcePath: SCHEMA.source, python: '/p/', javascript: '/j/', capi: '/c/', architecture: '/a/', changelog: '/l/' });
	for (const name of Object.keys(schema.$defs)) assert.match(page, new RegExp(`^### ${name}$`, 'm'));
	assert.equal(typeOf({ type: 'array', items: { $ref: '#/$defs/Point3' } }), 'array of [`Point3`](#point3)');
	assert.equal(typeOf({ type: ['integer', 'null'], format: 'uint64' }), 'integer (uint64) or null');
});

test('manifest sources exist and routes are unique', () => {
	const routes = new Set();
	for (const d of DOCS) {
		assert.ok(existsSync(join(repo, d.source)), d.source);
		assert.ok(d.route.endsWith('/') && !routes.has(d.route), d.route);
		routes.add(d.route);
		assert.ok(GENERATED_DIRS.some((g) => d.out.startsWith(g + '/')), `${d.out} outside the generated directories`);
		if (d.description) assert.ok(d.description.length >= 50 && d.description.length <= 160, d.source);
	}
});

test('the sync step runs and produces every page with frontmatter', () => {
	execFileSync(process.execPath, [join(site, 'scripts/prebuild.d/20-docs.mjs')], { encoding: 'utf8' });
	for (const out of [...DOCS.map((d) => d.out), SCHEMA.out]) {
		const text = readFileSync(join(content, out), 'utf8');
		assert.match(text, /^---\ntitle: ".+"\ndescription: ".{50,160}"\neditUrl: "https:\/\/github\.com\//, out);
	}
	for (const dir of GENERATED_DIRS) assert.equal(readFileSync(join(content, dir, '.gitignore'), 'utf8').split('\n').slice(1, 3).join(','), '*,!.gitignore');
});

const FORBIDDEN = [/\bAutoCAD\b/i, /\bMicroStation\b/i, /\bAutodesk\b/i, /\bBentley\b/i, /\bODA\b/, /\bcompatible with\b/i, /\bcertified\b/i, /\bLibreDWG\b/i, /\bblazing(ly)?\b/i, /\bcoming soon\b/i, /\blorem ipsum\b/i];

test('hand-written docs and format pages: frontmatter and wording', () => {
	const generated = GENERATED_DIRS.map((g) => join(content, g) + sep);
	const files = [];
	const walk = (dir) => {
		for (const name of readdirSync(dir)) {
			const p = join(dir, name);
			if (statSync(p).isDirectory()) walk(p);
			else if (/\.(md|mdx|astro)$/.test(name)) files.push(p);
		}
	};
	walk(content);
	walk(join(site, 'src/pages/formats'));
	const handWritten = files.filter((f) => !generated.some((g) => f.startsWith(g)));
	assert.ok(handWritten.length >= 15, `expected the hand-written pages, found ${handWritten.length}`);
	for (const f of handWritten) {
		const text = readFileSync(f, 'utf8');
		const where = relative(site, f);
		if (/\.mdx?$/.test(f)) {
			const fm = /^---\n([\s\S]*?)\n---\n/.exec(text)?.[1] ?? '';
			assert.match(fm, /^title: .+$/m, `${where}: title`);
			const raw = /^description: (.+)$/m.exec(fm)?.[1] ?? '';
			const description = raw.startsWith('"') ? JSON.parse(raw) : raw;
			assert.ok(description.length >= 50 && description.length <= 160, `${where}: description length ${description.length}`);
		} else if (!f.endsWith('_FormatPage.astro')) {
			const description = /description="([^"]+)"/.exec(text)?.[1] ?? '';
			assert.ok(description.length >= 50 && description.length <= 160, `${where}: description length ${description.length}`);
		}
		for (const re of FORBIDDEN) assert.doesNotMatch(text, re, `${where}: forbidden wording`);
	}
});
