import { expect, test } from 'bun:test';
import { planSkillBalance, type BalancePlayer } from './skill-balance-policy';
import type { Status } from './types';
const status = (scores = [110, 60, 40]) =>
	({ scores: scores.map((score, i) => ({ name: ['A', 'B', 'C'][i], score })) }) as Status;
const players: BalancePlayer[] = Array.from({ length: 12 }, (_, i) => ({
	steamId: `765611980000000${String(i).padStart(2, '0')}`,
	name: `p${i}`,
	faction: i < 6 ? 'A' : 'C',
	kpm: i < 6 ? 3 : 1,
	kd: i < 6 ? i + 1 : i - 5,
	kills: 10,
	deaths: 2,
	cash: 0,
	ping: 20
}));
test('top KPM then KD and bottom KPM then KD: exactly three pairs', () => {
	const plan = planSkillBalance(status(), players, 40)!;
	expect(plan.strong).toBe('A');
	expect(plan.weak).toBe('C');
	expect(plan.pairs).toHaveLength(3);
	expect(plan.pairs.map((p) => p.strong.name)).toEqual(['p5', 'p4', 'p3']);
	expect(plan.pairs.map((p) => p.weak.name)).toEqual(['p6', 'p7', 'p8']);
});
test('leader must exceed BOTH trailing teams by strictly more than 40', () => {
	expect(planSkillBalance(status([100, 60, 1]), players, 40)).toBeNull();
	expect(planSkillBalance(status([101, 60, 1]), players, 40)).not.toBeNull();
	expect(planSkillBalance(status([100, 90, 1]), players, 40)).toBeNull();
	expect(planSkillBalance(status([100, 100, 1]), players, 40)).toBeNull();
});
test('ambiguous or missing teams, insufficient eligible players, or no improvement do not swap', () => {
	expect(planSkillBalance(status([100, 0, 0]), players, 40)).toBeNull();
	expect(planSkillBalance(status([100, 1]), players, 40)).toBeNull();
	expect(planSkillBalance(status([NaN, 5, 1]), players, 40)).toBeNull();
	expect(planSkillBalance(status(), players.slice(0, 8), 40)).toBeNull();
	expect(
		planSkillBalance(
			status(),
			players.map((p) => ({ ...p, kpm: 0, kd: 0 })),
			40
		)
	).toBeNull();
});
