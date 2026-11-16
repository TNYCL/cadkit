// Minimal reader for the sitemaps this site generates (scripts/verify.mjs, scripts/indexnow.mjs).

/**
 * @param {string} xml  A <urlset> or <sitemapindex> document.
 * @returns {{loc: string, lastmod?: string}[]} Entries in document order.
 */
export function parseSitemap(xml) {
	const decode = (s) => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, '&');
	const out = [];
	for (const m of xml.matchAll(/<(url|sitemap)>([\s\S]*?)<\/\1>/g)) {
		const loc = /<loc>([^<]+)<\/loc>/.exec(m[2])?.[1];
		if (!loc) continue;
		const lastmod = /<lastmod>([^<]+)<\/lastmod>/.exec(m[2])?.[1];
		out.push(lastmod ? { loc: decode(loc.trim()), lastmod: lastmod.trim() } : { loc: decode(loc.trim()) });
	}
	return out;
}

/**
 * Cut-off for "changed since": an ISO date or datetime, or an age such as "36h" or "7d".
 * @param {string} value @param {number} [now] Milliseconds since the epoch.
 * @returns {number | undefined} Milliseconds since the epoch; undefined when unparsable.
 */
export function parseSince(value, now = Date.now()) {
	const age = /^(\d+)([hd])$/.exec(value);
	if (age) return now - Number(age[1]) * (age[2] === 'h' ? 36e5 : 864e5);
	const time = Date.parse(value);
	return Number.isFinite(time) ? time : undefined;
}
