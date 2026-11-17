#!/usr/bin/env node
// Read-only post-deployment audit. No IndexNow submission, purge, or Cloudflare mutations.
import { parseSitemap } from './lib/sitemap.mjs';
import { fetchPublic, inspectPublishedPage } from './lib/published-page.mjs';
import { SITE, RELEASE_HOST } from '../src/config.ts';

const origin = new URL(process.argv[2] ?? SITE).origin;
const auditAgent = `cadkit-site-audit/1.0 (+${SITE})`;
const errors = [];
const check = (ok, message) => { if (!ok) errors.push(message); };
const request = async (path, discovery = false) => {
	const result = await fetchPublic(new URL(path, origin), auditAgent);
	if (discovery) check(result.response.headers.get('link')?.includes('rel="describedby"'), `${path}: missing Link discovery header`);
	return result;
};

try {
	const { text: robots } = await request('/robots.txt');
	check(/^User-agent: \*\s+Allow: \/$/m.test(robots) && !/^Disallow:\s*\S/m.test(robots), 'robots.txt: public pages must be crawlable');
	const { text: llms, response: indexResponse } = await request('/llms.txt');
	check(llms.startsWith('# cadkit\n'), 'llms.txt: missing project heading');
	check(!/noindex|none/i.test(indexResponse.headers.get('x-robots-tag') ?? ''), 'llms.txt: noindex header');
	const { text: full, response: fullResponse } = await request('/llms-full.txt', true);
	check(/noindex/i.test(fullResponse.headers.get('x-robots-tag') ?? ''), 'llms-full.txt: missing noindex header');
	const version = /Current release: ([\d.]+(?:-[\w.-]+)?)\./.exec(llms)?.[1];
	check(Boolean(version) && full.includes(`cadkit ${version},`), 'LLM exports disagree on their version');
	// The published download marker, not the checkout, defines the latest stable release.
	const { text: latest } = await fetchPublic(`${RELEASE_HOST}/latest.txt`, auditAgent);
	check(version === latest.trim(), `site version ${version} differs from latest release ${latest.trim()}`);
	const { text: index } = await request('/sitemap-index.xml');
	const entries = [];
	for (const { loc } of parseSitemap(index)) {
		if (new URL(loc).origin !== origin) throw new Error('foreign sitemap URL');
		const { text } = await request(loc);
		entries.push(...parseSitemap(text));
	}
	check(entries.length > 0, 'empty sitemap');
	const titles = new Set();
	const descriptions = new Set();
	let markdownCount = 0;
	// Deliberately sequential: the public origin does not need a burst of crawler traffic.
	for (const { loc } of entries) {
		if (new URL(loc).origin !== origin) throw new Error('foreign page URL');
		try {
			const { text, response } = await request(loc, true);
			const page = inspectPublishedPage(text, loc);
			errors.push(...page.issues.map((issue) => `${new URL(loc).pathname}: ${issue}`));
			check(!/noindex|none/i.test(response.headers.get('x-robots-tag') ?? ''), `${loc}: HTML noindex header`);
			check(!titles.has(page.title), `${loc}: duplicate title`); titles.add(page.title);
			check(!descriptions.has(page.description), `${loc}: duplicate description`); descriptions.add(page.description);
			if (page.markdown) {
				const mdUrl = new URL(page.markdown, origin).href;
				const { text: md, response: mdResponse } = await request(mdUrl, true);
				check(mdResponse.headers.get('content-type')?.startsWith('text/markdown'), `${mdUrl}: wrong content type`);
				check(/noindex/i.test(mdResponse.headers.get('x-robots-tag') ?? ''), `${mdUrl}: missing noindex header`);
				check(md.includes(`Canonical page: ${loc}\n`), `${mdUrl}: wrong canonical page`);
				check(llms.includes(`](${mdUrl})`), `${mdUrl}: missing from llms.txt`);
				check(full.includes(md), `${mdUrl}: differs from llms-full.txt`);
				markdownCount++;
			}
			if (page.image) await fetchPublic(new URL(page.image, origin), auditAgent);
		} catch (error) { errors.push(error.message); }
	}
	console.log(`live audit: ${entries.length} HTML pages, ${markdownCount} Markdown copies, version ${version}`);
	const agents = [
		['Python urllib', 'Python-urllib/3.12'],
		['Python requests', 'python-requests/2.32.3'],
		['Googlebot', 'Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)'],
		['OAI-SearchBot', 'Mozilla/5.0 (compatible; OAI-SearchBot/1.4; +https://openai.com/searchbot)'],
		['GPTBot', 'Mozilla/5.0 (compatible; GPTBot/1.4; +https://openai.com/gptbot)'],
		['ClaudeBot', 'ClaudeBot/1.0; +https://anthropic.com/claudebot'],
		['PerplexityBot', 'Mozilla/5.0 (compatible; PerplexityBot/1.0; +https://perplexity.ai/perplexitybot)'],
	];
	for (const [label, agent] of agents) {
		for (const path of ['/', '/robots.txt', '/llms.txt', '/llms-full.txt', '/docs/getting-started/rust.md', '/sitemap-index.xml']) {
			try { await fetchPublic(new URL(path, origin), agent); }
			catch (error) { errors.push(`${label}: ${error.message}`); }
		}
	}
} catch (error) { errors.push(error.message); }

if (errors.length) {
	for (const error of errors) console.error('error: ' + error);
	console.error(`live audit failed: ${errors.length} issue(s). User-agent probes do not prove access from real crawler IPs.`);
	process.exitCode = 1;
} else console.log('live audit: ok (real crawler access and indexing still require provider logs)');
