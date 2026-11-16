// Third-party works this site serves that need their licence text shipped with them.
// The fonts are SIL Open Font License 1.1; /licenses/<name>.txt serves each package's LICENSE
// verbatim (src/pages/licenses/[name].txt.ts), linked from /brand/.
export interface ServedLicense {
	/** URL name: /licenses/<name>.txt */
	name: string;
	/** What the licence applies to on this site. */
	work: string;
	/** Licence file, relative to the site root (resolved from the installed package). */
	file: string;
	/** Upstream project. */
	upstream: string;
	spdx: string;
}

export const SERVED_LICENSES: readonly ServedLicense[] = [
	{
		name: 'archivo',
		work: 'Archivo variable font (headings, body text and the outlined wordmark)',
		file: 'node_modules/@fontsource-variable/archivo/LICENSE',
		upstream: 'https://github.com/Omnibus-Type/Archivo',
		spdx: 'OFL-1.1',
	},
	{
		name: 'jetbrains-mono',
		work: 'JetBrains Mono variable font (code, coordinates and labels)',
		file: 'node_modules/@fontsource-variable/jetbrains-mono/LICENSE',
		upstream: 'https://github.com/JetBrains/JetBrainsMono',
		spdx: 'OFL-1.1',
	},
];

export const licenseHref = (name: string): string => `/licenses/${name}.txt`;
