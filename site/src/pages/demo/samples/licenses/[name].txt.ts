// /demo/samples/licenses/<name>.txt: the licence of each third-party sample's project, copied
// verbatim (with a short source header) from its pinned upstream commit into
// scripts/samples/licenses/.
import type { APIRoute, GetStaticPaths } from 'astro';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { SAMPLES } from '../../../../demo/samples';

const copies = [...new Set(SAMPLES.flatMap((s) => (s.upstream ? [s.upstream.licenseCopy] : [])))];

export const getStaticPaths = (() =>
	copies.map((copy) => ({ params: { name: copy.split('/').pop()!.replace(/\.txt$/, '') }, props: { copy } }))) satisfies GetStaticPaths;

export const GET: APIRoute = ({ props }) => {
	const text = readFileSync(join(process.cwd(), (props as { copy: string }).copy), 'utf8');
	return new Response(text, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
