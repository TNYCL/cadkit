// @ts-check
import { defineConfig, fontProviders } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';
import { existsSync, readFileSync } from 'node:fs';
import { SITE, REPO } from './src/config.ts';
import { sidebar } from './src/sidebar.ts';
import cadkitPostbuild from './scripts/postbuild.mjs';

// Site path -> date of the last content change, written by scripts/prebuild.d/40-lastmod.mjs.
// Empty when the git history is unavailable: those pages get no <lastmod>.
const lastmodFile = new URL('./src/generated/lastmod.json', import.meta.url);
/** @type {Record<string, string>} */
const lastmod = existsSync(lastmodFile) ? JSON.parse(readFileSync(lastmodFile, 'utf8')) : {};

const latin =
	'U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,U+20AC,U+2122,U+2191,U+2193,U+2212,U+2215,U+FEFF,U+FFFD';

export default defineConfig({
	site: SITE,
	trailingSlash: 'always',
	build: { format: 'directory' },
	// Self-hosted variable fonts (latin subset) with metric-matched fallbacks to avoid layout shift.
	fonts: [
		{
			provider: fontProviders.local(),
			name: 'Archivo',
			cssVariable: '--ck-font-sans-face',
			fallbacks: ['Arial', 'sans-serif'],
			options: {
				variants: [
					{
						src: ['./node_modules/@fontsource-variable/archivo/files/archivo-latin-wdth-normal.woff2'],
						weight: '100 900',
						stretch: '62% 125%',
						style: 'normal',
						unicodeRange: [latin],
					},
				],
			},
		},
		{
			provider: fontProviders.local(),
			name: 'JetBrains Mono',
			cssVariable: '--ck-font-mono-face',
			fallbacks: ['monospace'],
			options: {
				variants: [
					{
						src: ['./node_modules/@fontsource-variable/jetbrains-mono/files/jetbrains-mono-latin-wght-normal.woff2'],
						weight: '100 800',
						style: 'normal',
						unicodeRange: [latin],
					},
				],
			},
		},
	],
	integrations: [
		// Listed before Starlight, which then keeps this instance instead of adding its own.
		sitemap({
			serialize(item) {
				const date = lastmod[new URL(item.url).pathname];
				return date ? { ...item, lastmod: date } : item;
			},
		}),
		starlight({
			title: 'cadkit',
			description:
				'Open-source CAD engine in pure Rust. Read DWG, DGN, DXF and CityGML; write DXF, seed-based DGN V8, CityGML, SVG and JSON from Rust, Python, JavaScript and C.',
			titleDelimiter: '·',
			favicon: '/favicon.svg',
			head: [
				{ tag: 'link', attrs: { rel: 'icon', href: '/favicon.ico', sizes: '32x32' } },
				{ tag: 'link', attrs: { rel: 'apple-touch-icon', href: '/apple-touch-icon.png' } },
			],
			social: [{ icon: 'github', label: 'GitHub', href: REPO }],
			sidebar,
			customCss: ['./src/styles/tokens.css', './src/styles/global.css'],
			disable404Route: true,
			credits: false,
			lastUpdated: false,
			components: {
				Head: './src/components/starlight/Head.astro',
				Header: './src/components/starlight/Header.astro',
				PageFrame: './src/components/starlight/PageFrame.astro',
				ThemeSelect: './src/components/starlight/ThemeSelect.astro',
				PageTitle: './src/components/starlight/PageTitle.astro',
				ContentPanel: './src/components/starlight/ContentPanel.astro',
				MarkdownContent: './src/components/starlight/MarkdownContent.astro',
				Footer: './src/components/starlight/Footer.astro',
				MobileMenuFooter: './src/components/starlight/MobileMenuFooter.astro',
			},
		}),
		cadkitPostbuild(),
	],
});
