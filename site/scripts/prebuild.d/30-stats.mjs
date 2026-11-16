#!/usr/bin/env node
// Fetches the public cadkit-stats summary for /stats/ and writes src/stats/generated/stats.json:
//
//   {"status": "ok", "checked_at", "source", "summary"}            validated summary (schema 1)
//   {"status": "unavailable", "checked_at", "source", "reason"}    the page says so honestly
//
// The build never fails because the stats repository is unreachable or publishes a bad file:
// /stats/ then shows "temporarily unavailable" and no numbers. It does fail on bugs here.
//
// Environment:
//   STATS_SUMMARY_FILE=path   read a local summary instead of fetching (offline work, QA)
//   STATS_OFFLINE=1           skip the network and render the unavailable state
//   STATS_REFRESH=1           ignore the 10-minute cache of a previous successful fetch

import { readFileSync, writeFileSync, mkdirSync, existsSync, statSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateSummary } from '../../src/stats/schema.mjs';
import { STATS_SUMMARY_URL } from '../../src/config.ts';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const outDir = join(root, 'src', 'stats', 'generated');
const outFile = join(outDir, 'stats.json');
const TIMEOUT_MS = 10_000;
const MAX_BYTES = 5 * 1024 * 1024;
const CACHE_MS = 10 * 60 * 1000;
const env = process.env;

mkdirSync(outDir, { recursive: true });
const ignore = join(outDir, '.gitignore');
if (!existsSync(ignore)) writeFileSync(ignore, '*\n!.gitignore\n');

const now = new Date();
const write = (record) => {
	writeFileSync(outFile, JSON.stringify({ checked_at: now.toISOString().replace(/\.\d+Z$/, 'Z'), ...record }) + '\n');
};

/** Reuse a recent successful fetch of the same URL (repeated dev and check runs). */
function cached() {
	if (env.STATS_REFRESH || env.STATS_SUMMARY_FILE || env.STATS_OFFLINE || !existsSync(outFile)) return false;
	try {
		const prev = JSON.parse(readFileSync(outFile, 'utf8'));
		return prev.status === 'ok' && prev.source === STATS_SUMMARY_URL && now.getTime() - statSync(outFile).mtimeMs < CACHE_MS;
	} catch {
		return false;
	}
}

async function fetchOnce() {
	const res = await fetch(STATS_SUMMARY_URL, {
		signal: AbortSignal.timeout(TIMEOUT_MS),
		headers: { accept: 'application/json', 'user-agent': 'cadkit-site-build (https://cadkit.org)' },
	});
	if (!res.ok) {
		const err = new Error(`HTTP ${res.status}`);
		err.retry = res.status >= 500 || res.status === 429;
		throw err;
	}
	const text = await res.text();
	if (text.length > MAX_BYTES) throw Object.assign(new Error('response larger than 5 MiB'), { retry: false });
	return text;
}

async function fetchSummary() {
	try {
		return await fetchOnce();
	} catch (e) {
		if (e.retry === false) throw e;
		console.warn(`stats: ${describe(e)}; retrying once`);
		await new Promise((r) => setTimeout(r, 1500));
		return fetchOnce();
	}
}

const describe = (e) => (e?.name === 'TimeoutError' ? `timed out after ${TIMEOUT_MS / 1000} s` : String(e?.message ?? e));

async function main() {
	if (cached()) {
		console.log('stats: using the summary fetched less than 10 minutes ago');
		return;
	}
	if (env.STATS_OFFLINE) {
		write({ status: 'unavailable', source: STATS_SUMMARY_URL, reason: 'statistics were not fetched for this build' });
		console.log('stats: STATS_OFFLINE set; /stats/ renders the unavailable state');
		return;
	}
	const source = env.STATS_SUMMARY_FILE ? resolve(env.STATS_SUMMARY_FILE) : STATS_SUMMARY_URL;
	let text;
	try {
		text = env.STATS_SUMMARY_FILE ? readFileSync(source, 'utf8') : await fetchSummary();
	} catch (e) {
		write({ status: 'unavailable', source: STATS_SUMMARY_URL, reason: 'the statistics summary could not be downloaded' });
		console.warn(`stats: WARNING: ${describe(e)}; /stats/ renders the unavailable state`);
		return;
	}
	let summary;
	try {
		summary = JSON.parse(text);
	} catch {
		write({ status: 'unavailable', source: STATS_SUMMARY_URL, reason: 'the statistics summary is not valid JSON' });
		console.warn('stats: WARNING: summary is not valid JSON; /stats/ renders the unavailable state');
		return;
	}
	const errors = validateSummary(summary);
	if (errors.length) {
		write({ status: 'unavailable', source: STATS_SUMMARY_URL, reason: 'the statistics summary did not match the expected format' });
		console.warn(`stats: WARNING: summary failed validation (${errors.length} problem(s)); first: ${errors.slice(0, 3).join('; ')}`);
		return;
	}
	write({ status: 'ok', source: env.STATS_SUMMARY_FILE ? 'local file' : STATS_SUMMARY_URL, summary });
	console.log(`stats: summary generated ${summary.generated_at} (${env.STATS_SUMMARY_FILE ? 'local file' : 'fetched'})`);
}

await main();
