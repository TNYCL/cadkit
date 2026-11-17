// Single source of site-wide facts. Every page, component and build script reads from here.
// Values are public (this repository is open source): never put a key, token or private URL here.

import wasmPackage from 'cadkit-wasm/package.json' with { type: 'json' };

/** Canonical origin (apex; www redirects here at the Cloudflare level). No trailing slash. */
export const SITE = 'https://cadkit.org';
export const SITE_NAME = 'cadkit';

/** Source repository. Moves to a GitHub organisation later: change it only here. */
export const REPO = 'https://github.com/TNYCL/cadkit';
/** Release files (CLI and C API archives, SHA256SUMS) under /v<version>/. Same value as MIRROR_URL in scripts/release.py. */
export const RELEASE_HOST = 'https://release.cadkit.org';
export const STATS_REPO = 'https://github.com/TNYCL/cadkit-stats';
export const STATS_SUMMARY_URL =
	'https://raw.githubusercontent.com/TNYCL/cadkit-stats/main/data/summary.json';

/** Version shown on the site: the installed cadkit-wasm package (all packages release together). */
export const CADKIT_VERSION: string = wasmPackage.version;

/**
 * Build date (UTC). Honours SOURCE_DATE_EPOCH for reproducible builds.
 * Used in the footer title block, security.txt and OG cards.
 */
export const BUILD_TIME: Date = (() => {
	const epoch = globalThis.process?.env?.SOURCE_DATE_EPOCH;
	const seconds = epoch ? Number.parseInt(epoch, 10) : Number.NaN;
	return Number.isFinite(seconds) ? new Date(seconds * 1000) : new Date();
})();
export const BUILD_DATE: string = BUILD_TIME.toISOString().slice(0, 10);

export const LICENSE = 'MIT OR Apache-2.0';
export const COPYRIGHT = '© 2026 TNYCL';

/** Role addresses, all routed by the maintainers. No personal address is ever published. */
export const EMAIL = {
	security: 'security@cadkit.org',
	conduct: 'conduct@cadkit.org',
	hello: 'hello@cadkit.org',
	maintainers: 'maintainers@cadkit.org',
} as const;

/** Published crates (crates.io). */
export const CRATES = [
	'cadkit',
	'cadkit-core',
	'cadkit-dwg',
	'cadkit-dgn',
	'cadkit-dxf',
	'cadkit-gml',
	'cadkit-geom',
	'cadkit-scene',
	'cadkit-gpu',
	'cadkit-cli',
	'cadkit-capi',
] as const;
export const PYPI_PACKAGE = 'pycadkit';
export const NPM_PACKAGE = 'cadkit-wasm';

export const repoFile = (path: string): string => `${REPO}/blob/main/${path}`;
export const crateUrl = (name: string): string => `https://crates.io/crates/${name}`;
export const docsRsUrl = (name: string): string => `https://docs.rs/${name}`;

export const LINKS = {
	repo: REPO,
	issues: `${REPO}/issues`,
	discussions: `${REPO}/discussions`,
	releases: `${REPO}/releases/latest`,
	securityAdvisory: `${REPO}/security/advisories/new`,
	changelogSource: repoFile('CHANGELOG.md'),
	provenanceSource: repoFile('docs/PROVENANCE.md'),
	interchangeStatus: repoFile('docs/INTERCHANGE_STATUS.md'),
	licenseMit: repoFile('LICENSE-MIT'),
	licenseApache: repoFile('LICENSE-APACHE'),
	crates: crateUrl('cadkit'),
	docsRs: docsRsUrl('cadkit'),
	pypi: `https://pypi.org/project/${PYPI_PACKAGE}/`,
	npm: `https://www.npmjs.com/package/${NPM_PACKAGE}`,
	statsRepo: STATS_REPO,
} as const;

/** Adopters. One line each, stated exactly; an adopter is not a sponsor. */
export const USED_BY = [
	{
		name: 'Cady',
		url: 'https://usecady.com',
		description: 'an architectural CAD application',
		modules: ['cadkit-geom', 'cadkit-scene', 'cadkit-gpu'],
	},
] as const;

export interface NavLink {
	label: string;
	href: string;
	external?: boolean;
}

/** Primary navigation (header and mobile menu). */
export const NAV: readonly NavLink[] = [
	{ label: 'Docs', href: '/docs/' },
	{ label: 'Demo', href: '/demo/' },
	{ label: 'Formats', href: '/formats/' },
	{ label: 'Changelog', href: '/changelog/' },
	{ label: 'Community', href: '/community/' },
];

/** Footer link columns. */
export const FOOTER_NAV: readonly { title: string; links: readonly NavLink[] }[] = [
	{
		title: 'Project',
		links: [
			{ label: 'Docs', href: '/docs/' },
			{ label: 'Demo', href: '/demo/' },
			{ label: 'Changelog', href: '/changelog/' },
			{ label: 'Provenance', href: '/provenance/' },
			{ label: 'Stats', href: '/stats/' },
		],
	},
	{
		title: 'Formats',
		links: [
			{ label: 'DWG', href: '/formats/dwg/' },
			{ label: 'DGN', href: '/formats/dgn/' },
			{ label: 'DXF', href: '/formats/dxf/' },
			{ label: 'CityGML', href: '/formats/citygml/' },
		],
	},
	{
		title: 'Community',
		links: [
			{ label: 'Contribute', href: '/community/' },
			{ label: 'Security', href: '/security/' },
			{ label: 'GitHub', href: REPO, external: true },
			{ label: 'Discussions', href: `${REPO}/discussions`, external: true },
		],
	},
	{
		title: 'Legal',
		links: [
			{ label: 'Privacy', href: '/privacy/' },
			{ label: 'Trademarks', href: '/trademarks/' },
			{ label: 'Brand', href: '/brand/' },
			{ label: 'License', href: `${REPO}#license`, external: true },
		],
	},
];

/** Paths whose pages never load the analytics beacon. */
export const NO_ANALYTICS_PREFIXES = ['/demo/'] as const;
