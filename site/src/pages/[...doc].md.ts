// Markdown version of every documentation page: /docs/getting-started/rust/ is also served as
// /docs/getting-started/rust.md (linked from llms.txt and from the page's <head>).
import type { APIRoute, GetStaticPaths } from 'astro';
import { docPages } from '../lib/llms';

export const getStaticPaths: GetStaticPaths = async () =>
	(await docPages()).map((page) => ({ params: { doc: page.id }, props: { markdown: page.markdown } }));

export const GET: APIRoute = ({ props }) =>
	new Response((props as { markdown: string }).markdown, { headers: { 'Content-Type': 'text/markdown; charset=utf-8' } });
