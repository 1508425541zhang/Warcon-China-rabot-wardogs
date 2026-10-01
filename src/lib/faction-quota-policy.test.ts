import { expect, test } from 'bun:test';
import { quotaTeams, planQuota, quotaInput, quotaColors } from './faction-quota-policy';
import type { Player } from './types';

const scores = [
	{ name: 'Manticore', colorHex: '#1DD65C', score: 0 },
	{ name: 'Lonestar', colorHex: '#4CB1EF', score: 0 },
	{ name: 'Valkyra', colorHex: '#FA503E', score: 0 }
];
const teams = { blue: 'Lonestar', red: 'Valkyra', green: 'Manticore' };
const player = (index: number, faction: string): Player => ({
	steamId: String(76561198000000000n + BigInt(index)),
	name: 'player',
	faction,
	kills: 0,
	deaths: 0,
	cash: 0,
	ping: 0
});
test('official colours identify teams independently of scoreboard order; ambiguous colours stop allocation', () => {
	expect(quotaTeams(scores)).toEqual(teams);
	expect(quotaTeams([{ ...scores[0], colorHex: '#888888' }, ...scores.slice(1)])).toBeNull();
	expect(quotaTeams([scores[0], scores[1], { ...scores[2], colorHex: '#4CB1EF' }])).toBeNull();
});
test('zero closes exactly one selected side; all populations from 0 to 100 stay within quota', () => {
	for (const closed of quotaColors)
		for (let n = 0; n <= 100; n++) {
			const limits = { blue: 50, red: 50, green: 50 };
			limits[closed] = 0;
			const players = Array.from({ length: n }, (_, i) => player(i, teams[quotaColors[i % 3]]));
			const plan = planQuota(players, teams, limits);
			const projected = { ...plan.counts };
			for (const move of plan.moves) {
				const from = quotaColors.find((c) => teams[c] === move.from)!;
				const to = quotaColors.find((c) => teams[c] === move.to)!;
				projected[from]--;
				projected[to]++;
			}
			expect(projected).toEqual(plan.targets);
			expect(projected[closed]).toBe(0);
			expect(Object.values(projected).reduce((a, b) => a + b, 0)).toBe(n);
			expect(Object.values(projected).every((v) => v <= 50)).toBe(true);
		}
});
test('odd balanced populations do not oscillate; unknown/unassigned players and cooled moves are left alone', () => {
	const roster = [
		...Array.from({ length: 25 }, (_, i) => player(i, teams.blue)),
		...Array.from({ length: 26 }, (_, i) => player(i + 30, teams.red))
	];
	expect(planQuota(roster, teams, { blue: 50, red: 50, green: 0 }).moves).toHaveLength(0);
	const p = player(100, teams.green);
	expect(
		planQuota([p], teams, { blue: 50, red: 50, green: 0 }, new Set([p.steamId])).moves
	).toHaveLength(0);
	expect(
		planQuota([player(101, 'Holding')], teams, { blue: 50, red: 50, green: 0 }).moves
	).toHaveLength(0);
});
test('asymmetric quotas are proportional; exhausted capacity does not invent a kick action', () => {
	const players = Array.from({ length: 50 }, (_, i) => player(i, teams.green));
	expect(planQuota(players, teams, { blue: 60, red: 40, green: 0 }).targets).toEqual({
		blue: 30,
		red: 20,
		green: 0
	});
	const full = planQuota(players, teams, { blue: 10, red: 10, green: 0 });
	expect(full.moves).toHaveLength(0);
	expect(full.reason).toContain('不会踢人');
});
test('reject invalid or unsupported capacities and accept any zero side', () => {
	const base = { revision: 'empty', enabled: true, graceSeconds: 60 };
	for (const limits of [
		{ blue: -1, red: 50, green: 0 },
		{ blue: 0, red: 0, green: 0 },
		{ blue: 100, red: 0, green: 0 },
		{ blue: 50.5, red: 49.5, green: 0 },
		{ blue: 51, red: 50, green: 0 }
	])
		expect(quotaInput.safeParse({ ...base, limits }).success).toBe(false);
	expect(quotaInput.safeParse({ ...base, limits: { blue: 0, red: 50, green: 50 } }).success).toBe(
		true
	);
});
