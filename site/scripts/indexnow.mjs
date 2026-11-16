#!/usr/bin/env node
// Tells the IndexNow search engines (Bing, Yandex, Seznam, Naver and others; not Google) which
// pages of the deployed site to crawl now. Run it after a deploy:
//
//   npm run indexnow -- --all            every URL in the live sitemap (first launch, big changes)
//   npm run indexnow -- --since 2d       URLs whose <lastmod> is at most two days old
//   npm run indexnow -- --since 2026-11-16 --dry-run
//
// The key is public by protocol: public/<key>.txt holds the key and proves control of the host.
// The script reads the live sitemap and refuses to run until the live site serves the key file,
// so it can never announce pages that are not deployed.

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseSitemap, parseSince } from './lib/sitemap.mjs';
import { SITE } from '../src/config.ts';

const ENDPOINT = 'https://api.indexnow.org/indexnow';
const site = join(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');
const all = args.includes('--all');
const sinceArg = args.includes('--since') ? args[args.indexOf('--since') + 1] : undefined;
const since = sinceArg === undefined ? undefined : parseSince(sinceArg);

if ((!all && sinceArg === undefined) || (sinceArg !== undefined && since === undefined)) {
	console.error('usage: node scripts/indexnow.mjs (--all | --since <ISO date | Nh | Nd>) [--dry-run]');
	process.exit(2);
}

const keyFile = readdirSync(join(site, 'public')).find((f) => /^[a-f0-9]{32}\.txt$/.test(f));
const key = keyFile?.slice(0, -'.txt'.length);
if (!key || readFileSync(join(site, 'public', keyFile), 'utf8').trim() !== key) {
	console.error('indexnow: public/<key>.txt is missing or does not contain its own name');
	process.exit(1);
}

const get = async (url) => {
	const res = await fetch(url, { redirect: 'follow' });
	if (!res.ok) throw new Error(`${url} returned HTTP ${res.status}`);
	return res.text();
};

try {
	const keyLocation = `${SITE}/${key}.txt`;
	if ((await get(keyLocation)).trim() !== key) throw new Error(`${keyLocation} does not serve the key: deploy first`);

	const entries = [];
	for (const { loc } of parseSitemap(await get(`${SITE}/sitemap-index.xml`))) entries.push(...parseSitemap(await get(loc)));
	const urls = entries
		.filter((e) => e.loc.startsWith(SITE + '/'))
		.filter((e) => all || (e.lastmod !== undefined && Date.parse(e.lastmod) >= since))
		.map((e) => e.loc);
	if (urls.length === 0) {
		console.log('indexnow: no URL to submit');
		process.exit(0);
	}
	console.log(`indexnow: ${urls.length} of ${entries.length} sitemap URLs${dryRun ? ' (dry run, nothing sent)' : ''}`);
	for (const u of urls) console.log('  ' + u);
	if (!dryRun) {
		const res = await fetch(ENDPOINT, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json; charset=utf-8' },
			body: JSON.stringify({ host: new URL(SITE).host, key, keyLocation, urlList: urls }),
		});
		// 200: accepted. 202: accepted, key validation pending.
		if (res.status !== 200 && res.status !== 202) throw new Error(`${ENDPOINT} returned HTTP ${res.status}: ${(await res.text()).slice(0, 200)}`);
		console.log(`indexnow: accepted (HTTP ${res.status})`);
	}
} catch (e) {
	console.error('indexnow: ' + e.message);
	process.exit(1);
}
