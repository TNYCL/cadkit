// Small DOM helpers of the demo page.

export const SVG_NS = 'http://www.w3.org/2000/svg';

export const byId = <T extends HTMLElement = HTMLElement>(id: string): T => {
	const el = document.getElementById(id);
	if (!el) throw new Error('demo: missing #' + id);
	return el as T;
};

/** Creates an HTML element with attributes ("class" sets className) and children. */
export function h<K extends keyof HTMLElementTagNameMap>(tag: K, props: Record<string, string> = {}, ...children: (Node | string)[]): HTMLElementTagNameMap[K] {
	const el = document.createElement(tag);
	for (const [k, v] of Object.entries(props)) {
		if (k === 'class') el.className = v;
		else el.setAttribute(k, v);
	}
	el.append(...children);
	return el;
}

/** Creates an SVG element with attributes. */
export function s<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number> = {}): SVGElementTagNameMap[K] {
	const el = document.createElementNS(SVG_NS, tag);
	for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
	return el;
}

/** Fills a definition list with label / value rows. */
export function fillDl(dl: HTMLElement, rows: [string, string | Node][]): void {
	dl.replaceChildren(...rows.map(([k, v]) => h('div', {}, h('dt', {}, k), h('dd', {}, v))));
}

export const mono = (text: string) => h('code', {}, text);

/** Saves data through a temporary blob: link (the browser's own download; nothing is uploaded). */
export function download(name: string, data: BlobPart, mime: string): void {
	const url = URL.createObjectURL(new Blob([data], { type: mime }));
	const a = h('a', { href: url, download: name });
	a.hidden = true;
	document.body.append(a);
	a.click();
	a.remove();
	setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
