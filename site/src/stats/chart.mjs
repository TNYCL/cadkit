// Pure scale, formatting and geometry helpers for the build-time charts on /stats/.
// No DOM, no I/O: every function is unit-tested in test/stats-chart.test.mjs.

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/**
 * Sum of numbers (nulls ignored).
 * @param {readonly (number | null | undefined)[]} xs
 * @returns {number}
 */
export function sum(xs) {
	let total = 0;
	for (const x of xs) if (typeof x === 'number') total += x;
	return total;
}

/** "12,345" (en-US grouping, integers). @param {number} n */
export const formatCount = (n) => Math.round(n).toLocaleString('en-US');

/**
 * Axis tick label: in full below 10,000, compact (10K, 1.5M) from there, as the axis is read
 * at a glance. Pass the axis maximum so one axis uses one style.
 * @param {number} n @param {number} axisMax
 */
export function formatTick(n, axisMax) {
	if (axisMax < 10_000) return formatCount(n);
	if (axisMax < 1_000_000) return `${trim(n / 1000)}K`;
	return `${trim(n / 1_000_000)}M`;
}
const trim = (v) => String(Math.round(v * 10) / 10);

/**
 * Share as a percentage string with one decimal below 10 % and none above ("3.4%", "81%").
 * @param {number} part @param {number} whole
 */
export function formatPercent(part, whole) {
	if (!whole) return '0%';
	const p = (part / whole) * 100;
	if (p > 0 && p < 0.1) return '<0.1%';
	return p < 10 ? `${Math.round(p * 10) / 10}%` : `${Math.round(p)}%`;
}

/**
 * A "nice" axis: a maximum at or above `max` and evenly spaced ticks from 0 on 1/2/2.5/5 x 10^n
 * steps. Always returns at least [0, step].
 * @param {number} max largest value on the axis
 * @param {number} [count] about how many intervals
 * @returns {{max: number, step: number, ticks: number[]}}
 */
export function niceAxis(max, count = 4) {
	const m = Number.isFinite(max) && max > 0 ? max : 1;
	const mag = 10 ** Math.floor(Math.log10(m / count));
	// Whole-number steps only (the data are counts).
	const steps = [mag / 10, mag, mag * 10]
		.flatMap((g) => [1, 2, 2.5, 5].map((f) => f * g))
		.filter((s) => s >= 1 && Number.isInteger(s));
	const intervals = (s) => Math.ceil(m / s - 1e-9);
	const fits = steps.filter((s) => intervals(s) >= 3 && intervals(s) <= count + 2);
	// Tiny maxima (fewer than three whole-number intervals): one gridline per unit.
	const step = !fits.length ? 1 : fits.reduce((best, s) => {
		// Least headroom above the data; on a tie, fewer gridlines.
		const top = intervals(s) * s;
		const bestTop = intervals(best) * best;
		return top < bestTop || (top === bestTop && s > best) ? s : best;
	}, fits[0]);
	const top = Math.max(step, intervals(step) * step);
	const ticks = [];
	for (let v = 0; v <= top + step / 2; v += step) ticks.push(Math.round(v * 1e6) / 1e6);
	return { max: top, step, ticks };
}

/** @param {string} date YYYY-MM-DD @param {number} days */
export function addDays(date, days) {
	const t = Date.parse(`${date}T00:00:00Z`) + days * 864e5;
	return new Date(t).toISOString().slice(0, 10);
}

/** Whole days from a to b (both YYYY-MM-DD). */
export const daysBetween = (a, b) => Math.round((Date.parse(`${b}T00:00:00Z`) - Date.parse(`${a}T00:00:00Z`)) / 864e5);

/** Every date from start to end inclusive. */
export function dateRange(start, end) {
	const out = [];
	for (let d = start; d <= end; d = addDays(d, 1)) out.push(d);
	return out;
}

/**
 * "Oct 1" or "Oct 1, 2026".
 * @param {string} date YYYY-MM-DD @param {{year?: boolean}} [opts]
 */
export function formatDate(date, { year = false } = {}) {
	const [y, m, d] = date.split('-').map(Number);
	return `${MONTHS[m - 1]} ${d}${year ? `, ${y}` : ''}`;
}

/** "Oct 1 – Nov 16, 2026" (or with both years when they differ). */
export function formatDateSpan(start, end) {
	const sameYear = start.slice(0, 4) === end.slice(0, 4);
	return `${formatDate(start, { year: !sameYear })} – ${formatDate(end, { year: true })}`;
}

/** "2026-11-16 15:44 UTC" from an ISO-8601 UTC timestamp. */
export const formatTimestamp = (iso) => `${iso.slice(0, 10)} ${iso.slice(11, 16)} UTC`;

/**
 * Values of a sparse daily series on a dense list of dates; days the series does not contain
 * are null (not collected), never 0.
 * @param {[string, ...number[]][]} rows ascending [date, value, ...]
 * @param {string[]} dates
 * @param {number} [col] column of the value (1 = first value)
 * @returns {(number | null)[]}
 */
export function densify(rows, dates, col = 1) {
	const byDate = new Map(rows.map((r) => [r[0], r[col]]));
	return dates.map((d) => (byDate.has(d) ? /** @type {number} */ (byDate.get(d)) : null));
}

/**
 * Trailing mean over `k` days; null until a full window of collected days exists, and wherever
 * the window contains a day that was not collected.
 * @param {(number | null)[]} values @param {number} [k]
 * @returns {(number | null)[]}
 */
export function trailingMean(values, k = 7) {
	return values.map((_, i) => {
		if (i < k - 1) return null;
		const win = values.slice(i - k + 1, i + 1);
		if (win.some((v) => v === null)) return null;
		return sum(win) / k;
	});
}

/**
 * Column marks in a viewBox `n` units wide (one unit per day) and 100 units high, y down.
 * Missing days draw nothing; zero draws nothing (the table still lists it).
 * @param {(number | null)[]} values @param {number} yMax @param {number} [gap] share of a slot left empty
 */
export function columnRects(values, yMax, gap = 0.25) {
	const out = [];
	values.forEach((v, i) => {
		if (v === null || v <= 0) return;
		const h = Math.min(100, (v / yMax) * 100);
		out.push({ i, value: v, x: round(i + gap / 2), y: round(100 - h), w: round(1 - gap), h: round(h) });
	});
	return out;
}

/**
 * SVG path through day centres, broken where a value is null.
 * @param {(number | null)[]} values @param {number} yMax
 */
export function linePath(values, yMax) {
	let d = '';
	let pen = false;
	values.forEach((v, i) => {
		if (v === null) {
			pen = false;
			return;
		}
		const x = round(i + 0.5);
		const y = round(100 - Math.min(100, (v / yMax) * 100));
		d += `${pen ? 'L' : 'M'}${x} ${y}`;
		pen = true;
	});
	return d;
}

/**
 * X-axis labels for a dense list of dates: the first day of each month (every second month
 * past six months), positioned as a percentage of the plot width at the day centre. Labels
 * closer than `minGap` percent to the previous one are dropped.
 * @param {string[]} dates
 * @returns {{i: number, pct: number, label: string}[]}
 */
export function dateTicks(dates, minGap = 12) {
	const n = dates.length;
	if (n === 0) return [];
	const pct = (i) => round(((i + 0.5) / n) * 100);
	let cands = dates.map((d, i) => ({ d, i })).filter(({ d }) => d.endsWith('-01'));
	if (cands.length > 6) cands = cands.filter((_, j) => j % 2 === 0);
	if (cands.length < 3) {
		// Short ranges: marks every one or more whole weeks from the first day, at most seven.
		const every = 7 * Math.max(1, Math.ceil(n / 49));
		cands = dates.map((d, i) => ({ d, i })).filter(({ i }) => i % every === 0);
	}
	const out = [];
	for (const { d, i } of cands) {
		const p = pct(i);
		if (out.length && p - out[out.length - 1].pct < minGap) continue;
		out.push({ i, pct: p, label: formatDate(d, { year: d.endsWith('-01-01') }) });
	}
	return out;
}

/**
 * Artefact kind of a GitHub release asset, from cadkit's release naming:
 * cadkit-<target> (CLI), cadkit-capi-<target> (C API), pycadkit-* (Python), cadkit-wasm-* (npm
 * tarball), SHA256SUMS (checksums).
 * @param {string} name
 */
export function assetKind(name) {
	if (name === 'SHA256SUMS' || /\.sha256$/i.test(name)) return 'Checksums';
	if (name.startsWith('cadkit-capi-')) return 'C API archives';
	if (name.startsWith('cadkit-wasm-')) return 'npm tarball';
	if (name.startsWith('pycadkit-')) return 'Python wheels and sdist';
	if (name.startsWith('cadkit-')) return 'CLI archives';
	return 'Other';
}

function round(v) {
	return Math.round(v * 1000) / 1000;
}
