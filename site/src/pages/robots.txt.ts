// robots.txt: the whole site is public. Search and AI crawlers are named so that the policy is
// stated rather than implied by the wildcard; /llms.txt is the entry point for the latter.
import type { APIRoute } from 'astro';
import { SITE } from '../config';

/** Crawlers of search engines and AI assistants that honour a group of their own. */
const NAMED_CRAWLERS = [
	'Googlebot',
	'Bingbot',
	'DuckDuckBot',
	'YandexBot',
	'Applebot',
	'Google-Extended',
	'Applebot-Extended',
	'GPTBot',
	'OAI-SearchBot',
	'ChatGPT-User',
	'ClaudeBot',
	'Claude-SearchBot',
	'Claude-User',
	'PerplexityBot',
	'Perplexity-User',
	'CCBot',
] as const;

export const GET: APIRoute = () => {
	const body = [
		`# ${SITE}: everything is public. Summary for language models: ${SITE}/llms.txt`,
		'User-agent: *',
		'Allow: /',
		'',
		...NAMED_CRAWLERS.map((name) => `User-agent: ${name}`),
		'Allow: /',
		'',
		`Sitemap: ${SITE}/sitemap-index.xml`,
		'',
	].join('\n');
	return new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
