// Shape check for the cadkit-stats summary (schema 1), mirroring the required part of
// https://github.com/TNYCL/cadkit-stats/blob/main/schema/summary.schema.json.
// Fields not checked here may be added by the collector (additive changes keep schema 1).
// Pure: used by scripts/prebuild.d/30-stats.mjs and the tests.

const TIMESTAMP = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

const isObj = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);
const isCount = (v) => Number.isSafeInteger(v) && v >= 0;

/**
 * Validates a parsed summary.json.
 * @param {unknown} s
 * @returns {string[]} problems; empty when the summary can be rendered
 */
export function validateSummary(s) {
	const errors = [];
	const bad = (path, what) => errors.push(`${path}: ${what}`);
	const obj = (v, path) => {
		if (!isObj(v)) {
			bad(path, 'expected an object');
			return false;
		}
		return true;
	};
	const counts = (o, path, keys) => {
		for (const k of keys) if (!isCount(o[k])) bad(`${path}.${k}`, 'expected a non-negative integer');
	};
	const str = (v, path) => {
		if (typeof v !== 'string' || v.length === 0) bad(path, 'expected a non-empty string');
	};
	const series = (v, path, width) => {
		if (!Array.isArray(v)) return bad(path, 'expected an array');
		let prev = '';
		v.forEach((row, i) => {
			if (!Array.isArray(row) || row.length !== width) return bad(`${path}[${i}]`, `expected [date${', count'.repeat(width - 1)}]`);
			const [date, ...values] = row;
			if (typeof date !== 'string' || !DATE.test(date)) bad(`${path}[${i}][0]`, 'expected YYYY-MM-DD');
			else if (date <= prev) bad(`${path}[${i}][0]`, 'dates must be strictly ascending');
			else prev = date;
			values.forEach((x, j) => {
				if (!isCount(x)) bad(`${path}[${i}][${j + 1}]`, 'expected a non-negative integer');
			});
		});
	};

	if (!obj(s, 'summary')) return errors;
	if (s.schema !== 1) bad('schema', `expected 1, got ${JSON.stringify(s.schema)}`);
	if (typeof s.generated_at !== 'string' || !TIMESTAMP.test(s.generated_at)) bad('generated_at', 'expected an ISO-8601 UTC timestamp');

	if (obj(s.totals, 'totals'))
		counts(s.totals, 'totals', ['crates_io', 'pypi_without_mirrors', 'pypi_with_mirrors', 'npm', 'github_release_assets']);

	if (!Array.isArray(s.crates)) bad('crates', 'expected an array');
	else
		s.crates.forEach((c, i) => {
			if (!obj(c, `crates[${i}]`)) return;
			str(c.name, `crates[${i}].name`);
			str(c.max_version, `crates[${i}].max_version`);
			counts(c, `crates[${i}]`, ['downloads', 'recent_90d']);
		});

	if (obj(s.pypi, 'pypi')) {
		str(s.pypi.package, 'pypi.package');
		counts(s.pypi, 'pypi', ['last_day', 'last_week', 'last_month']);
		if (obj(s.pypi.os_without_mirrors, 'pypi.os_without_mirrors'))
			counts(s.pypi.os_without_mirrors, 'pypi.os_without_mirrors', ['Linux', 'Darwin', 'Windows', 'other', 'null']);
	}

	if (obj(s.npm, 'npm')) {
		str(s.npm.package, 'npm.package');
		counts(s.npm, 'npm', ['last_week', 'last_month']);
	}

	if (obj(s.github, 'github')) {
		const g = s.github;
		counts(g, 'github', ['stars', 'forks', 'open_issues', 'watchers']);
		if (!Array.isArray(g.release_assets)) bad('github.release_assets', 'expected an array');
		else
			g.release_assets.forEach((a, i) => {
				if (!obj(a, `github.release_assets[${i}]`)) return;
				if (typeof a.tag !== 'string') bad(`github.release_assets[${i}].tag`, 'expected a string');
				if (typeof a.asset !== 'string') bad(`github.release_assets[${i}].asset`, 'expected a string');
				counts(a, `github.release_assets[${i}]`, ['downloads']);
			});
		if (!('traffic_14d' in g)) bad('github.traffic_14d', 'missing (null is allowed)');
		else if (g.traffic_14d !== null && obj(g.traffic_14d, 'github.traffic_14d'))
			counts(g.traffic_14d, 'github.traffic_14d', ['views', 'unique_views', 'clones', 'unique_clones']);
	}

	if (obj(s.series, 'series')) {
		for (const k of ['crates_io', 'pypi_without_mirrors', 'npm']) series(s.series[k], `series.${k}`, 2);
		for (const k of ['github_views', 'github_clones']) series(s.series[k], `series.${k}`, 3);
	}

	if (!Array.isArray(s.notes) || !s.notes.every((n) => typeof n === 'string')) bad('notes', 'expected an array of strings');

	// Optional (additive): per-source status.
	if (s.sources !== undefined && obj(s.sources, 'sources'))
		for (const [k, v] of Object.entries(s.sources)) {
			if (!obj(v, `sources.${k}`)) continue;
			if (typeof v.ok !== 'boolean') bad(`sources.${k}.ok`, 'expected a boolean');
			if (v.last_success_at != null && (typeof v.last_success_at !== 'string' || !TIMESTAMP.test(v.last_success_at)))
				bad(`sources.${k}.last_success_at`, 'expected an ISO-8601 UTC timestamp or null');
		}

	return errors;
}
