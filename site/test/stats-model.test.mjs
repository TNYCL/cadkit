// Summary validation and the /stats/ view model, on a synthetic summary (no network).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateSummary } from '../src/stats/schema.mjs';
import { buildModel, WINDOW_DAYS } from '../src/stats/model.mjs';
import { dateRange } from '../src/stats/chart.mjs';

/** A small, valid schema-1 summary with made-up numbers. */
function fixture() {
	const days = dateRange('2026-10-01', '2026-11-16');
	return {
		schema: 1,
		generated_at: '2026-11-16T03:17:00Z',
		totals: { crates_io: 300, pypi_without_mirrors: 90, pypi_with_mirrors: 200, npm: 47, github_release_assets: 6 },
		crates: [
			{ name: 'cadkit', downloads: 100, recent_90d: 100, max_version: '0.5.3' },
			{ name: 'cadkit-core', downloads: 200, recent_90d: 150, max_version: '0.5.3' },
		],
		pypi: { package: 'pycadkit', last_day: 1, last_week: 7, last_month: 30, os_without_mirrors: { Linux: 6, Darwin: 2, Windows: 2, other: 0, null: 80 } },
		npm: { package: 'cadkit-wasm', last_week: 7, last_month: 30 },
		github: {
			stars: 1,
			forks: 0,
			open_issues: 0,
			watchers: 0,
			release_assets: [
				{ tag: 'v0.5.3', asset: 'cadkit-x86_64-unknown-linux-musl.tar.gz', downloads: 4 },
				{ tag: 'v0.5.3', asset: 'SHA256SUMS', downloads: 1 },
				{ tag: 'v0.5.2', asset: 'cadkit-capi-x86_64-apple-darwin.tar.gz', downloads: 1 },
			],
			traffic_14d: null,
			popular_referrers_14d: null,
		},
		series: {
			crates_io: days.map((d) => [d, 6]),
			pypi_without_mirrors: days.slice(0, -1).map((d) => [d, 2]),
			npm: days.map((d) => [d, 1]),
			github_views: [],
			github_clones: [],
		},
		sources: {
			crates_io: { ok: true, fetched_at: '2026-11-16T03:17:00Z', error: null, last_success_at: '2026-11-16T03:17:00Z' },
			github_traffic: { ok: false, fetched_at: '2026-11-16T03:17:00Z', error: 'skipped', skipped: true, last_success_at: null },
		},
		notes: ['Download counts include CI systems.'],
		future_field: { anything: true },
	};
}

test('a valid summary passes, additive fields are ignored', () => {
	assert.deepEqual(validateSummary(fixture()), []);
});

test('validation reports wrong shapes instead of throwing', () => {
	assert.ok(validateSummary(null).length > 0);
	assert.ok(validateSummary([]).length > 0);
	const s = fixture();
	s.schema = 2;
	s.totals.npm = -1;
	s.crates[0].downloads = 1.5;
	s.series.npm = [['2026-11-02', 1], ['2026-11-01', 1]];
	s.series.github_views = [['2026-11-01', 1]];
	s.github.traffic_14d = { views: 1 };
	delete s.pypi.os_without_mirrors.null;
	s.generated_at = 'yesterday';
	const errors = validateSummary(s).join('\n');
	for (const part of ['schema', 'totals.npm', 'crates[0].downloads', 'series.npm[1][0]', 'series.github_views[0]', 'traffic_14d.unique_views', 'os_without_mirrors.null', 'generated_at'])
		assert.ok(errors.includes(part), `expected a problem about ${part}:\n${errors}`);
	const missing = fixture();
	delete missing.github.traffic_14d;
	assert.match(validateSummary(missing).join('\n'), /traffic_14d/);
});

test('model: daily panels share dates and axis; missing days are null', () => {
	const m = buildModel(fixture(), new Date('2026-11-16T12:00:00Z'));
	assert.equal(m.stale, false);
	assert.equal(m.daily.start, '2026-10-01');
	assert.equal(m.daily.end, '2026-11-16');
	assert.equal(m.daily.dates.length, 47);
	assert.ok(m.daily.dates.length <= WINDOW_DAYS);
	const pypi = m.daily.panels.find((p) => p.key === 'pypi');
	assert.equal(pypi.values.at(-1), null, 'PyPI has not reported the last day');
	assert.equal(m.daily.axis.max >= 6, true);
	// Takeaway uses the last 30 days every channel has: 2026-10-17 to 2026-11-15.
	assert.match(m.daily.takeaway, /30 days to Nov 15, 2026: crates\.io 180, PyPI 60, npm 30 downloads/);
});

test('model: crates ranked, OS shares, release kinds, staleness and sources', () => {
	const m = buildModel(fixture(), new Date('2026-11-20T12:00:00Z'));
	assert.deepEqual(m.crates.rows.map((c) => c.name), ['cadkit-core', 'cadkit']);
	assert.match(m.crates.takeaway, /^cadkit-core leads with 200 downloads \(67%\)/);
	assert.deepEqual(m.pypiOs.reported.map((r) => r.label), ['Linux', 'macOS', 'Windows']);
	assert.equal(m.pypiOs.notReported, 80);
	assert.match(m.pypiOs.takeaway, /^89% of downloads did not report an operating system; of those that did, Linux accounts for 60%\./);
	assert.deepEqual(m.releases.kinds, [
		{ label: 'CLI archives', value: 4 },
		{ label: 'C API archives', value: 1 },
		{ label: 'Checksums', value: 1 },
	]);
	assert.equal(m.releases.count, 2);
	assert.equal(m.stale, true, 'four days old');
	assert.equal(m.ageDays, 4);
	assert.equal(m.traffic.window, null);
	assert.equal(m.traffic.views, null);
	const t = m.sourceFor('github_traffic');
	assert.equal(t.ok, false);
	assert.equal(t.skipped, true);
	assert.equal(t.label, 'GitHub traffic');
});

test('model: a summary without the optional sources field still renders', () => {
	const s = fixture();
	delete s.sources;
	const m = buildModel(s, new Date('2026-11-16T12:00:00Z'));
	assert.deepEqual(m.sources, []);
	assert.equal(m.traffic.source, null);
});
