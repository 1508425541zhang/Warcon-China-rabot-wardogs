import { describe, expect, test } from 'bun:test';
import { abandoned, awardsFor, durationOf, perMinute, type MatchLine } from './matches';

const line = (steamId: string, extra: Partial<MatchLine> = {}): MatchLine => ({
	steamId,
	name: steamId,
	faction: 'Valkyra',
	seconds: 3600,
	kills: 0,
	deaths: 0,
	cashDelta: 0,
	headshots: 0,
	teamKills: 0,
	suicides: 0,
	vehicleKills: 0,
	longestM: null,
	killStreak: 0,
	deathStreak: 0,
	result: 'win',
	...extra
});

describe('awards', () => {
	const lines = [
		line('a', { kills: 31, deaths: 5, longestM: 120, killStreak: 9, cashDelta: 4000 }),
		line('b', { kills: 12, deaths: 1, longestM: 412, killStreak: 4, cashDelta: 12_400 }),
		line('c', { kills: 9, deaths: 0, longestM: 40, killStreak: 2, cashDelta: -300 })
	];

	test('the five, judged as the page says', () => {
		expect(awardsFor(lines, 3600).map((a) => [a.key, a.steamId, a.value])).toEqual([
			['kills', 'a', '31'],
			['kd', 'b', '12.00'],
			['longest', 'b', '412 m'],
			['streak', 'a', '9'],
			['cash', 'b', '+$12,400']
		]);
	});

	test('none for a short match, and only what was earned', () => {
		expect(awardsFor(lines, 19 * 60)).toEqual([]);
		const quiet = [line('a', { kills: 2, killStreak: 2 }), line('b', { cashDelta: -50 })];
		expect(awardsFor(quiet, 3600).map((a) => a.key)).toEqual(['kills']);
		expect(awardsFor([], 3600)).toEqual([]);
	});
});

describe('a match', () => {
	test('is abandoned when it ended without scores', () => {
		expect(abandoned({ endedAt: '2026-09-22T13:00:00Z', finalScores: null })).toBe(true);
		expect(abandoned({ endedAt: '2026-09-22T13:00:00Z', finalScores: [] })).toBe(true);
		expect(
			abandoned({ endedAt: '2026-09-22T13:00:00Z', finalScores: [{ name: 'Valkyra', score: 100 }] })
		).toBe(false);
		expect(abandoned({ endedAt: null, finalScores: null })).toBe(false);
	});

	test('lasts from its start to its end, or to now', () => {
		expect(durationOf({ startedAt: '2026-09-22T13:00:00Z', endedAt: '2026-09-22T14:30:00Z' })).toBe(
			5400
		);
		expect(
			durationOf(
				{ startedAt: '2026-09-22T13:00:00Z', endedAt: null },
				Date.parse('2026-09-22T13:10:00Z')
			)
		).toBe(600);
	});

	test('kills per minute needs time on', () => {
		expect(perMinute(30, 1800)).toBe(1);
		expect(perMinute(3, 0)).toBeNull();
	});
});
