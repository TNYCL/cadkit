// Astro integration that finishes the static build (runs in astro:build:done):
//
//   1. SHEET n/N in every page's title block (index over the sorted list of built pages).
//   2. An Open Graph card for every page, from its og:title and meta description
//      (dist/og/<path>.png; the <head> override links the same path, see src/lib/og-path.mjs).
//   3. dist/_headers from the public/_headers template: SHA-256 hashes of every inline script in
//      the built HTML, and the Cloudflare Web Analytics origins only when the beacon is enabled.
//
// Phase-2 pages need no registration here: everything is derived from the built HTML.

import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, writeFileSync, mkdirSync, statSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ogImagePath } from '../src/lib/og-path.mjs';
import { renderOgCard } from './og.mjs';
import { htmlPageMarkdown, markdownPathFor } from '../src/lib/site-markdown.mjs';

/** The landing page plan rendered by scripts/hero.mjs, for the home page's card (absent: none). */
function heroDrawing() {
	try {
		return JSON.parse(readFileSync(new URL('../src/generated/hero-plan.json', import.meta.url), 'utf8')).svg;
	} catch {
		return undefined;
	}
}

const BEACON_SCRIPT_ORIGIN = 'https://static.cloudflareinsights.com';
const BEACON_CONNECT_ORIGIN = 'https://cloudflareinsights.com';

/** @param {string} dir @returns {string[]} */
export function listFiles(dir) {
	const out = [];
	for (const name of readdirSync(dir)) {
		const p = join(dir, name);
		if (statSync(p).isDirectory()) out.push(...listFiles(p));
		else out.push(p);
	}
	return out;
}

/** URL path of a built HTML file: dist/docs/x/index.html -> /docs/x/ ; dist/404.html -> /404.html */
export function urlPathOf(distDir, file) {
	const rel = relative(distDir, file).split(sep).join('/');
	if (rel === 'index.html') return '/';
	if (rel.endsWith('/index.html')) return '/' + rel.slice(0, -'index.html'.length);
	return '/' + rel;
}

const decode = (s) =>
	s
		.replace(/&quot;/g, '"')
		.replace(/&#39;|&#x27;/g, "'")
		.replace(/&lt;/g, '<')
		.replace(/&gt;/g, '>')
		.replace(/&amp;/g, '&');

/** Attribute value of the first <meta> whose key attribute matches. */
export function metaContent(html, key, value) {
	for (const m of html.matchAll(/<meta\b[^>]*>/g)) {
		const tag = m[0];
		if (new RegExp(`\\b${key}="${value}"`).test(tag)) {
			const c = /\bcontent="([^"]*)"/.exec(tag);
			if (c) return decode(c[1]);
		}
	}
	return undefined;
}

/** Bodies of executable inline scripts (no src, JavaScript type). */
export function inlineScripts(html) {
	const out = [];
	for (const m of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)) {
		const attrs = m[1];
		if (/\bsrc=/.test(attrs)) continue;
		const type = /\btype="([^"]*)"/.exec(attrs)?.[1];
		if (type && type !== 'module' && type !== 'text/javascript') continue;
		out.push(m[2]);
	}
	return out;
}

export const scriptHash = (body) => `'sha256-${createHash('sha256').update(body, 'utf8').digest('base64')}'`;

export default function cadkitPostbuild() {
	return {
		name: 'cadkit-postbuild',
		hooks: {
			'astro:build:done': async ({ dir, logger }) => {
				const dist = fileURLToPath(dir);
				const { SITE, CADKIT_VERSION, BUILD_DATE } = await import('../src/config.ts');
				const htmlFiles = listFiles(dist).filter((f) => f.endsWith('.html'));
				const pages = htmlFiles
					.map((file) => ({ file, path: urlPathOf(dist, file) }))
					.filter((p) => !p.path.startsWith('/404'))
					.sort((a, b) => (a.path === '/' ? -1 : b.path === '/' ? 1 : a.path.localeCompare(b.path)));
				const total = pages.length;
				const width = String(total).length;
				const sheetOf = new Map(pages.map((p, i) => [p.file, `${String(i + 1).padStart(width, '0')}/${total}`]));

				const hashes = new Set();
				let cards = 0;
				const extraMarkdown = [];
				for (const file of htmlFiles) {
					let html = readFileSync(file, 'utf8');
					const sheet = sheetOf.get(file) ?? '404';
					html = html.replace(/(<span data-ck-sheet[^>]*>)[^<]*(<\/span>)/g, `$1${sheet}$2`);
					writeFileSync(file, html);

					for (const body of inlineScripts(html)) hashes.add(scriptHash(body));

					const path = urlPathOf(dist, file);
					if (path.startsWith('/404')) continue;
					const title = metaContent(html, 'property', 'og:title');
					const description = metaContent(html, 'name', 'description') ?? '';
					if (!title) {
						logger.warn(`no og:title in ${path}; skipped OG card`);
						continue;
					}
					// Collection pages already have lossless Markdown generated from their source.
					// Hand-written pages use the built content, including all code tabs and tables.
					const mdPath = markdownPathFor(path);
					if (!html.includes('data-ck-doc')) {
						const markdown = htmlPageMarkdown(html, { title, description, url: SITE + path });
						const target = join(dist, mdPath);
						mkdirSync(dirname(target), { recursive: true });
						writeFileSync(target, markdown);
						extraMarkdown.push({ path, mdPath, title, description, markdown });
					}
					const out = join(dist, ogImagePath(path));
					mkdirSync(dirname(out), { recursive: true });
					const drawing = path === '/' ? heroDrawing() : undefined;
					writeFileSync(out, renderOgCard({ title, description, path, version: CADKIT_VERSION, date: BUILD_DATE, drawing }));
					cards++;
				}

				const llmsPath = join(dist, 'llms.txt');
				let llms = readFileSync(llmsPath, 'utf8');
				const missing = [];
				for (const page of extraMarkdown.sort((a, b) => a.path.localeCompare(b.path))) {
					const htmlLink = `](${SITE}${page.path})`;
					if (llms.includes(htmlLink)) llms = llms.replaceAll(htmlLink, `](${SITE}${page.mdPath})`);
					else missing.push(`- [${page.title}](${SITE}${page.mdPath}): ${page.description}`);
				}
				if (missing.length) llms += '\n## Optional\n\n' + missing.join('\n') + '\n';
				writeFileSync(llmsPath, llms);
				const fullPath = join(dist, 'llms-full.txt');
				writeFileSync(fullPath, readFileSync(fullPath, 'utf8') + extraMarkdown.map((p) => '\n---\n\n' + p.markdown).join(''));

				const analytics = Boolean(process.env.PUBLIC_CF_WEB_ANALYTICS_TOKEN?.trim());
				const headersPath = join(dist, '_headers');
				const template = readFileSync(headersPath, 'utf8');
				const filled = template
					.replace(/^#.*\n/gm, '')
					.replaceAll('__CK_SCRIPT_HASHES__', [...hashes].sort().join(' '))
					.replaceAll('__CK_ANALYTICS_SCRIPT__', analytics ? BEACON_SCRIPT_ORIGIN : '')
					.replaceAll('__CK_ANALYTICS_CONNECT__', analytics ? BEACON_CONNECT_ORIGIN : '')
					.replace(/ {2,}(?=\S)/g, (m, offset, s) => (s[offset - 1] === '\n' ? m : ' '))
					.replace(/ +;/g, ';')
					.trimStart();
				writeFileSync(headersPath, filled);

				logger.info(
					`${total} pages (+404), ${cards} OG cards, ${hashes.size} inline script hashes, analytics ${analytics ? 'on' : 'off'}`
				);
			},
		},
	};
}
