// Checks the response after the CDN has applied redirects, bot rules and HTML transforms.
import { parse, walkSync, ELEMENT_NODE, TEXT_NODE } from '../../src/lib/html-tree.mjs';
import { markdownPathFor } from '../../src/lib/site-markdown.mjs';

/** Public-page metadata used by the live audit and its synthetic HTTP tests. */
export function inspectPublishedPage(html, url) {
	const issues = [];
	const elements = [];
	walkSync(parse(html), (node) => { if (node.type === ELEMENT_NODE) elements.push(node); });
	const links = elements.filter((n) => n.name === 'link');
	const ids = new Set(elements.map((n) => n.attributes.id).filter(Boolean));
	const meta = (key) => elements.find((n) => n.name === 'meta' && (n.attributes.name === key || n.attributes.property === key))?.attributes.content;
	const rel = (value) => links.find((n) => n.attributes.rel === value)?.attributes.href;
	const noindex = /\b(?:noindex|none)\b/i.test(meta('robots') ?? '');
	if (noindex) issues.push('HTML is noindex');
	if (rel('canonical') !== url) issues.push('canonical differs from the requested page');
	if (rel('describedby') !== '/llms.txt') issues.push('missing llms.txt discovery link');
	const markdown = links.find((n) => n.attributes.rel === 'alternate' && n.attributes.type === 'text/markdown')?.attributes.href;
	if (markdown !== markdownPathFor(new URL(url).pathname)) issues.push('missing or wrong Markdown alternate');
	const h1 = elements.filter((n) => n.name === 'h1');
	if (h1.length !== 1) issues.push('expected one h1');
	const skip = elements.find((n) => n.name === 'a' && (n.attributes.class ?? '').split(/\s+/).includes('sl-skip-link'));
	if (!skip?.attributes.href?.startsWith('#') || !ids.has(skip.attributes.href.slice(1))) issues.push('skip link has no content target');
	if (!meta('description')) issues.push('missing description');
	if (/__cf_email__|\/cdn-cgi\/l\/email-protection/.test(html)) issues.push('Cloudflare has obfuscated public contact addresses');
	const textOf = (n) => n.type === TEXT_NODE ? n.value : (n.children ?? []).map(textOf).join('');
	const title = elements.find((n) => n.name === 'head')?.children.find((n) => n.name === 'title');
	return { issues, markdown, title: title ? textOf(title) : '', description: meta('description') ?? '', image: meta('og:image') };
}

/** Fetch without a browser or authentication; fail on a challenge, error or oversized body. */
export async function fetchPublic(url, userAgent, maxBytes = 2 * 1024 * 1024) {
	const response = await fetch(url, { headers: { 'User-Agent': userAgent }, signal: AbortSignal.timeout(30_000) });
	if (response.status !== 200) throw new Error(`${new URL(url).pathname}: HTTP ${response.status}`);
	if (response.headers.has('cf-mitigated')) throw new Error(`${new URL(url).pathname}: CDN challenge`);
	const chunks = [];
	let size = 0;
	for await (const chunk of response.body ?? []) {
		size += chunk.length;
		if (size > maxBytes) throw new Error(`${new URL(url).pathname}: exceeds ${maxBytes} bytes`);
		chunks.push(chunk);
	}
	return { response, text: Buffer.concat(chunks).toString('utf8') };
}
