// Atom feed of releases (/changelog/feed.xml), built from the repository's CHANGELOG.md.
import type { APIRoute } from 'astro';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { SITE, SITE_NAME } from '../../config';
import { parseChangelog, atomFeed } from '../../lib/changelog-feed.mjs';

export const GET: APIRoute = () => {
	// The build runs in site/; CHANGELOG.md is at the repository root.
	const releases = parseChangelog(readFileSync(join(process.cwd(), '..', 'CHANGELOG.md'), 'utf8'));
	if (releases.length === 0) throw new Error('changelog feed: no dated release found in CHANGELOG.md');
	return new Response(atomFeed({ site: SITE, name: SITE_NAME, releases }), {
		headers: { 'Content-Type': 'application/atom+xml; charset=utf-8' },
	});
};
