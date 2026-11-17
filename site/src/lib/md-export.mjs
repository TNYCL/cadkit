// Turns the body of a docs page into plain Markdown for /llms-full.txt and the per-page ".md"
// files (pure functions; unit-tested in test/seo.test.mjs).
//
// Synced reference pages are Markdown already. Hand-written pages are MDX that use exactly
// three components: <Code> and <CodeTabs> (code samples imported with "?raw") and <Callout>.
// Each becomes fenced code or a block quote. Anything else fails the build: a silently
// dropped component would publish a page with a hole in it.

import { tagLines, rewriteLinks } from '../../scripts/docs/transform.mjs';

/** A fenced code block whose fence is longer than any backtick run inside the code. */
export function fence(code, lang, title) {
	const longest = Math.max(2, ...[...code.matchAll(/`+/g)].map((m) => m[0].length));
	const marks = '`'.repeat(longest + 1);
	const info = (lang ?? '') + (title ? ` title="${title.replaceAll('"', "'")}"` : '');
	return marks + info + '\n' + code.replace(/\s+$/, '') + '\n' + marks;
}

/** Applies fn to the text outside fenced code blocks and leaves the blocks untouched. */
function outsideFences(markdown, fn) {
	const out = [];
	let run = [];
	let inCode = false;
	const flush = () => {
		if (!run.length) return;
		out.push(inCode ? run.join('\n') : fn(run.join('\n')));
		run = [];
	};
	for (const { line, code } of tagLines(markdown)) {
		if (code !== inCode) {
			flush();
			inCode = code;
		}
		run.push(line);
	}
	flush();
	return out.join('\n');
}

/**
 * @param {string} body  MDX body (no frontmatter).
 * @param {object} ctx
 * @param {(specifier: string) => string} ctx.readImport  Text of a file imported with "?raw".
 * @param {Record<string, string>} ctx.vars  Values of the identifiers a <Code> template may use.
 * @returns {string} Markdown.
 */
export function mdxToMarkdown(body, { readImport, vars }) {
	/** @type {Map<string, string>} import name -> file text */
	const raw = new Map();
	const kept = [];
	for (const { line, code } of tagLines(body)) {
		const m = code ? null : /^import\s+(.+?)\s+from\s+['"]([^'"]+)['"];?\s*$/.exec(line);
		if (!m) {
			kept.push(line);
			continue;
		}
		if (m[2].endsWith('?raw')) raw.set(m[1], readImport(m[2].slice(0, -'?raw'.length)));
	}
	const sample = (name) => {
		const text = raw.get(name);
		if (text === undefined) throw new Error(`code sample "${name}" is not imported with ?raw`);
		return text;
	};
	const template = (text) =>
		text
			.replace(/\$\{(\w+)\}/g, (_, name) => {
				if (vars[name] === undefined) throw new Error(`unknown value ${name} in a <Code> template`);
				return vars[name];
			})
			.replace(/\\(n|`|\\)/g, (_, c) => (c === 'n' ? '\n' : c));

	const converted = outsideFences(kept.join('\n'), (text) =>
		text
			.replace(/<CodeTabs\s+snippets=\{\[([\s\S]*?)\]\}\s*\/>/g, (_, list) => {
				const items = [...list.matchAll(/\{\s*label:\s*'([^']*)',\s*lang:\s*'([^']*)',\s*code:\s*(\w+)(?:,\s*title:\s*'([^']*)')?\s*,?\s*\}/g)];
				if (items.length === 0 || items.length !== (list.match(/\{/g) ?? []).length) throw new Error('unsupported <CodeTabs> snippets list');
				return items.map(([, label, lang, name, title]) => `**${label}**\n\n${fence(sample(name), lang, title)}`).join('\n\n');
			})
			.replace(/<Code\s+code=\{(\w+)\}\s+lang="([^"]*)"(?:\s+title="([^"]*)")?\s*\/>/g, (_, name, lang, title) => fence(sample(name), lang, title))
			.replace(/<Code\s+code=\{`((?:[^`\\]|\\.)*)`\}\s+lang="([^"]*)"(?:\s+title="([^"]*)")?\s*\/>/g, (_, text, lang, title) =>
				fence(template(text), lang, title)
			)
			.replace(/<Callout(?:\s+variant="\w+")?(?:\s+title="([^"]*)")?\s*>([\s\S]*?)<\/Callout>/g, (_, title, inner) => {
				const lines = inner.split('\n').map((l) => l.trim());
				while (lines.length && lines[0] === '') lines.shift();
				while (lines.length && lines.at(-1) === '') lines.pop();
				if (title) lines.unshift(`**${title}**`, '');
				return lines.map((l) => (l ? '> ' + l : '>')).join('\n');
			})
	);

	// Whatever JSX is left (outside code and inline code) is a component this export does not know.
	for (const { line, code } of tagLines(converted)) {
		if (code) continue;
		const left = /<\/?[A-Z][A-Za-z]*[\s/>]|^\s*\{/.exec(line.replace(/`+[^`]*`+/g, ''));
		if (left) throw new Error(`unsupported MDX syntax "${left[0].trim()}" in: ${line.trim().slice(0, 80)}`);
	}
	return normalizeMarkdownSpacing(converted).trim() + '\n';
}

/** Compact prose spacing without changing blank lines inside source examples. */
export function normalizeMarkdownSpacing(markdown) {
	return outsideFences(markdown, (text) => text.replace(/[ \t]+\n/g, '\n').replace(/\n{3,}/g, '\n\n'));
}

/** Makes site-relative link targets ("/docs/x/") absolute, outside code. */
export function absoluteLinks(markdown, site) {
	return rewriteLinks(markdown, (target) => (target.startsWith('/') && !target.startsWith('//') ? site + target : undefined));
}

/** One page as a standalone Markdown document. */
export function markdownDocument({ title, description, url, body }) {
	return `# ${title}\n\n> ${description}\n\nCanonical page: ${url}\n\n${body.trim()}\n`;
}
