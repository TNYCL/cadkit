// Pure Markdown transforms for the docs sync (no file system access; unit-tested in
// test/docs-sync.test.mjs).

import { posix } from 'node:path';

/** Splits Markdown into lines tagged as inside or outside fenced code blocks. */
export function tagLines(markdown) {
	const out = [];
	let fence = null;
	for (const line of markdown.split('\n')) {
		const m = /^\s{0,3}(`{3,}|~{3,})/.exec(line);
		if (fence) {
			out.push({ line, code: true });
			// A closing fence uses the opening character, at least as many times, and nothing else.
			if (m && m[1][0] === fence[0] && m[1].length >= fence.length && /^\s{0,3}[`~]+\s*$/.test(line)) fence = null;
		} else if (m) {
			fence = m[1];
			out.push({ line, code: true });
		} else out.push({ line, code: false });
	}
	return out;
}

/**
 * Removes the first level-1 ATX heading (outside code) and returns its text as the title.
 * Throws when the document has none: every synced page needs a title.
 */
export function extractTitle(markdown) {
	const tagged = tagLines(markdown);
	const index = tagged.findIndex((t) => !t.code && /^# \S/.test(t.line));
	if (index < 0) throw new Error('no level-1 heading');
	const title = tagged[index].line.replace(/^# /, '').replace(/\s+#*\s*$/, '').trim();
	const rest = tagged.slice(0, index).concat(tagged.slice(index + 1)).map((t) => t.line);
	// Collapse the blank lines that surrounded the heading into one.
	while (index > 0 && rest[index - 1]?.trim() === '' && rest[index]?.trim() === '') rest.splice(index, 1);
	while (rest.length && rest[0].trim() === '') rest.shift();
	return { title: plainText(title), body: rest.join('\n') };
}

/** Markdown inline syntax to plain text (links keep their text, code keeps its content). */
export function plainText(s) {
	return s
		.replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
		.replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
		.replace(/\[([^\]]*)\]\[[^\]]*\]/g, '$1')
		.replace(/<(https?:[^>]+)>/g, '$1')
		.replace(/<[^>]+>/g, '')
		.replace(/`+([^`]*)`+/g, '$1')
		.replace(/(\*\*|__)(.+?)\1/g, '$2')
		.replace(/(^|[^\w*])[*_]([^*_\s][^*_]*?)[*_](?=[^\w*]|$)/g, '$1$2')
		.replace(/\s+/g, ' ')
		.trim();
}

/**
 * First meaningful sentence of the body: the first paragraph that is not a heading, list,
 * table, quote, code block or HTML, as plain text, cut to at most `max` characters at a
 * sentence end (or a word boundary as a last resort).
 */
export function firstSentence(body, max = 160) {
	const paragraphs = [];
	let current = [];
	for (const { line, code } of tagLines(body)) {
		const blank = line.trim() === '';
		const structural = code || /^\s*(#{1,6}\s|[-*+]\s|\d+[.)]\s|>|\||<|!\[|---|\*\*\*)/.test(line);
		if (blank || structural) {
			if (current.length) paragraphs.push(current.join(' '));
			current = [];
			continue;
		}
		current.push(line.trim());
	}
	if (current.length) paragraphs.push(current.join(' '));
	const text = plainText(paragraphs[0] ?? '');
	if (!text) return '';
	// A sentence ends at ".", "!" or "?" before white space and anything but a lower-case
	// letter (so "e.g. the" does not end one), or at the end of the paragraph.
	const ends = [...text.matchAll(/[.!?](?=\s+[^a-z\s]|$)/g)].map((m) => m.index + 1);
	const sentence = ends.length ? text.slice(0, ends[0]) : text;
	if (sentence.length <= max) return sentence;
	const cut = sentence.slice(0, max - 1);
	return cut.slice(0, cut.lastIndexOf(' ')).replace(/[,;:]$/, '') + '…';
}

/**
 * Rewrites Markdown link targets outside code (inline links, images and reference
 * definitions). `resolve(target, isImage)` returns the new target or undefined to keep it.
 */
export function rewriteLinks(markdown, resolve) {
	const rewriteInline = (text) =>
		// Split off inline code spans so their content is never touched.
		text
			.split(/(`+[^`]*`+)/g)
			.map((part, i) =>
				i % 2 === 1
					? part
					: part.replace(/(!?)\[((?:[^\[\]]|\[[^\]]*\])*)\]\(\s*<?([^)\s>]+)>?((?:\s+"[^"]*")?)\s*\)/g, (all, bang, label, target, title) => {
							const next = resolve(target, bang === '!');
							return next === undefined ? all : `${bang}[${label}](${next}${title})`;
						})
			)
			.join('');
	return tagLines(markdown)
		.map(({ line, code }) => {
			if (code) return line;
			const def = /^(\s{0,3}\[[^\]]+\]:\s*)(\S+)(.*)$/.exec(line);
			if (def) {
				const next = resolve(def[2], false);
				return next === undefined ? line : def[1] + next + def[3];
			}
			return rewriteInline(line);
		})
		.join('\n');
}

/**
 * Builds the link resolver for one source file.
 * @param {object} ctx
 * @param {string} ctx.source       repository path of the file being synced
 * @param {string} ctx.repo         repository URL (https://github.com/owner/name)
 * @param {string} ctx.site         canonical site origin
 * @param {Map<string, string>} ctx.routes  repository path -> site route
 * @param {Record<string, string>} ctx.aliases  "path#anchor" -> site route
 * @param {(path: string) => 'file' | 'dir' | undefined} ctx.exists
 * @param {string[]} ctx.errors     collects unresolved links
 */
export function makeResolver({ source, repo, site, routes, aliases, exists, errors }) {
	const blobPrefix = `${repo}/blob/main/`;
	const treePrefix = `${repo}/tree/main/`;
	const toSite = (path, hash) => {
		const key = path + hash;
		if (aliases[key]) return aliases[key];
		const route = routes.get(path);
		if (route) return route + hash;
		return undefined;
	};
	return (target, isImage) => {
		if (target.startsWith('#') || /^(mailto|tel):/i.test(target)) return undefined;
		// Absolute links to this repository's main branch point at the site when published.
		for (const prefix of [blobPrefix, treePrefix]) {
			if (target.startsWith(prefix)) {
				const [path, hash = ''] = splitHash(target.slice(prefix.length));
				return toSite(decodeURIComponent(path), hash) ?? undefined;
			}
		}
		if (target.startsWith(site + '/')) return target.slice(site.length);
		if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('//')) return undefined;
		const [rel, hash = ''] = splitHash(target);
		const path = posix.normalize(posix.join(posix.dirname(source), decodeURIComponent(rel))).replace(/\/$/, '');
		if (path.startsWith('..')) {
			errors.push(`${source}: link outside the repository: ${target}`);
			return undefined;
		}
		if (!isImage) {
			const mapped = toSite(path, hash);
			if (mapped) return mapped;
		}
		const kind = exists(path);
		if (!kind) {
			errors.push(`${source}: broken link ${target} (${path} does not exist)`);
			return undefined;
		}
		if (isImage) return `${repo}/raw/main/${path}`;
		return (kind === 'dir' ? treePrefix : blobPrefix) + encodeURI(path) + hash;
	};
}

function splitHash(s) {
	const i = s.indexOf('#');
	return i < 0 ? [s] : [s.slice(0, i), s.slice(i)];
}

/** YAML frontmatter with JSON-quoted strings (valid YAML), in a fixed key order. */
export function frontmatter(data) {
	const lines = ['---'];
	for (const [key, value] of Object.entries(data)) {
		if (value === undefined) continue;
		if (value && typeof value === 'object') {
			lines.push(`${key}:`);
			for (const [k, v] of Object.entries(value)) if (v !== undefined) lines.push(`  ${k}: ${JSON.stringify(v)}`);
		} else lines.push(`${key}: ${JSON.stringify(value)}`);
	}
	lines.push('---', '');
	return lines.join('\n');
}
