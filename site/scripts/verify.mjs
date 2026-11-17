#!/usr/bin/env node
// Post-build checks over dist/ (run after `npm run build`):
//   - every page: exactly one <h1>, one <main>, lang, non-empty <title>, meta description,
//     canonical (except noindex pages), og:image pointing at an existing file, img alt
//   - internal links and fragment targets resolve (routes still to be built are listed in
//     PENDING_ROUTES and reported as notices)
//   - sitemap (dates, the /sitemap.xml alias), robots.txt policy, llms.txt, llms-full.txt and the
//     Markdown copy of every docs page, web manifest, changelog feed, IndexNow key,
//     .well-known/security.txt (RFC 9116 fields, Expires window)
//   - _headers: no placeholders left, every inline script hash present, lines under 2000 chars
//   - no forbidden strings on hand-written pages, no secret-like strings anywhere, only role
//     e-mail addresses, no build-machine paths
//   - SEO: unique titles and descriptions of a sensible length, every indexable page (and no
//     noindex page) in the sitemap, Twitter and Open Graph tags, JSON-LD that parses, uses
//     schema.org, has no rating, review or FAQ types, whose breadcrumbs and logo point at
//     existing pages and whose dateModified equals the sitemap's lastmod
//   - privacy: no resource loaded from another site except the analytics beacon, which never
//     appears on demo or noindex pages
// Exit code 1 on any error.

import { readFileSync, existsSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { listFiles, urlPathOf, inlineScripts, scriptHash } from './postbuild.mjs';
import { parseSitemap } from './lib/sitemap.mjs';
import { SITE, EMAIL, CADKIT_VERSION } from '../src/config.ts';
import { markdownPathFor } from '../src/lib/site-markdown.mjs';
import { parse, preformattedText, walkSync } from '../src/lib/html-tree.mjs';

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

/** W3C datetime in UTC with seconds, as written by scripts/prebuild.d/40-lastmod.mjs. */
const UTC_SECONDS = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/;

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
		case 'WebSite':
			if (!data.name || data.url !== SITE + '/' || data.inLanguage !== 'en') err(where, 'WebSite needs a name, the site url and inLanguage');
			break;
		case 'TechArticle':
			for (const k of ['headline', 'description', 'author', 'publisher']) if (!data[k]) err(where, `TechArticle lacks ${k}`);
			if (data.url !== SITE + path || data.mainEntityOfPage !== SITE + path) err(where, 'TechArticle url must be this page');
			if (!onSite(data.image)) err(where, `TechArticle image is not a file of this site: ${data.image}`);
			if (data.dateModified !== undefined && !UTC_SECONDS.test(data.dateModified)) err(where, `TechArticle dateModified is not a UTC datetime: ${data.dateModified}`);
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
	const skipTarget = /<a\b[^>]*class="[^"]*sl-skip-link[^"]*"[^>]*href="#([^"]+)"/.exec(html)?.[1];
	if (!skipTarget || !idsOf(file).has(skipTarget)) err(where, 'skip link must point at an existing content target');
	if (!/<link\b[^>]*rel="describedby"[^>]*href="\/llms\.txt"/.test(html)) err(where, 'missing llms.txt discovery link');
	const description = /<meta\b[^>]*name="description"[^>]*content="([^"]*)"/.exec(html)?.[1] ?? '';
	/** @type {{path: string, title: string, description: string, noindex: boolean, ldTypes: string[], dateModified?: string, markdown?: string}} */
	const pageInfo = { path, title: title ?? '', description, noindex, ldTypes: [] };
	pages.push(pageInfo);

	for (const m of html.matchAll(/<script\b[^>]*type="application\/ld\+json"[^>]*>([\s\S]*?)<\/script>/g)) {
		let data;
		try {
			data = JSON.parse(m[1]);
		} catch (e) {
			err(where, `JSON-LD does not parse: ${e.message}`);
			continue;
		}
		checkJsonLd(where, path, data);
		pageInfo.ldTypes.push(String(data?.['@type']));
		if (data?.['@type'] === 'TechArticle') pageInfo.dateModified = data.dateModified;
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
		for (const name of ['twitter:title', 'twitter:description', 'twitter:image'])
			if (!new RegExp(`<meta\\b[^>]*name="${name}"[^>]*content="[^"]+"`).test(html)) err(where, `missing ${name}`);
		if (!/<meta\b[^>]*property="og:locale"[^>]*content="en_US"/.test(html)) err(where, 'og:locale must be en_US');
		if (!/<link\b[^>]*rel="manifest"[^>]*href="\/site\.webmanifest"/.test(html)) err(where, 'missing web manifest link');
		pageInfo.markdown = /<link\b[^>]*rel="alternate"[^>]*type="text\/markdown"[^>]*href="([^"]+)"/.exec(html)?.[1];
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
			// Only "top" is the reserved top-of-document fragment. Starlight's "_top" needs an id.
			if (id && id.toLowerCase() !== 'top' && !idsOf(target).has(id)) err(where, `missing anchor ${raw}`);
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

// robots.txt policy: nothing disallowed, the main search and AI crawlers named.
if (robots) {
	if (!/^User-agent: \*\nAllow: \/$/m.test(robots)) err('robots.txt', 'missing "User-agent: *" with "Allow: /"');
	if (/^Disallow:\s*\S/m.test(robots)) err('robots.txt', 'a Disallow rule hides part of a fully public site');
	for (const bot of ['Googlebot', 'Bingbot', 'GPTBot', 'OAI-SearchBot', 'ClaudeBot', 'PerplexityBot', 'Google-Extended'])
		if (!robots.includes(`User-agent: ${bot}\n`)) err('robots.txt', `crawler ${bot} is not named`);
}

// Sitemap dates and the /sitemap.xml alias.
{
	const dated = new Map();
	for (const f of files.filter((f) => /[\\/]sitemap-\d+\.xml$/.test(f)))
		for (const { loc, lastmod } of parseSitemap(readFileSync(f, 'utf8'))) if (lastmod) dated.set(loc, lastmod);
	for (const [loc, lastmod] of dated) {
		// The sitemap integration re-serialises dates with milliseconds.
		if (!UTC_SECONDS.test(lastmod.replace(/\.\d{3}Z$/, 'Z'))) err('sitemap', `lastmod of ${loc} is not a UTC datetime: ${lastmod}`);
		else if (Date.parse(lastmod) > Date.now() + 864e5) err('sitemap', `lastmod of ${loc} is in the future: ${lastmod}`);
	}
	const instant = (date) => (date === undefined ? undefined : Date.parse(date));
	for (const p of pages.filter((p) => p.ldTypes.includes('TechArticle')))
		if (instant(p.dateModified) !== instant(dated.get(SITE + p.path))) err(p.path, `JSON-LD dateModified ${p.dateModified} differs from the sitemap lastmod ${dated.get(SITE + p.path)}`);
	notices.push(dated.size ? `sitemap: ${dated.size} URLs with lastmod` : 'sitemap: no lastmod (git history unavailable to the build)');
	if (!/^\/sitemap\.xml\s+\/sitemap-index\.xml\s+301$/m.test(read('_redirects') ?? '')) err('_redirects', 'missing /sitemap.xml -> /sitemap-index.xml (301)');
}

// Structured data: the site on the home page, an article and a breadcrumb trail elsewhere.
for (const p of pages.filter((p) => !p.noindex)) {
	if (p.path === '/') {
		for (const type of ['Organization', 'WebSite', 'SoftwareSourceCode']) if (!p.ldTypes.includes(type)) err(p.path, `missing ${type} JSON-LD`);
	} else if (!p.ldTypes.includes('BreadcrumbList')) err(p.path, 'missing BreadcrumbList JSON-LD');
	if (SYNCED_PREFIXES.some((prefix) => p.path.startsWith(prefix)) && !p.ldTypes.includes('TechArticle')) err(p.path, 'docs page without TechArticle JSON-LD');
}

// Markdown copies of the docs pages, llms.txt and llms-full.txt.
{
	const full = read('llms-full.txt');
	if (!full?.startsWith('# cadkit documentation')) err('llms-full.txt', 'missing or malformed');
	const docs = pages.filter((p) => p.markdown);
	if (docs.length === 0) err('llms.txt', 'no page links a Markdown copy');
	for (const p of pages.filter((p) => !p.noindex && !p.markdown))
		err(p.path, 'page without a Markdown alternate link');
	for (const p of docs) {
		const md = read(p.markdown);
		if (p.markdown !== markdownPathFor(p.path)) err(p.path, `unexpected Markdown path ${p.markdown}`);
		if (!md) {
			err(p.path, `Markdown copy ${p.markdown} is missing`);
			continue;
		}
		if (!md.startsWith('# ') || !md.includes(`Canonical page: ${SITE}${p.path}\n`)) err(p.markdown, 'Markdown copy lacks its title or canonical page');
		if (md.length < 400) err(p.markdown, 'Markdown copy is nearly empty');
		// Real rendered samples catch truncation that small synthetic converter tests cannot.
		const html = readFileSync(resolveTarget(p.path), 'utf8');
		walkSync(parse(html), (node) => {
			if (node.name !== 'pre') return;
			// The syntax highlighter trims end-of-line spaces; indentation and blank lines matter.
			const trimLineEnds = (text) => text.replace(/[ \t]+$/gm, '').trimEnd();
			const source = trimLineEnds(preformattedText(node));
			if (source && !trimLineEnds(md).includes(source)) err(p.markdown, 'a rendered code sample is missing or changed');
		});
		// MDX that the export did not convert would be published as text.
		const leftover = /^import .+ from ['"]|<\/?(?:CodeTabs|Code|Callout|Tabs|TabItem)\b/m.exec(md.replace(/^(`{3,}|~{3,})[\s\S]*?^\1\s*$/gm, ''));
		if (leftover) err(p.markdown, `unconverted MDX: ${leftover[0]}`);
		if (llms && !llms.includes(`(${SITE}${p.markdown})`)) err('llms.txt', `docs page not listed: ${p.markdown}`);
		if (full && !full.includes(md)) err('llms-full.txt', `page content missing or differs: ${p.path}`);
	}
	if (!llms?.includes(`Current release: ${CADKIT_VERSION}.`)) err('llms.txt', 'current release differs from the installed engine');
	if (!full?.includes(`cadkit ${CADKIT_VERSION},`)) err('llms-full.txt', 'version differs from the installed engine');
	const manifest = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
	if (manifest.dependencies['cadkit-wasm'] !== CADKIT_VERSION) err('package.json', 'installed cadkit-wasm differs from the pinned version');
	notices.push(`llms: ${docs.length} pages as Markdown, llms-full.txt ${Math.round((full?.length ?? 0) / 1024)} KiB`);
}

// Web manifest.
{
	const text = read('site.webmanifest');
	let manifest;
	try {
		manifest = JSON.parse(text ?? '');
	} catch {
		err('site.webmanifest', 'missing or not JSON');
	}
	if (manifest) {
		if (!manifest.name || manifest.start_url !== '/') err('site.webmanifest', 'needs a name and start_url "/"');
		const icons = Array.isArray(manifest.icons) ? manifest.icons : [];
		if (!icons.some((i) => i.sizes === '512x512')) err('site.webmanifest', 'needs a 512x512 icon');
		for (const i of icons) if (!resolveTarget(String(i.src))) err('site.webmanifest', `icon file missing: ${i.src}`);
	}
}

// Changelog feed: every entry points at a heading of the changelog page.
{
	const feed = read('changelog/feed.xml');
	const page = join(dist, 'changelog', 'index.html');
	if (!feed?.startsWith('<?xml') || !feed.includes('<feed xmlns="http://www.w3.org/2005/Atom">')) err('changelog/feed.xml', 'missing or not an Atom feed');
	else {
		const links = [...feed.matchAll(/<entry>[\s\S]*?<link rel="alternate"[^>]*href="([^"]+)"/g)].map((m) => m[1]);
		if (links.length === 0) err('changelog/feed.xml', 'no entries');
		for (const link of links) {
			const u = new URL(link);
			if (u.origin + u.pathname !== SITE + '/changelog/' || !existsSync(page) || !idsOf(page).has(decodeURIComponent(u.hash.slice(1))))
				err('changelog/feed.xml', `entry link without a heading on the changelog page: ${link}`);
		}
	}
}

// IndexNow key: public/<key>.txt must hold its own name (scripts/indexnow.mjs submits with it).
{
	const keys = files.map((f) => /[\\/]([a-f0-9]{32})\.txt$/.exec(f)?.[1]).filter((k) => k && existsSync(join(dist, k + '.txt')));
	if (keys.length !== 1) err('indexnow', `expected exactly one key file at the site root, found ${keys.length}`);
	else if (read(keys[0] + '.txt')?.trim() !== keys[0]) err('indexnow', 'the key file does not contain its own name');
}

// _headers.
const headers = read('_headers');
if (!headers) err('_headers', 'missing');
else {
	if (headers.includes('__CK_')) err('_headers', 'unreplaced placeholder');
	if (!/^\s+Link: <\/llms\.txt>; rel="describedby"; type="text\/plain"$/m.test(headers)) err('_headers', 'missing llms.txt discovery header');
	if (!/^\s+Cache-Control: .*\bno-transform\b/m.test(headers)) err('_headers', 'missing no-transform: public contact addresses must stay readable');
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
	// Files that repeat a page or are not pages stay out of search results.
	const rules = headers.split(/\n(?=\/)/);
	for (const pattern of ['/*.md', '/llms-full.txt', '/samples/*', '/demo/samples/licenses/*', '/licenses/*', '/pagefind/*']) {
		const rule = rules.find((b) => b.split('\n')[0].trim() === pattern);
		if (!rule || !/^\s+X-Robots-Tag: noindex$/m.test(rule)) err('_headers', `${pattern} must be served with X-Robots-Tag: noindex`);
	}
	// A noindex header on a page, a sitemap or robots.txt would remove the site from search.
	for (const rule of rules.filter((b) => /X-Robots-Tag/i.test(b))) {
		const pattern = rule.split('\n')[0].trim();
		if (pattern === '/*' || /(^|\/)(sitemap[^/]*|robots\.txt|llms\.txt)$/.test(pattern))
			err('_headers', `X-Robots-Tag on ${pattern} would hide indexable content`);
	}
	for (const h of ['Strict-Transport-Security', 'X-Content-Type-Options', 'Referrer-Policy: strict-origin-when-cross-origin', 'Permissions-Policy'])
		if (!headers.includes(h)) err('_headers', `missing ${h}`);
	// Workers static assets send no charset on HTML and text and no CORS header (Pages did).
	for (const [pattern, header] of [
		['/', 'Content-Type: text/html; charset=utf-8'],
		['/*/', 'Content-Type: text/html; charset=utf-8'],
		['/*.txt', 'Content-Type: text/plain; charset=utf-8'],
		['/*', 'Access-Control-Allow-Origin: *'],
	]) {
		const rule = rules.find((b) => b.split('\n')[0].trim() === pattern);
		if (!rule?.split('\n').some((line) => line.trim() === header)) err('_headers', `${pattern} must send ${header}`);
	}
	// Cloudflare joins the values of two rules that set one header on the same URL.
	const glob = (p) => new RegExp('^' + p.split('*').map((s) => s.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('.*') + '$');
	const typeRules = rules.filter((b) => /^\s+Content-Type:/m.test(b)).map((b) => b.split('\n')[0].trim());
	for (const file of files) {
		const path = urlPathOf(dist, file);
		const hits = typeRules.filter((p) => glob(p).test(path));
		if (hits.length > 1) err(path, `several Content-Type rules match: ${hits.join(', ')}`);
	}
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
