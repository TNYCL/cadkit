// Parse browser HTML, including raw <script> text. Token-only parsers can mistake strings in
// copy-button scripts for elements and silently swallow the following content.
import { parse as parseHtml } from 'parse5';

export const ELEMENT_NODE = 1;
export const TEXT_NODE = 2;

/** Small tree shared by the Markdown export and the post-deployment audit. */
export function parse(html) {
	const convert = (node) => ({
		type: node.tagName ? ELEMENT_NODE : node.nodeName === '#text' ? TEXT_NODE : 0,
		name: node.tagName,
		value: node.value ?? '',
		attributes: Object.fromEntries((node.attrs ?? []).map((a) => [a.name, a.value])),
		children: (node.childNodes ?? []).map(convert),
	});
	return convert(parseHtml(html));
}

/** Visit each node in document order. */
export function walkSync(node, visit) {
	visit(node);
	for (const child of node.children) walkSync(child, visit);
}

/** Source code inside a rendered pre. Expressive Code uses a newline sentinel for blank lines. */
export function preformattedText(pre) {
	const text = (node) => node.type === TEXT_NODE ? node.value : node.children.map(text).join('');
	const code = pre.children.find((n) => n.name === 'code') ?? pre;
	const lines = code.children.filter((n) => (n.attributes.class ?? '').split(/\s+/).includes('ec-line'));
	return lines.length ? lines.map((line) => text(line).replace(/\n$/, '')).join('\n') : text(code);
}
