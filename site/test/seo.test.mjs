// SEO build helpers on synthetic input: page dates (scripts/lib/lastmod.mjs), the Markdown
// export of MDX docs pages (src/lib/md-export.mjs), the changelog feed and the sitemap reader.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { routeOf, relativeImports, isContentSource, toUtcSeconds } from '../scripts/lib/lastmod.mjs';
import { parseSitemap, parseSince } from '../scripts/lib/sitemap.mjs';
import { mdxToMarkdown, absoluteLinks, markdownDocument, fence } from '../src/lib/md-export.mjs';
import { parseChangelog, atomFeed, headingAnchor } from '../src/lib/changelog-feed.mjs';

test('routeOf maps source files to site paths', () => {
	assert.equal(routeOf('index.astro'), '/');
	assert.equal(routeOf('formats/dwg.astro'), '/formats/dwg/');
	assert.equal(routeOf('demo/samples/index.astro'), '/demo/samples/');
	assert.equal(routeOf('docs/index.md'), '/docs/');
	assert.equal(routeOf('docs/guides/geometry.mdx'), '/docs/guides/geometry/');
	assert.equal(routeOf('provenance.md'), '/provenance/');
});

test('relativeImports finds relative specifiers and drops query suffixes', () => {
	const source = [
		"import { Code } from '@astrojs/starlight/components';",
		"import rust from '../../snippets/a.rs?raw';",
		'import {',
		'  FORMATS,',
		"} from '../data/formats';",
		"import './side-effect.css';",
		"const x = 'import y from \"./not-code\"';",
	].join('\n');
	assert.deepEqual(relativeImports(source), ['../../snippets/a.rs', '../data/formats', './side-effect.css', './not-code']);
});

test('isContentSource separates page content from site chrome', () => {
	assert.ok(isContentSource('site/src/data/formats.ts'));
	assert.ok(isContentSource('site/scripts/check-snippets/docs/rust/quickstart.rs'));
	assert.ok(isContentSource('site/src/pages/formats/_FormatPage.astro'));
	assert.ok(!isContentSource('site/src/config.ts'));
	assert.ok(!isContentSource('site/src/layouts/PageLayout.astro'));
	assert.ok(!isContentSource('site/src/components/FormatMatrix.astro'));
});

test('toUtcSeconds normalises git dates and rejects garbage', () => {
	assert.equal(toUtcSeconds('2026-11-16T20:09:12+03:00'), '2026-11-16T17:09:12Z');
	assert.equal(toUtcSeconds('2026-11-16T17:09:12Z'), '2026-11-16T17:09:12Z');
	assert.equal(toUtcSeconds(''), undefined);
});

test('parseSitemap reads url sets and indexes', () => {
	const xml =
		'<urlset><url><loc>https://example.org/</loc><lastmod>2026-01-02T03:04:05Z</lastmod></url>' +
		'<url><loc>https://example.org/a/?x=1&amp;y=2</loc></url></urlset>';
	assert.deepEqual(parseSitemap(xml), [
		{ loc: 'https://example.org/', lastmod: '2026-01-02T03:04:05Z' },
		{ loc: 'https://example.org/a/?x=1&y=2' },
	]);
	assert.deepEqual(parseSitemap('<sitemapindex><sitemap><loc>https://example.org/sitemap-0.xml</loc></sitemap></sitemapindex>'), [
		{ loc: 'https://example.org/sitemap-0.xml' },
	]);
});

test('parseSince accepts dates and ages', () => {
	const now = Date.parse('2026-11-16T12:00:00Z');
	assert.equal(parseSince('2026-11-15', now), Date.parse('2026-11-15T00:00:00Z'));
	assert.equal(parseSince('36h', now), now - 36 * 36e5);
	assert.equal(parseSince('7d', now), now - 7 * 864e5);
	assert.equal(parseSince('soon', now), undefined);
});

test('fence outgrows backticks in the code', () => {
	assert.equal(fence('a\n', 'sh'), '```sh\na\n```');
	assert.equal(fence('```md\nx\n```', 'md', 'a "b"'), '````md title="a \'b\'"\n```md\nx\n```\n````');
});

test('mdxToMarkdown converts the three components and drops imports', () => {
	const files = { '../snippets/a.rs': 'fn main() {}\n', '../snippets/a.py': 'print(1)\n' };
	const mdx = [
		"import { Code } from '@astrojs/starlight/components';",
		"import CodeTabs from '../components/CodeTabs.astro';",
		"import rust from '../snippets/a.rs?raw';",
		"import python from '../snippets/a.py?raw';",
		'',
		'Intro with `Result<T>` and a [link](/docs/x/).',
		'',
		'<CodeTabs',
		'  snippets={[',
		"    { label: 'Rust', lang: 'rust', code: rust, title: 'src/main.rs' },",
		"    { label: 'Python', lang: 'python', code: python },",
		'  ]}',
		'/>',
		'',
		'<Code code={rust} lang="rust" title="main.rs" />',
		'',
		'<Code code={`cadkit = "${CADKIT_VERSION}"\\nx = 1`} lang="toml" title="Cargo.toml" />',
		'',
		'<Callout variant="caution" title="Heads up">',
		'  First line',
		'  second line.',
		'</Callout>',
		'',
		'```mdx',
		"import kept from './inside-a-fence';",
		'<Code code={kept} lang="js" />',
		'```',
	].join('\n');
	const md = mdxToMarkdown(mdx, { readImport: (spec) => files[spec], vars: { CADKIT_VERSION: '1.2.3' } });
	assert.equal(
		md,
		[
			'Intro with `Result<T>` and a [link](/docs/x/).',
			'',
			'**Rust**',
			'',
			'```rust title="src/main.rs"',
			'fn main() {}',
			'```',
			'',
			'**Python**',
			'',
			'```python',
			'print(1)',
			'```',
			'',
			'```rust title="main.rs"',
			'fn main() {}',
			'```',
			'',
			'```toml title="Cargo.toml"',
			'cadkit = "1.2.3"',
			'x = 1',
			'```',
			'',
			'> **Heads up**',
			'>',
			'> First line',
			'> second line.',
			'',
			'```mdx',
			"import kept from './inside-a-fence';",
			'<Code code={kept} lang="js" />',
			'```',
			'',
		].join('\n')
	);
});

test('mdxToMarkdown refuses what it cannot convert', () => {
	const ctx = { readImport: () => '', vars: {} };
	assert.throws(() => mdxToMarkdown('<Tabs>\n</Tabs>\n', ctx), /unsupported MDX syntax/);
	assert.throws(() => mdxToMarkdown('<Code code={missing} lang="js" />\n', ctx), /not imported/);
	assert.throws(() => mdxToMarkdown('<Code code={`${NOPE}`} lang="js" />\n', ctx), /unknown value NOPE/);
	assert.throws(() => mdxToMarkdown('{someExpression}\n', ctx), /unsupported MDX syntax/);
});

test('MDX export preserves multiple blank lines inside a source example', () => {
	const source = 'first = 1\n\n\nsecond = 2\n';
	const md = mdxToMarkdown("import sample from './sample.py?raw';\n\n<Code code={sample} lang=\"python\" />", {
		readImport: () => source, vars: {},
	});
	assert.ok(md.includes(source));
});

test('absoluteLinks and markdownDocument', () => {
	const md = 'See [x](/docs/x/#a), [y](https://other.example/y) and `[z](/z/)`.\n\n```\n[c](/c/)\n```\n';
	assert.equal(
		absoluteLinks(md, 'https://example.org'),
		'See [x](https://example.org/docs/x/#a), [y](https://other.example/y) and `[z](/z/)`.\n\n```\n[c](/c/)\n```\n'
	);
	assert.equal(
		markdownDocument({ title: 'T', description: 'D.', url: 'https://example.org/t/', body: '\nBody.\n\n' }),
		'# T\n\n> D.\n\nCanonical page: https://example.org/t/\n\nBody.\n'
	);
});

test('changelog feed lists dated releases with the anchors of the changelog page', () => {
	assert.equal(headingAnchor('[0.5.3] - 2026-11-16'), '053---2026-11-16');
	const md = [
		'# Changelog',
		'',
		'## [Unreleased]',
		'',
		'- pending',
		'',
		'## [0.2.0] - 2026-02-01',
		'',
		'### Added',
		'',
		'- a < b & c',
		'',
		'## [0.1.0] - 2026-01-01',
		'',
		'- first',
		'',
		'[0.2.0]: https://example.org/compare',
	].join('\n');
	const releases = parseChangelog(md);
	assert.deepEqual(releases, [
		{ version: '0.2.0', date: '2026-02-01', anchor: '020---2026-02-01', body: '### Added\n\n- a < b & c' },
		{ version: '0.1.0', date: '2026-01-01', anchor: '010---2026-01-01', body: '- first' },
	]);
	const feed = atomFeed({ site: 'https://example.org', name: 'cadkit', releases, limit: 1 });
	assert.match(feed, /^<\?xml version="1\.0" encoding="utf-8"\?>\n<feed xmlns="http:\/\/www\.w3\.org\/2005\/Atom">/);
	assert.match(feed, /<updated>2026-02-01T00:00:00Z<\/updated>/);
	assert.match(feed, /<link rel="alternate" type="text\/html" href="https:\/\/example\.org\/changelog\/#020---2026-02-01"\/>/);
	assert.match(feed, /- a &lt; b &amp; c/);
	assert.equal((feed.match(/<entry>/g) ?? []).length, 1);
});
