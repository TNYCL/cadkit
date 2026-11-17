// llms-full.txt: the whole documentation as one Markdown file, in sidebar order.
import type { APIRoute } from 'astro';
import { SITE, CADKIT_VERSION, LICENSE } from '../config';
import { docPages } from '../lib/llms';

export const GET: APIRoute = async () => {
	const pages = await docPages();
	const head = `# cadkit documentation

> The complete documentation and website content of cadkit ${CADKIT_VERSION}, the open-source CAD engine in pure Rust (${LICENSE}), as one file. Index with one link per page: ${SITE}/llms.txt
`;
	const body = [head, ...pages.map((p) => p.markdown)].join('\n---\n\n');
	return new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
