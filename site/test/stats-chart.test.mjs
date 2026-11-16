// Pure chart, scale and formatting helpers behind /stats/.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	niceAxis,
	formatTick,
	formatCount,
	formatPercent,
	addDays,
	dateRange,
	formatDate,
	formatDateSpan,
	formatTimestamp,
	densify,
	trailingMean,
	columnRects,
	linePath,
	dateTicks,
	assetKind,
	sum,
} from '../src/stats/chart.mjs';

test('niceAxis covers the maximum with 1/2/2.5/5 steps from zero', () => {
	assert.deepEqual(niceAxis(640), { max: 750, step: 250, ticks: [0, 250, 500, 750] });
	assert.deepEqual(niceAxis(100), { max: 100, step: 25, ticks: [0, 25, 50, 75, 100] });
	assert.deepEqual(niceAxis(4122).ticks, [0, 1000, 2000, 3000, 4000, 5000]);
	assert.equal(niceAxis(13).max, 15);
	assert.deepEqual(niceAxis(1).ticks, [0, 1], 'counts never get fractional ticks');
	assert.deepEqual(niceAxis(2).ticks, [0, 1, 2]);
	for (const max of [1, 7, 99, 101, 2782, 123456]) {
		const a = niceAxis(max);
		assert.ok(a.max >= max, `${max}: axis max ${a.max}`);
		assert.equal(a.ticks[0], 0);
		assert.equal(a.ticks.at(-1), a.max);
		assert.ok(a.ticks.length >= 2 && a.ticks.length <= 7, `${max}: ${a.ticks.length} ticks`);
	}
	// Degenerate input still gives a usable axis.
	assert.deepEqual(niceAxis(0).ticks.slice(0, 2), [0, niceAxis(0).step]);
	assert.ok(niceAxis(Number.NaN).max > 0);
});

test('tick and count formatting', () => {
	assert.equal(formatCount(10785), '10,785');
	assert.equal(formatTick(8000, 8000), '8,000');
	assert.equal(formatTick(20000, 40000), '20K');
	assert.equal(formatTick(2500, 10000), '2.5K');
	assert.equal(formatTick(1_500_000, 2_000_000), '1.5M');
	assert.equal(formatPercent(4122, 5085), '81%');
	assert.equal(formatPercent(188, 5085), '3.7%');
	assert.equal(formatPercent(1, 5000), '<0.1%');
	assert.equal(formatPercent(0, 0), '0%');
	assert.equal(sum([1, null, 2]), 3);
});

test('dates', () => {
	assert.equal(addDays('2026-10-31', 1), '2026-11-01');
	assert.equal(addDays('2026-01-01', -1), '2025-12-31');
	assert.deepEqual(dateRange('2026-02-27', '2026-03-01'), ['2026-02-27', '2026-02-28', '2026-03-01']);
	assert.equal(formatDate('2026-10-01'), 'Oct 1');
	assert.equal(formatDate('2026-10-01', { year: true }), 'Oct 1, 2026');
	assert.equal(formatDateSpan('2026-10-01', '2026-11-16'), 'Oct 1 – Nov 16, 2026');
	assert.equal(formatDateSpan('2025-12-01', '2026-01-16'), 'Dec 1, 2025 – Jan 16, 2026');
	assert.equal(formatTimestamp('2026-11-16T15:44:35Z'), '2026-11-16 15:44 UTC');
});

test('densify marks days a series does not contain as null, never 0', () => {
	const dates = dateRange('2026-11-01', '2026-11-04');
	assert.deepEqual(
		densify([['2026-11-01', 5], ['2026-11-03', 0]], dates),
		[5, null, 0, null],
	);
	assert.deepEqual(densify([['2026-11-02', 9, 4]], dates, 2), [null, 4, null, null]);
});

test('trailingMean needs a full window of collected days', () => {
	const v = [7, 7, 7, 7, 7, 7, 14, null, 7, 7, 7, 7, 7, 7, 7];
	const m = trailingMean(v, 7);
	assert.deepEqual(m.slice(0, 6), [null, null, null, null, null, null]);
	assert.equal(m[6], 8);
	assert.equal(m[7], null);
	assert.equal(m[13], null);
	assert.equal(m[14], 7);
});

test('columnRects: one unit per day, 100 high, nothing for null or zero', () => {
	const r = columnRects([50, null, 0, 100, 150], 100, 0.25);
	assert.deepEqual(r, [
		{ i: 0, value: 50, x: 0.125, y: 50, w: 0.75, h: 50 },
		{ i: 3, value: 100, x: 3.125, y: 0, w: 0.75, h: 100 },
		{ i: 4, value: 150, x: 4.125, y: 0, w: 0.75, h: 100 },
	]);
});

test('linePath goes through day centres and breaks at nulls', () => {
	assert.equal(linePath([0, 50, null, 100, 25], 100), 'M0.5 100L1.5 50M3.5 0L4.5 75');
	assert.equal(linePath([null, null], 100), '');
});

test('dateTicks: month starts on long ranges, weekly on short ones, never crowded', () => {
	const long = dateRange('2026-08-18', '2026-11-15');
	const t = dateTicks(long);
	assert.deepEqual(t.map((x) => x.label), ['Sep 1', 'Oct 1', 'Nov 1']);
	for (const x of t) assert.ok(x.pct > 0 && x.pct < 100);
	const short = dateRange('2026-11-01', '2026-11-14');
	assert.deepEqual(dateTicks(short).map((x) => x.label), ['Nov 1', 'Nov 8']);
	const year = dateRange('2026-01-01', '2026-12-31');
	const yt = dateTicks(year);
	assert.ok(yt.length <= 7);
	assert.equal(yt[0].label, 'Jan 1, 2026');
	for (let i = 1; i < yt.length; i++) assert.ok(yt[i].pct - yt[i - 1].pct >= 12);
	assert.deepEqual(dateTicks([]), []);
});

test('assetKind follows the release naming', () => {
	assert.equal(assetKind('SHA256SUMS'), 'Checksums');
	assert.equal(assetKind('cadkit-capi-x86_64-pc-windows-msvc.zip'), 'C API archives');
	assert.equal(assetKind('cadkit-x86_64-unknown-linux-musl.tar.gz'), 'CLI archives');
	assert.equal(assetKind('cadkit-wasm-0.5.3.tgz'), 'npm tarball');
	assert.equal(assetKind('pycadkit-0.5.3-cp39-abi3-win_amd64.whl'), 'Python wheels and sdist');
	assert.equal(assetKind('notes.txt'), 'Other');
});
