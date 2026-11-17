// Markdown copies of hand-written pages are derived from the rendered <main>, so examples,
// support tables and statistics have the same source as the HTML. Docs keep their MDX export.
import { parse, preformattedText, ELEMENT_NODE, TEXT_NODE } from './html-tree.mjs';
import { fence, markdownDocument, normalizeMarkdownSpacing } from './md-export.mjs';

/** Markdown URL for a canonical HTML route (the home page uses /index.md). */
export function markdownPathFor(path) {
	return path === '/' ? '/index.md' : path.replace(/\/$/, '') + '.md';
}

const OMIT = new Set(['script', 'style', 'svg', 'nav', 'button', 'form', 'input', 'select']);
const BLOCKS = new Set(['article', 'section', 'header', 'footer', 'div', 'p', 'figure', 'figcaption', 'details', 'summary', 'dl', 'dt', 'dd']);
const classes = (node) => (node.attributes?.class ?? '').split(/\s+/);
const children = (node) => node.children ?? [];
const find = (node, predicate) => predicate(node) ? node : children(node).map((c) => find(c, predicate)).find(Boolean);

function omitted(node) {
	return node.type === ELEMENT_NODE && (
		OMIT.has(node.name) || node.attributes['aria-hidden'] === 'true' ||
		('hidden' in node.attributes && node.attributes.role !== 'tabpanel') ||
		'data-pagefind-ignore' in node.attributes || classes(node).includes('sr-only')
	);
}

function textOf(node) {
	if (node.type === TEXT_NODE) return node.value;
	return children(node).map(textOf).join('');
}

const escapeText = (text) => text.replace(/([\\`*_[\]<>|])/g, '\\$1');
const compact = (text) => text.replace(/\s+/g, ' ').trim();

/** Export one rendered page. UI controls and SVGs are omitted; all code tabs and tables remain. */
export function htmlPageMarkdown(html, { title, description, url }) {
	const tree = parse(html);
	const main = find(tree, (n) => n.type === ELEMENT_NODE && n.name === 'main');
	if (!main) throw new Error(`${url}: missing <main> for Markdown export`);
	let firstHeading = true;
	const renderChildren = (node) => children(node).map(render).join('');
	function render(node) {
		if (node.type === TEXT_NODE) return escapeText(node.value.replace(/\s+/g, ' '));
		if (node.type !== ELEMENT_NODE || omitted(node)) return '';
		const a = node.attributes;
		if (node.name === 'h1' && firstHeading) { firstHeading = false; return ''; }
		if (/^h[1-6]$/.test(node.name)) return `\n\n${'#'.repeat(Number(node.name[1]))} ${compact(renderChildren(node))}\n\n`;
		if (node.name === 'pre') {
			const code = find(node, (n) => n.type === ELEMENT_NODE && n.name === 'code') ?? node;
			const source = preformattedText(node);
			const lang = a['data-language'] ?? /language-([\w-]+)/.exec(code.attributes?.class ?? '')?.[1] ?? '';
			return '\n\n' + fence(source, lang) + '\n\n';
		}
		if (node.name === 'code') {
			const value = textOf(node);
			const marks = '`'.repeat(Math.max(0, ...[...value.matchAll(/`+/g)].map((m) => m[0].length)) + 1);
			const pad = value.startsWith('`') || value.endsWith('`') ? ' ' : '';
			return marks + pad + value + pad + marks;
		}
		if (a.role === 'tablist') return '';
		if (a.role === 'tabpanel') {
			const label = find(tree, (n) => n.type === ELEMENT_NODE && n.attributes?.id === a['aria-labelledby']);
			return '\n\n' + (label ? '**' + escapeText(compact(textOf(label))) + '**\n\n' : '') + renderChildren(node) + '\n\n';
		}
		if (node.name === 'a') {
			const label = compact(renderChildren(node));
			if (!a.href || !label) return label;
			const target = new URL(a.href, url);
			if (!['http:', 'https:', 'mailto:'].includes(target.protocol)) return label;
			return `[${label}](${target.href.replaceAll('(', '%28').replaceAll(')', '%29')})`;
		}
		if (node.name === 'img') return a.alt ? escapeText(a.alt) : '';
		if (node.name === 'br') return '\n';
		if (node.name === 'strong' || node.name === 'b') return `**${renderChildren(node)}**`;
		if (node.name === 'em' || node.name === 'i') return `*${renderChildren(node)}*`;
		if (node.name === 'blockquote') return '\n\n' + renderChildren(node).trim().split('\n').map((l) => '> ' + l).join('\n') + '\n\n';
		if (node.name === 'ul' || node.name === 'ol') {
			const items = children(node).filter((n) => n.type === ELEMENT_NODE && n.name === 'li');
			return '\n\n' + items.map((n, i) => {
				const body = normalizeMarkdownSpacing(renderChildren(n)).trim();
				if (!body) return '';
				const marker = node.name === 'ol' ? `${i + 1}. ` : '- ';
				return marker + body.replaceAll('\n', '\n' + ' '.repeat(marker.length));
			}).filter(Boolean).join('\n') + '\n\n';
		}
		if (node.name === 'table') {
			const rows = [];
			const collect = (n) => {
				if (n.type === ELEMENT_NODE && n.name === 'tr') {
					rows.push(children(n).filter((c) => c.name === 'th' || c.name === 'td').map((c) => compact(renderChildren(c))));
				} else children(n).forEach(collect);
			};
			collect(node);
			if (!rows.length) return '';
			const width = Math.max(...rows.map((r) => r.length));
			const row = (cells) => '| ' + Array.from({ length: width }, (_, i) => cells[i] ?? '').join(' | ') + ' |';
			return '\n\n' + [row(rows[0]), row(Array(width).fill('---')), ...rows.slice(1).map(row)].join('\n') + '\n\n';
		}
		const body = children(node).map((child, index, siblings) => {
			const next = siblings[index + 1];
			return render(child) + (child.name === 'a' && next?.name === 'a' ? ' ' : '');
		}).join('');
		if (a.role === 'tab') return '\n\n**' + compact(body) + '**\n\n';
		if (BLOCKS.has(node.name)) return '\n\n' + body.trim() + '\n\n';
		return node.name === 'span' ? ' ' + body + ' ' : body;
	}
	const body = normalizeMarkdownSpacing(renderChildren(main)).trim();
	if (!body) throw new Error(`${url}: empty Markdown export`);
	return markdownDocument({ title, description, url, body });
}
