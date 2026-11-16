// Pure helpers for the "last modified" manifest written by scripts/prebuild.d/40-lastmod.mjs
// (no file system or git access; unit-tested in test/seo.test.mjs).

/**
 * URL path of a routed source file.
 * @param {string} rel  Path relative to the routing root (src/pages or src/content/docs), with
 *                      forward slashes, e.g. "index.astro", "formats/dwg.astro", "docs/guides/x.mdx".
 * @returns {string} Site path with a trailing slash, e.g. "/", "/formats/dwg/", "/docs/guides/x/".
 */
export function routeOf(rel) {
	const path = rel.replace(/\.(astro|mdx?)$/, '').replace(/(^|\/)index$/, '');
	return path === '' ? '/' : '/' + path + '/';
}

/**
 * Relative import specifiers of a source file, without query suffixes such as "?raw".
 * @param {string} source  Text of an .astro, .mdx, .ts or .mjs file.
 * @returns {string[]}
 */
export function relativeImports(source) {
	const out = [];
	for (const m of source.matchAll(/\bimport\s+(?:[\w*{}\s,$]+\s+from\s+)?['"](\.{1,2}\/[^'"?]+)(?:\?[^'"]*)?['"]/g)) out.push(m[1]);
	return out;
}

/**
 * Whether a repository file carries page content (text, data or code samples). Layouts, shared
 * components, styles and src/config.ts are site chrome: a change there must not move the
 * modification date of every page.
 * @param {string} repoPath  Repository-relative path with forward slashes.
 */
export function isContentSource(repoPath) {
	return (
		repoPath.startsWith('site/src/data/') ||
		repoPath.startsWith('site/scripts/check-snippets/') ||
		repoPath.startsWith('site/src/content/') ||
		/^site\/src\/pages\/(?:.*\/)?_[^/]+$/.test(repoPath)
	);
}

/**
 * Normalises a git committer date to the W3C datetime the sitemap protocol expects (UTC, seconds).
 * @param {string} iso  e.g. "2026-11-16T20:09:12+03:00".
 * @returns {string | undefined} e.g. "2026-11-16T17:09:12Z"; undefined when unparsable.
 */
export function toUtcSeconds(iso) {
	const time = Date.parse(iso);
	return Number.isFinite(time) ? new Date(time).toISOString().replace(/\.\d{3}Z$/, 'Z') : undefined;
}
