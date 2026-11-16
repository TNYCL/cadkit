// /licenses/<name>.txt: the licence of each third-party font this site serves, copied verbatim
// (with a short header) from the installed package at build time. The OFL asks that the licence
// travels with the font; the font files themselves carry only the copyright and a licence URL.
import type { APIRoute, GetStaticPaths } from 'astro';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { SERVED_LICENSES, type ServedLicense } from '../../data/licenses';

export const getStaticPaths = (() =>
	SERVED_LICENSES.map((l) => ({ params: { name: l.name }, props: { license: l } }))) satisfies GetStaticPaths;

export const GET: APIRoute = ({ props }) => {
	const l = (props as { license: ServedLicense }).license;
	const text = readFileSync(join(process.cwd(), l.file), 'utf8');
	const header = [
		`Applies to: ${l.work}, served by cadkit.org.`,
		`Upstream: ${l.upstream} (${l.spdx})`,
		'Copied verbatim below the line from the installed @fontsource-variable package.',
		'-------',
		'',
	].join('\n');
	return new Response(header + text, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
