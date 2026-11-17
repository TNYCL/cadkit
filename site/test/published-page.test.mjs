import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { fetchPublic, inspectPublishedPage } from '../scripts/lib/published-page.mjs';

test('live page audit detects CDN transforms and broken discovery/skip links', () => {
	const html = '<head><title>A · cadkit</title><meta name="description" content="A description">' +
		'<link rel="canonical" href="https://cadkit.org/"><link rel="describedby" href="/llms.txt">' +
		'<link rel="alternate" type="text/markdown" href="/index.md"></head>' +
		'<a class="sl-skip-link" href="#_top">Skip to content</a><main id="_top"><h1>A</h1></main>';
	assert.deepEqual(inspectPublishedPage(html, 'https://cadkit.org/').issues, []);
	const changed = html.replace('id="_top"', 'id="gone"') + '<span class="__cf_email__">[email protected]</span>';
	assert.deepEqual(inspectPublishedPage(changed, 'https://cadkit.org/').issues, [
		'skip link has no content target', 'Cloudflare has obfuscated public contact addresses',
	]);
	assert.ok(inspectPublishedPage(html.replace('/llms.txt', '/missing.txt'), 'https://cadkit.org/').issues.includes('missing llms.txt discovery link'));
});

test('public fetch reports bot blocking and response limits without requiring a real CDN', async (t) => {
	const server = createServer((req, res) => {
		if (req.headers['user-agent'] === 'Python-urllib/3.12') res.writeHead(403).end('error code: 1010');
		else if (req.url === '/challenge') res.writeHead(200, { 'cf-mitigated': 'challenge' }).end('Challenge');
		else res.writeHead(200, { 'content-type': 'text/plain' }).end('Public content');
	});
	t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve); }));
	server.listen(0, '127.0.0.1');
	await once(server, 'listening');
	const url = `http://127.0.0.1:${server.address().port}`;
	assert.equal((await fetchPublic(url, 'cadkit-site-audit/1.0')).text, 'Public content');
	await assert.rejects(fetchPublic(url, 'Python-urllib/3.12'), /HTTP 403/);
	await assert.rejects(fetchPublic(url + '/challenge', 'cadkit-site-audit/1.0'), /CDN challenge/);
	await assert.rejects(fetchPublic(url, 'cadkit-site-audit/1.0', 3), /exceeds 3 bytes/);
});
