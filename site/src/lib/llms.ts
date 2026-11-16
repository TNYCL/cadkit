// Markdown view of the documentation, for /llms.txt, /llms-full.txt and the per-page ".md"
// files (https://llmstxt.org). Runs at build time only.

import { getCollection } from 'astro:content';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { SITE, CADKIT_VERSION } from '../config';
import { sidebar } from '../sidebar';
import { mdxToMarkdown, absoluteLinks, markdownDocument } from './md-export.mjs';

export interface DocPage {
	/** Collection id, e.g. "docs/getting-started/rust", "docs", "changelog". */
	id: string;
	/** Site path of the HTML page, e.g. "/docs/getting-started/rust/". */
	route: string;
	/** Site path of the Markdown file, e.g. "/docs/getting-started/rust.md". */
	mdPath: string;
	title: string;
	description: string;
	/** The page as a standalone Markdown document with absolute links. */
	markdown: string;
}

/** One entry of a sidebar group, flattened: a docs page (slug) or a plain link. */
export interface NavEntry {
	label: string;
	slug?: string;
	link?: string;
}

/** Sidebar groups in order, nested groups flattened into their parent. */
export function sidebarSections(): { label: string; entries: NavEntry[] }[] {
	const flatten = (items: readonly unknown[]): NavEntry[] =>
		items.flatMap((item): NavEntry[] => {
			if (typeof item !== 'object' || item === null) return [];
			const it = item as { label?: string; slug?: string; link?: string; items?: readonly unknown[] };
			if (it.items) return flatten(it.items);
			if (!it.label) return [];
			if (it.slug) return [{ label: it.label, slug: it.slug }];
			return it.link ? [{ label: it.label, link: it.link }] : [];
		});
	return sidebar.flatMap((group) => {
		if (typeof group !== 'object' || group === null || !('items' in group) || !group.items) return [];
		return [{ label: String(group.label), entries: flatten(group.items) }];
	});
}

let cache: Promise<DocPage[]> | undefined;

/** Every documentation page, in sidebar order (pages outside the sidebar follow, by id). */
export function docPages(): Promise<DocPage[]> {
	cache ??= load();
	return cache;
}

async function load(): Promise<DocPage[]> {
	const order = sidebarSections().flatMap((s) => s.entries.flatMap((e) => (e.slug ? [e.slug] : [])));
	const rank = (id: string) => (order.includes(id) ? order.indexOf(id) : order.length);
	const entries = (await getCollection('docs')).sort((a, b) => rank(a.id) - rank(b.id) || a.id.localeCompare(b.id));
	return entries.map((entry) => {
		const id = entry.id.replace(/(^|\/)index$/, '');
		if (id === '') throw new Error('a docs entry at the site root has no Markdown path');
		const route = `/${id}/`;
		const file = entry.filePath ? join(process.cwd(), entry.filePath) : undefined;
		let body = entry.body ?? '';
		if (file?.endsWith('.mdx')) {
			try {
				body = mdxToMarkdown(body, {
					readImport: (specifier: string) => readFileSync(join(dirname(file), specifier), 'utf8'),
					vars: { CADKIT_VERSION },
				});
			} catch (e) {
				throw new Error(`${entry.filePath}: ${(e as Error).message}`);
			}
		}
		const { title, description = '' } = entry.data;
		return {
			id,
			route,
			mdPath: `/${id}.md`,
			title,
			description,
			markdown: markdownDocument({ title, description, url: SITE + route, body: absoluteLinks(body, SITE) }),
		};
	});
}
