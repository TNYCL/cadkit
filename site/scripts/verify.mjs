#!/usr/bin/env node
// Post-build checks over dist/ (run after `npm run build`):
//   - every page: exactly one <h1>, one <main>, lang, non-empty <title>, meta description,
//     canonical (except noindex pages), og:image pointing at an existing file, img alt
//   - internal links and fragment targets resolve (routes still to be built are listed in
//     PENDING_ROUTES and reported as notices)
//   - sitemap, robots.txt, llms.txt, .well-known/security.txt (RFC 9116 fields, Expires window)
//   - _headers: no placeholders left, every inline script hash present, lines under 2000 chars
//   - no forbidden strings on hand-written pages, no secret-like strings anywhere, only role
//     e-mail addresses, no build-machine paths
//   - SEO: unique titles and descriptions of a sensible length, every indexable page (and no
//     noindex page) in the sitemap, JSON-LD that parses, uses schema.org, has no rating, review
//     or FAQ types, and whose breadcrumbs and logo point at existing pages
//   - privacy: no resource loaded from another site except the analytics beacon, which never
//     appears on demo or noindex pages
// Exit code 1 on any error.

import { readFileSync, existsSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { listFiles, urlPathOf, inlineScripts, scriptHash } from './postbuild.mjs';
import { SITE, EMAIL, CADKIT_VERSION } from '../src/config.ts';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(root, 'dist');

/**
 * Routes linked from shared navigation that later work packages add (demo, stats, community,
 * legal and format pages). Remove an entry once its page exists; verify then checks it normally
 * and warns when an entry is no longer needed.
 */
export const PENDING_ROUTES = [];

/** Path prefixes whose content is synced from the repository docs (not hand-written here). */
const SYNCED_PREFIXES = ['/docs/', '/changelog/'];

const FORBIDDEN = [
	/\bAutoCAD\b/i,
	/\bMicroStation\b/i,
	/\bAutodesk\b/i,
	/\bBentley\b/i,
	/\bODA\b/,
	/\bcompatible with\b/i,
	/\bcertified\b/i,
	/\bLibreDWG\b/i,
	/\bLibreCAD\b/i,
	/\bQCAD\b/,
	/\blibdxfrw\b/i,
	/\bTrustedDWG\b/i,
	/\bRealDWG\b/i,
	/\bTeigha\b/i,
	/\bCityMax\b/i,
	/\bblazing(ly)?\b/i,
	/\blorem ipsum\b/i,
	/\bcoming soon\b/i,
];

const SECRETS = [
	/\bgh[pousr]_[A-Za-z0-9]{30,}/,
	/\bgithub_pat_[A-Za-z0-9_]{40,}/,
	/\bAKIA[0-9A-Z]{16}\b/,
	/-----BEGIN [A-Z ]*PRIVATE KEY-----/,
	/\bxox[abprs]-[A-Za-z0-9-]{10,}/,
	/\bnpm_[A-Za-z0-9]{36}\b/,
	/\bpypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{20,}/,
	/\bsk-[A-Za-z0-9]{32,}/,
	/\bAIza[0-9A-Za-z_-]{35}\b/,
];

const ALLOWED_EMAILS = new Set(Object.values(EMAIL));
// Verbatim upstream licence files served with the demo samples (see /demo/samples/).
const THIRD_PARTY_VERBATIM = /(^|\/)demo\/samples\/licenses\//;

const errors = [];
const notices = [];
const err = (where, msg) => errors.push(`${where}: ${msg}`);

if (!existsSync(dist)) {
	console.error('dist/ not found: run npm run build first');
	process.exit(1);
}

const files = listFiles(dist);
const htmlFiles = files.filter((f) => f.endsWith('.html'));
const textFiles = files.filter((f) => /\.(html|txt|xml|js|css|json|svg|md)$/.test(f) || f.endsWith('_headers'));

/** Resolve a site path to a built file, or undefined. */
function resolveTarget(pathname) {
	let p;
	try {
		p = decodeURIComponent(pathname);
	} catch {
		return undefined;
	}
	const candidates = p.endsWith('/') ? [join(dist, p, 'index.html')] : [join(dist, p), join(dist, p, 'index.html')];
	return candidates.find((c) => existsSync(c) && statSync(c).isFile());
}

const idCache = new Map();
function idsOf(file) {
	if (!idCache.has(file)) {
		const html = readFileSync(file, 'utf8');
		idCache.set(file, new Set([...html.matchAll(/\bid="([^"]+)"/g)].map((m) => m[1])));
	}
	return idCache.get(file);
}

const strip = (html) =>
	html
		.replace(/<script\b[\s\S]*?<\/script>/g, ' ')
		.replace(/<style\b[\s\S]*?<\/style>/g, ' ')
		.replace(/<[^>]+>/g, ' ');

const pendingSeen = new Set();
const allHashes = new Set();
/** One entry per built page, for the cross-page checks below (titles, descriptions, sitemap). */
const pages = [];

/** Schema types that would claim ratings, reviews or Q&A content this site does not have. */
const BANNED_LD_TYPES = /^(AggregateRating|Rating|Review|FAQPage|QAPage|HowTo|Product|Offer)$/;

function checkJsonLd(where, path, data) {
	if (data?.['@context'] !== 'https://schema.org') err(where, 'JSON-LD @context must be https://schema.org');
	const walk = (v) => {
		if (Array.isArray(v)) v.forEach(walk);
		else if (v && typeof v === 'object')
			for (const [k, x] of Object.entries(v)) {
				if (k === '@type' && BANNED_LD_TYPES.test(String(x))) err(where, `JSON-LD type ${x} is not allowed`);
				if (/rating|review/i.test(k)) err(where, `JSON-LD property ${k} is not allowed`);
				walk(x);
			}
	};
	walk(data);
	const onSite = (u) => typeof u === 'string' && u.startsWith(SITE + '/') && resolveTarget(new URL(u).pathname);
	switch (data?.['@type']) {
		case 'BreadcrumbList': {
			const items = Array.isArray(data.itemListElement) ? data.itemListElement : [];
			if (items.length < 2) err(where, 'BreadcrumbList needs at least two items');
			items.forEach((it, i) => {
				if (it?.['@type'] !== 'ListItem' || it.position !== i + 1 || !it.name) err(where, `BreadcrumbList item ${i + 1} is malformed`);
				if (!onSite(it?.item)) err(where, `BreadcrumbList item ${i + 1} is not a page of this site: ${it?.item}`);
			});
			if (items.at(-1)?.item !== SITE + path) err(where, 'BreadcrumbList must end at this page');
			break;
		}
		case 'SoftwareSourceCode':
			for (const k of ['name', 'description', 'url', 'codeRepository', 'license']) if (!data[k]) err(where, `SoftwareSourceCode lacks ${k}`);
			if (data.version !== CADKIT_VERSION) err(where, `SoftwareSourceCode version ${data.version} != ${CADKIT_VERSION}`);
			break;
		case 'Organization':
			if (!data.name || !onSite(data.url) || !onSite(data.logo)) err(where, 'Organization needs a name, a site url and an existing logo');
			break;
	}
}

for (const file of htmlFiles) {
	const html = readFileSync(file, 'utf8');
	const path = urlPathOf(dist, file);
	const where = path;
	const noindex = /<meta\b[^>]*name="robots"[^>]*content="[^"]*noindex/.test(html);

	const h1 = (html.match(/<h1\b/g) ?? []).length;
	if (h1 !== 1) err(where, `expected exactly one <h1>, found ${h1}`);
	const mains = (html.match(/<main\b/g) ?? []).length;
	if (mains !== 1) err(where, `expected exactly one <main>, found ${mains}`);
	if (!/<html\b[^>]*\blang="en"/.test(html)) err(where, 'missing <html lang="en">');
	const title = /<title>([^<]*)<\/title>/.exec(html)?.[1]?.trim();
	if (!title) err(where, 'missing <title>');
	else if (!title.endsWith('· cadkit')) err(where, `title should end with "· cadkit": ${title}`);
	if (!/<meta\b[^>]*name="description"[^>]*content="[^"]{20,}"/.test(html)) err(where, 'missing or short meta description');
	if (!/<meta\b[^>]*name="viewport"/.test(html)) err(where, 'missing viewport meta');
	const description = /<meta\b[^>]*name="description"[^>]*content="([^"]*)"/.exec(html)?.[1] ?? '';
	pages.push({ path, title: title ?? '', description, noindex });

	for (const m of html.matchAll(/<script\b[^>]*type="application\/ld\+json"[^>]*>([\s\S]*?)<\/script>/g)) {
		let data;
		try {
			data = JSON.parse(m[1]);
		} catch (e) {
			err(where, `JSON-LD does not parse: ${e.message}`);
			continue;
		}
		checkJsonLd(where, path, data);
	}

	// The privacy page promises that pages load nothing from other sites except the optional
	// analytics beacon, and that the beacon never runs on the demo or on noindex pages (404).
	for (const m of html.matchAll(/<(script|img|link|iframe|source|video|audio|embed|object)\b[^>]*>/g)) {
		const tag = m[0];
		if (m[1] === 'link' && !/\brel="(stylesheet|preload|modulepreload|prefetch|preconnect|dns-prefetch|icon|apple-touch-icon|manifest)"/.test(tag)) continue;
		for (const u of tag.matchAll(/\b(?:src|href|srcset|data)="(?:https?:)?\/\/([^/"\s]+)/g)) {
			const host = u[1];
			const beacon = m[1] === 'script' && host === 'static.cloudflareinsights.com';
			if ('https://' + host !== SITE && !beacon) err(where, `loads a resource from another site: ${tag.slice(0, 120)}`);
		}
	}
	if (noindex && html.includes('cloudflareinsights')) err(where, 'analytics beacon on a noindex page');

	if (!noindex) {
		const canonical = /<link\b[^>]*rel="canonical"[^>]*href="([^"]+)"/.exec(html)?.[1];
		if (!canonical) err(where, 'missing canonical');
		else if (canonical !== SITE + path) err(where, `canonical ${canonical} != ${SITE + path}`);
		const og = /<meta\b[^>]*property="og:image"[^>]*content="([^"]+)"/.exec(html)?.[1];
		if (!og) err(where, 'missing og:image');
		else if (!og.startsWith(SITE + '/')) err(where, `og:image not on ${SITE}: ${og}`);
		else if (!resolveTarget(new URL(og).pathname)) err(where, `og:image file missing: ${og}`);
		if (!/<meta\b[^>]*name="twitter:card"/.test(html)) err(where, 'missing twitter:card');
	}

	for (const m of html.matchAll(/<img\b[^>]*>/g)) if (!/\balt="/.test(m[0])) err(where, `img without alt: ${m[0].slice(0, 80)}`);
	if (html.includes('__CK_')) err(where, 'unreplaced __CK_ placeholder');
	if (/<span data-ck-sheet[^>]*>\s*(—|&mdash;)\s*</.test(html)) err(where, 'SHEET number not filled');

	for (const body of inlineScripts(html)) allHashes.add(scriptHash(body));

	// Links and assets.
	for (const m of html.matchAll(/\s(?:href|src)="([^"]+)"/g)) {
		const raw = m[1].replace(/&amp;/g, '&');
		if (/^(mailto:|tel:|data:|javascript:)/.test(raw)) continue;
		let url;
		try {
			url = new URL(raw, SITE + path);
		} catch {
			err(where, `unparsable URL ${raw}`);
			continue;
		}
		if (url.origin !== SITE) continue;
		const target = resolveTarget(url.pathname);
		if (!target) {
			if (PENDING_ROUTES.includes(url.pathname)) pendingSeen.add(url.pathname);
			else err(where, `broken internal link ${raw}`);
			continue;
		}
		if (url.hash && target.endsWith('.html')) {
			const id = decodeURIComponent(url.hash.slice(1));
			// "#_top" and "#top" scroll to the top of any document (HTML spec).
			if (id && id !== '_top' && id !== 'top' && !idsOf(target).has(id)) err(where, `missing anchor ${raw}`);
		}
		if (/\/[^/.]+$/.test(url.pathname) && target.endsWith('index.html')) err(where, `link without trailing slash: ${raw}`);
	}

	// Words this project does not use on its own pages (synced docs keep their technical notes).
	if (!SYNCED_PREFIXES.some((p) => path.startsWith(p))) {
		const text = strip(html);
		for (const re of FORBIDDEN) {
			const hit = re.exec(text);
			if (hit) err(where, `forbidden wording "${hit[0]}"`);
		}
	}
}

for (const file of textFiles) {
	const rel = file.slice(dist.length);
	if (/\/pagefind\//.test(rel) && !rel.endsWith('.json')) continue;
	const text = readFileSync(file, 'utf8');
	for (const re of SECRETS) if (re.test(text)) err(rel, `secret-like string matches ${re}`);
	// Build-machine paths must not leak into shipped files.
	const local = /(?:^|[\s"'(=:])((?:\/home|\/Users)\/[A-Za-z0-9._-]+\/|[A-Z]:\\Users\\)/.exec(text);
	if (local) err(rel, `local absolute path ${local[1]}`);
	// Third-party licence texts are shipped verbatim and may name their authors' addresses.
	if (/\.(html|txt|xml|md)$/.test(rel) && !THIRD_PARTY_VERBATIM.test(rel.replaceAll('\\', '/'))) {
		for (const m of text.matchAll(/[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g)) {
			const addr = m[0].toLowerCase();
			if (/\.(png|svg|webp|jpg|woff2?)$/.test(addr)) continue;
			if (!ALLOWED_EMAILS.has(addr)) err(rel, `unexpected e-mail address ${addr}`);
		}
	}
}

// Sitemap.
const sitemapIndex = join(dist, 'sitemap-index.xml');
if (!existsSync(sitemapIndex)) err('sitemap', 'sitemap-index.xml missing');
else {
	const locs = [];
	for (const m of readFileSync(sitemapIndex, 'utf8').matchAll(/<loc>([^<]+)<\/loc>/g)) {
		const f = join(dist, new URL(m[1]).pathname);
		if (!existsSync(f)) err('sitemap', `missing ${m[1]}`);
		else for (const u of readFileSync(f, 'utf8').matchAll(/<loc>([^<]+)<\/loc>/g)) locs.push(u[1]);
	}
	if (!locs.includes(SITE + '/')) err('sitemap', 'home page not listed');
	for (const loc of locs) {
		if (!loc.startsWith(SITE + '/')) err('sitemap', `foreign URL ${loc}`);
		else if (!resolveTarget(new URL(loc).pathname)) err('sitemap', `URL without page ${loc}`);
		if (/\/404\b/.test(loc)) err('sitemap', '404 page listed');
	}
	notices.push(`sitemap: ${locs.length} URLs`);
	const listed = new Set(locs);
	for (const p of pages) {
		if (p.noindex && listed.has(SITE + p.path)) err('sitemap', `noindex page listed: ${p.path}`);
		if (!p.noindex && !listed.has(SITE + p.path)) err('sitemap', `indexable page not listed: ${p.path}`);
	}
}

// Titles and descriptions: unique across indexable pages, of a length search results show.
{
	const indexable = pages.filter((p) => !p.noindex);
	for (const key of ['title', 'description']) {
		const seen = new Map();
		for (const p of indexable) {
			const v = p[key];
			if (!v) continue;
			if (seen.has(v)) err(p.path, `same ${key} as ${seen.get(v)}`);
			else seen.set(v, p.path);
		}
	}
	for (const p of indexable) {
		const n = p.description.replace(/&[a-z]+;|&#\d+;/g, '_').length;
		if (n < 70 || n > 170) err(p.path, `meta description has ${n} characters (70 to 170 expected)`);
		if (p.title.length > 75) err(p.path, `title has ${p.title.length} characters (at most 75 expected)`);
	}
}

// robots.txt, llms.txt, security.txt.
const read = (p) => (existsSync(join(dist, p)) ? readFileSync(join(dist, p), 'utf8') : undefined);
const robots = read('robots.txt');
if (!robots?.includes(`Sitemap: ${SITE}/sitemap-index.xml`)) err('robots.txt', 'missing or without sitemap');
const llms = read('llms.txt');
if (!llms?.startsWith('# cadkit')) err('llms.txt', 'missing or malformed');
else
	for (const m of llms.matchAll(/\]\((https?:\/\/[^)]+)\)/g)) {
		const u = new URL(m[1]);
		if (u.origin === SITE && !resolveTarget(u.pathname) && !PENDING_ROUTES.includes(u.pathname)) err('llms.txt', `broken link ${m[1]}`);
	}
const sec = read('.well-known/security.txt');
if (!sec) err('security.txt', 'missing');
else {
	for (const field of ['Contact:', 'Expires:', 'Canonical:', 'Policy:', 'Preferred-Languages:'])
		if (!sec.includes(field)) err('security.txt', `missing ${field}`);
	const exp = /^Expires: (.+)$/m.exec(sec)?.[1];
	const days = exp ? (Date.parse(exp) - Date.now()) / 864e5 : Number.NaN;
	if (!(days > 150 && days < 366)) err('security.txt', `Expires should be about 180 days ahead, got ${exp}`);
}

// _headers.
const headers = read('_headers');
if (!headers) err('_headers', 'missing');
else {
	if (headers.includes('__CK_')) err('_headers', 'unreplaced placeholder');
	for (const line of headers.split('\n')) if (line.length > 2000) err('_headers', `line longer than 2000 characters (${line.length})`);
	const csp = [...headers.matchAll(/^\s*Content-Security-Policy: (.+)$/gm)].map((m) => m[1]);
	if (csp.length < 2) err('_headers', 'expected a site CSP and a /demo/ CSP');
	for (const h of allHashes) if (!csp.every((c) => c.includes(h))) err('_headers', `inline script hash ${h} missing from a CSP`);
	for (const c of csp) {
		if (/'unsafe-eval'/.test(c)) err('_headers', "CSP allows 'unsafe-eval'");
		if (/script-src[^;]*'unsafe-inline'/.test(c)) err('_headers', "script-src allows 'unsafe-inline'");
		if (!/frame-ancestors 'none'/.test(c)) err('_headers', "CSP lacks frame-ancestors 'none'");
	}
	const demo = headers.split(/\n(?=\/)/).find((b) => b.startsWith('/demo/*'));
	if (!demo || !demo.includes('! Content-Security-Policy')) err('_headers', '/demo/* must detach the site CSP');
	if (demo && /cloudflareinsights/.test(demo)) err('_headers', '/demo/* must not allow the analytics beacon');
	for (const h of ['Strict-Transport-Security', 'X-Content-Type-Options', 'Referrer-Policy: strict-origin-when-cross-origin', 'Permissions-Policy'])
		if (!headers.includes(h)) err('_headers', `missing ${h}`);
}

for (const file of htmlFiles.filter((f) => urlPathOf(dist, f).startsWith('/demo/'))) {
	if (readFileSync(file, 'utf8').includes('cloudflareinsights')) err(urlPathOf(dist, file), 'analytics beacon on a demo page');
}

for (const r of PENDING_ROUTES) {
	if (resolveTarget(r)) notices.push(`PENDING_ROUTES entry ${r} now exists: remove it from scripts/verify.mjs`);
}
if (pendingSeen.size) notices.push(`links to routes not built yet (allowed, see PENDING_ROUTES): ${[...pendingSeen].sort().join(' ')}`);

console.log(`verify: ${htmlFiles.length} HTML files, ${files.length} files, ${allHashes.size} inline script hashes`);
for (const n of notices) console.log('note: ' + n);
if (errors.length) {
	for (const e of errors) console.error('error: ' + e);
	console.error(`verify failed with ${errors.length} error(s)`);
	process.exit(1);
}
console.log('verify: ok');
