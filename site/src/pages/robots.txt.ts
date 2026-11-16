import type { APIRoute } from 'astro';
import { SITE } from '../config';

export const GET: APIRoute = () =>
	new Response(`User-agent: *\nAllow: /\n\nSitemap: ${SITE}/sitemap-index.xml\n`, {
		headers: { 'Content-Type': 'text/plain; charset=utf-8' },
	});
