import { test, expect } from 'bun:test';
import { numericBreaches, defaultNumericLimits, validNumericLimits } from './numeric-limit-policy';
const rule = {
	...defaultNumericLimits,
	enabled: true,
	kpm: 3,
	kd: 5,
	cash: 500,
	windowSeconds: 60,
	minKills: 10
};
const points = [
	{ at: 0, kills: 10, deaths: 2, cash: 100 },
	{ at: 30000, kills: 12, deaths: 2, cash: 300 },
	{ at: 60000, kills: 14, deaths: 2, cash: 700 }
];
test('strict limits calculate scoreboard KPM, round KD and cash net rate', () => {
	expect(numericBreaches(rule, points).map((b) => b.metric)).toEqual(['kpm', 'kd', 'cash']);
	expect(numericBreaches({ ...rule, kpm: 4, kd: 7, cash: 600 }, points)).toEqual([]);
});
test('missing counters, resets and gaps cannot trigger', () => {
	for (const altered of [
		[points[0], { ...points[2], kills: NaN }],
		[points[0], { ...points[2], kills: 0 }],
		[points[0], { ...points[2], at: 120000 }]
	])
		expect(numericBreaches(rule, altered)).toEqual([]);
});
test('spending reduces cash rate and KD requires enough kills', () => {
	expect(
		numericBreaches(
			{ ...rule, kpm: null, minKills: 20 },
			points.map((p) => ({ ...p, cash: 100 }))
		)
	).toEqual([]);
});
test('settings reject empty enabled rules, nonfinite and zero values', () => {
	expect(validNumericLimits(rule)).toBe(true);
	for (const r of [
		{ ...rule, kpm: 0 },
		{ ...rule, kpm: Infinity },
		{ ...rule, windowSeconds: 0 },
		{ ...defaultNumericLimits, enabled: true }
	])
		expect(validNumericLimits(r)).toBe(false);
});
