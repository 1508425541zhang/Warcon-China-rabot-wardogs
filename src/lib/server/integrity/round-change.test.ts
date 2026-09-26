import { expect, test } from 'bun:test';
import { roundKillSeries } from './round-change';

test('change series includes completed current-round buckets only', () => {
	const series = roundKillSeries([-1, 1, 14, 15, 299, 300, 305], 0, 307);
	expect(series).toHaveLength(20);
	expect(series[0]).toBe(8);
	expect(series[1]).toBe(4);
	expect(series[19]).toBe(4);
	expect(series.reduce((a, b) => a + b, 0)).toBe(16);
});
test('late joins never invent earlier zero-performance history', () => {
	expect(roundKillSeries([1, 290], 120, 300)).toEqual([]);
	expect(roundKillSeries([1, 125, 450], 121, 450)).toHaveLength(21);
});
