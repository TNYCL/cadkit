// Atom feed of releases, built from CHANGELOG.md (Keep a Changelog headings). Pure functions;
// unit-tested in test/seo.test.mjs.

/**
 * Anchor Starlight (github-slugger) gives a release heading:
 * "[0.5.3] - 2026-11-16" -> "053---2026-11-16".
 */
export function headingAnchor(heading) {
	return heading
		.toLowerCase()
		.replace(/[^a-z0-9 _-]/g, '')
		.replaceAll(' ', '-');
}

/**
 * @param {string} markdown  Text of CHANGELOG.md.
 * @returns {{version: string, date: string, anchor: string, body: string}[]} Released versions
 *          in file order (newest first); "Unreleased" and undated sections are skipped.
 */
export function parseChangelog(markdown) {
	const releases = [];
	let current;
	for (const line of markdown.replace(/\r\n/g, '\n').split('\n')) {
		const heading = /^## (.+?)\s*$/.exec(line);
		if (heading) {
			const m = /^\[([^\]]+)\] - (\d{4}-\d{2}-\d{2})$/.exec(heading[1]);
			current = m ? { version: m[1], date: m[2], anchor: headingAnchor(heading[1]), lines: [] } : undefined;
			if (current) releases.push(current);
		} else if (/^\[[^\]]+\]:\s/.test(line)) {
			// Link reference definitions at the end of the file belong to no release.
		} else current?.lines.push(line);
	}
	return releases.map(({ lines, ...rest }) => ({ ...rest, body: lines.join('\n').trim() }));
}

const xml = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/**
 * @param {object} opts
 * @param {string} opts.site      Canonical origin, no trailing slash.
 * @param {string} opts.name      Project name.
 * @param {ReturnType<typeof parseChangelog>} opts.releases
 * @param {number} [opts.limit]   Newest releases to include (default 20).
 * @returns {string} Atom 1.0 document. Dates are release dates at 00:00 UTC, so the feed is
 *          identical from build to build until a release is added.
 */
export function atomFeed({ site, name, releases, limit = 20 }) {
	const page = site + '/changelog/';
	const stamp = (date) => date + 'T00:00:00Z';
	const entries = releases.slice(0, limit).map((r) =>
		[
			'  <entry>',
			`    <title>${xml(name)} ${xml(r.version)}</title>`,
			`    <link rel="alternate" type="text/html" href="${page}#${r.anchor}"/>`,
			`    <id>${page}#${r.anchor}</id>`,
			`    <updated>${stamp(r.date)}</updated>`,
			`    <content type="text">${xml(r.body)}</content>`,
			'  </entry>',
		].join('\n')
	);
	return [
		'<?xml version="1.0" encoding="utf-8"?>',
		'<feed xmlns="http://www.w3.org/2005/Atom">',
		`  <title>${xml(name)} changelog</title>`,
		`  <subtitle>Releases of ${xml(name)}: the Rust crates, CLI, Python, JavaScript and C packages.</subtitle>`,
		`  <link rel="self" type="application/atom+xml" href="${page}feed.xml"/>`,
		`  <link rel="alternate" type="text/html" href="${page}"/>`,
		`  <id>${page}</id>`,
		`  <updated>${stamp(releases[0]?.date ?? '1970-01-01')}</updated>`,
		`  <author><name>${xml(name)} maintainers</name></author>`,
		...entries,
		'</feed>',
		'',
	].join('\n');
}
