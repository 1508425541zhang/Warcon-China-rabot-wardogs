import { expect, test } from 'bun:test';
import { weaponDistanceDistribution as distribution } from './weapon-distance-distribution';
test('each player counts once, equal values share a bin and marker fits axis', () => {
	const d = distribution([10, 10, 100], 250);
	expect(d.players).toBe(3);
	expect(d.bins.reduce((n, b) => n + b.count, 0)).toBe(3);
	expect(d.peak).toBe(2);
	expect(d.upper).toBeGreaterThan(250);
	expect(d.bins.find((b) => b.from <= 100 && b.to > 100)?.count).toBe(1);
});
test('empty and constant distributions have finite axes without invented samples', () => {
	for (const values of [[], [20], [20, 20]]) {
		const d = distribution(values, 20);
		expect(d.players).toBe(values.length);
		expect(d.bins.reduce((n, b) => n + b.count, 0)).toBe(values.length);
		expect(Number.isFinite(d.upper / d.peak)).toBe(true);
	}
});
