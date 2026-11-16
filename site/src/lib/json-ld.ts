import { SITE, SITE_NAME, REPO, CADKIT_VERSION, LINKS } from '../config';
import type { Crumb } from './page-meta';

const abs = (href: string) => new URL(href, SITE).href;

/** schema.org BreadcrumbList: Home, then the given trail; the last item is the current page. */
export function breadcrumbJsonLd(crumbs: Crumb[], currentPath: string): Record<string, unknown> {
	const items = [{ label: 'Home', href: '/' }, ...crumbs];
	return {
		'@context': 'https://schema.org',
		'@type': 'BreadcrumbList',
		itemListElement: items.map((c, i) => ({
			'@type': 'ListItem',
			position: i + 1,
			name: c.label,
			item: abs(i === items.length - 1 ? currentPath : (c.href ?? currentPath)),
		})),
	};
}

export const organizationJsonLd: Record<string, unknown> = {
	'@context': 'https://schema.org',
	'@type': 'Organization',
	'@id': `${SITE}/#organization`,
	name: SITE_NAME,
	url: `${SITE}/`,
	logo: abs('/brand/cadkit-icon-256.png'),
	sameAs: [REPO, LINKS.crates, LINKS.pypi, LINKS.npm],
};

export const softwareJsonLd: Record<string, unknown> = {
	'@context': 'https://schema.org',
	'@type': 'SoftwareSourceCode',
	name: SITE_NAME,
	description:
		'Open-source CAD engine in pure Rust: read DWG, DGN (V7 and V8), DXF and CityGML 2.0; write DXF, seed-based DGN V8, CityGML 2.0, SVG and JSON.',
	url: `${SITE}/`,
	codeRepository: REPO,
	programmingLanguage: 'Rust',
	runtimePlatform: ['Rust', 'Python', 'JavaScript', 'WebAssembly', 'C'],
	license: ['https://spdx.org/licenses/MIT.html', 'https://spdx.org/licenses/Apache-2.0.html'],
	version: CADKIT_VERSION,
	publisher: { '@id': `${SITE}/#organization` },
};
