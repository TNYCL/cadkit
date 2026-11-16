// Turns the generated stats file into everything /stats/ renders: tiles, chart geometry inputs,
// tables and one-line takeaways. Pure (no I/O), so the tests can feed it fixtures.

import {
	sum,
	formatCount,
	formatPercent,
	niceAxis,
	addDays,
	dateRange,
	formatDate,
	formatDateSpan,
	densify,
	trailingMean,
	assetKind,
} from './chart.mjs';

/** Days shown in the daily charts (the raw CSVs in the stats repository keep everything). */
export const WINDOW_DAYS = 90;
/** A summary older than this at build time is flagged as stale. */
export const STALE_AFTER_HOURS = 72;

export const SOURCE_LABELS = {
	crates_io: 'crates.io',
	pypi: 'PyPI',
	npm: 'npm',
	github: 'GitHub repository and releases',
	github_traffic: 'GitHub traffic',
};

const OS_LABELS = [
	['Linux', 'Linux'],
	['Darwin', 'macOS'],
	['Windows', 'Windows'],
	['other', 'Other'],
];

/**
 * @param {any} summary validated summary (schema 1)
 * @param {Date} buildTime
 */
export function buildModel(summary, buildTime) {
	const s = summary;
	const ageHours = (buildTime.getTime() - Date.parse(s.generated_at)) / 36e5;

	// Source status (optional field in schema 1).
	const sources = Object.entries(s.sources ?? {}).map(([key, v]) => ({
		key,
		label: SOURCE_LABELS[key] ?? key,
		ok: v.ok === true,
		skipped: v.skipped === true,
		lastSuccessAt: v.last_success_at ?? null,
	}));
	const sourceFor = (key) => sources.find((x) => x.key === key);

	// Daily downloads: one shared date range and y axis across the three channels.
	const channels = [
		{ key: 'crates_io', source: 'crates_io', label: 'crates.io', detail: 'all cadkit crates', rows: s.series.crates_io },
		{ key: 'pypi', source: 'pypi', label: 'PyPI', detail: 'pycadkit, without mirrors', rows: s.series.pypi_without_mirrors },
		{ key: 'npm', source: 'npm', label: 'npm', detail: 'cadkit-wasm', rows: s.series.npm },
	];
	const lastDates = channels.map((c) => c.rows.at(-1)?.[0]).filter(Boolean);
	const firstDates = channels.map((c) => c.rows[0]?.[0]).filter(Boolean);
	let daily = null;
	if (lastDates.length) {
		const end = lastDates.reduce((a, b) => (a > b ? a : b));
		const earliest = firstDates.reduce((a, b) => (a < b ? a : b));
		const windowStart = addDays(end, -(WINDOW_DAYS - 1));
		const start = earliest > windowStart ? earliest : windowStart;
		const dates = dateRange(start, end);
		const panels = channels.map((c) => {
			const values = densify(c.rows, dates);
			return { ...c, values, mean7: trailingMean(values, 7), firstDate: c.rows[0]?.[0] ?? null };
		});
		const axis = niceAxis(Math.max(1, ...panels.flatMap((p) => p.values.filter((v) => v !== null))));
		// Takeaway over the last 30 days that every channel has data for.
		const commonEnd = lastDates.reduce((a, b) => (a < b ? a : b));
		const tStart = addDays(commonEnd, -29);
		const last30 = panels.map((p) => ({
			label: p.label,
			total: sum(p.values.filter((_, i) => dates[i] >= tStart && dates[i] <= commonEnd)),
		}));
		const full30 = tStart >= start;
		daily = {
			dates,
			start,
			end,
			span: formatDateSpan(start, end),
			axis,
			panels,
			takeaway: full30
				? `In the 30 days to ${formatDate(commonEnd, { year: true })}: ${last30
						.map((x) => `${x.label} ${formatCount(x.total)}`)
						.join(', ')} downloads.`
				: `Daily data starts ${formatDate(start, { year: true })}; ${last30
						.map((x) => `${x.label} ${formatCount(x.total)}`)
						.join(', ')} downloads up to ${formatDate(commonEnd, { year: true })}.`,
		};
	}

	// Per-crate downloads, largest first.
	const crates = [...s.crates].sort((a, b) => b.downloads - a.downloads || a.name.localeCompare(b.name));
	const cratesTotal = sum(crates.map((c) => c.downloads));
	const top = crates[0];
	const cratesTakeaway = !top
		? 'No crate data.'
		: top.name === 'cadkit-core'
			? `cadkit-core leads with ${formatCount(top.downloads)} downloads (${formatPercent(top.downloads, cratesTotal)}): every format crate depends on it, so each of their installs downloads it too.`
			: `${top.name} leads with ${formatCount(top.downloads)} downloads (${formatPercent(top.downloads, cratesTotal)} of the crates.io total).`;

	// PyPI operating systems (without mirrors). "Not reported" is shown apart from the ranking.
	const os = s.pypi.os_without_mirrors;
	const osTotal = sum([...OS_LABELS.map(([k]) => os[k]), os.null]);
	const reported = OS_LABELS.map(([k, label]) => ({ label, value: os[k] }))
		.filter((r) => r.value > 0 || r.label !== 'Other')
		.sort((a, b) => b.value - a.value);
	const reportedTotal = sum(reported.map((r) => r.value));
	const osTop = reported[0];
	const pypiOs = {
		total: osTotal,
		reported,
		notReported: os.null,
		takeaway:
			osTotal === 0
				? 'No PyPI downloads recorded yet.'
				: `${formatPercent(os.null, osTotal)} of downloads did not report an operating system${
						osTop && reportedTotal > 0
							? `; of those that did, ${osTop.label} accounts for ${formatPercent(osTop.value, reportedTotal)}.`
							: '.'
					}`,
	};

	// GitHub release assets by artefact kind and by release.
	const assets = s.github.release_assets;
	const kindMap = new Map();
	for (const a of assets) kindMap.set(assetKind(a.asset), (kindMap.get(assetKind(a.asset)) ?? 0) + a.downloads);
	const kinds = [...kindMap].map(([label, value]) => ({ label, value })).sort((a, b) => b.value - a.value || a.label.localeCompare(b.label));
	const tagMap = new Map();
	for (const a of assets) tagMap.set(a.tag, (tagMap.get(a.tag) ?? 0) + a.downloads);
	const tags = [...tagMap].map(([tag, value]) => ({ tag, value }));
	const assetsTotal = sum(assets.map((a) => a.downloads));
	const releases = {
		total: assetsTotal,
		count: tags.length,
		kinds,
		tags,
		takeaway:
			assetsTotal === 0
				? 'No release asset downloads recorded yet.'
				: `${formatCount(assetsTotal)} downloads across ${tags.length} release${tags.length === 1 ? '' : 's'}; ${kinds[0].label} account for ${formatPercent(kinds[0].value, assetsTotal)}.`,
	};

	// GitHub traffic (14-day window, may be missing).
	const t = s.github.traffic_14d;
	const trafficSeries = (rows) => {
		if (!rows.length) return null;
		const end = rows.at(-1)[0];
		const windowStart = addDays(end, -(WINDOW_DAYS - 1));
		const start = rows[0][0] > windowStart ? rows[0][0] : windowStart;
		const dates = dateRange(start, end);
		return { dates, count: densify(rows, dates, 1), uniques: densify(rows, dates, 2), span: formatDateSpan(start, end), days: rows.filter((r) => r[0] >= start).length };
	};
	const views = trafficSeries(s.series.github_views);
	const clones = trafficSeries(s.series.github_clones);
	const trafficAxisMax = (x) => (x ? niceAxis(Math.max(1, ...x.count.filter((v) => v !== null))) : null);
	const traffic = {
		window: t,
		source: sourceFor('github_traffic') ?? null,
		views: views && { ...views, axis: trafficAxisMax(views) },
		clones: clones && { ...clones, axis: trafficAxisMax(clones) },
		takeaway: t
			? `${formatCount(t.views)} page views from ${formatCount(t.unique_views)} visitors and ${formatCount(t.clones)} clones from ${formatCount(t.unique_clones)} cloners in GitHub's 14-day window.`
			: null,
	};

	const firstOf = (rows) => rows[0]?.[0] ?? null;

	return {
		generatedAt: s.generated_at,
		ageHours,
		stale: ageHours > STALE_AFTER_HOURS,
		ageDays: Math.floor(ageHours / 24),
		sources,
		sourceFor,
		totals: {
			crates: s.totals.crates_io,
			cratesCount: s.crates.length,
			pypi: s.totals.pypi_without_mirrors,
			pypiWithMirrors: s.totals.pypi_with_mirrors,
			pypiSince: firstOf(s.series.pypi_without_mirrors),
			npm: s.totals.npm,
			npmSince: firstOf(s.series.npm),
			releaseAssets: s.totals.github_release_assets,
		},
		recent: {
			pypi: { lastDay: s.pypi.last_day, lastWeek: s.pypi.last_week, lastMonth: s.pypi.last_month },
			npm: { lastWeek: s.npm.last_week, lastMonth: s.npm.last_month },
		},
		daily,
		crates: { rows: crates, total: cratesTotal, takeaway: cratesTakeaway },
		pypiOs,
		releases,
		traffic,
		repo: {
			name: s.github.repo ?? null,
			stars: s.github.stars,
			forks: s.github.forks,
			openIssues: s.github.open_issues,
			watchers: s.github.watchers,
		},
		notes: s.notes,
	};
}
