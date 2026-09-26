import { expect, test } from 'bun:test';
import { cashProgress } from './player-progress';
test('uses real balances, tracks spending, and normalizes late joins at first observation', () => {
	const start = '2026-09-26T00:00:00Z';
	const rows = [
		{
			observedAt: start,
			players: [
				{ steamId: 'a', cash: 100 },
				{ steamId: 'b', cash: 300 }
			]
		},
		{
			observedAt: '2026-09-26T00:01:00Z',
			players: [
				{ steamId: 'a', cash: 150 },
				{ steamId: 'b', cash: 320 },
				{ steamId: 'c', cash: 500 }
			]
		},
		{ observedAt: '2026-09-26T00:02:00Z', players: [{ steamId: 'a', cash: 80 }] }
	];
	const p = cashProgress(rows, 'a', start);
	expect(p.map((v) => v.cash)).toEqual([100, 150, 80]);
	expect(p.map((v) => v.growth)).toEqual([0, 50, -20]);
	expect(p[1].serverGrowth).toBeCloseTo(70 / 3);
	expect(p[2].seconds).toBe(120);
	expect(cashProgress(rows, 'missing', start)).toEqual([]);
});
