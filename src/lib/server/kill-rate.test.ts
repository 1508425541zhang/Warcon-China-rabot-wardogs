import { describe, expect, test } from 'bun:test';
import {
	countsForRate,
	killRateReplay,
	killRateStep,
	killRateVerdict,
	killTimes,
	pruneTracks,
	validateKillRate,
	type KillRateConfig,
	type RateTracks
} from './kill-rate';

const cfg = (c: Partial<KillRateConfig> = {}): KillRateConfig => ({
	windowMinutes: 5,
	maxKills: 10,
	headshotPct: 0,
	headshotMinKills: 15,
	cooldownMinutes: 30,
	...c
});
const MIN = 60_000;
const A = '76561198000000001';
const B = '76561198000000002';

describe('validateKillRate', () => {
	test('needs a kill count or a headshot share', () => {
		expect(() => validateKillRate({})).toThrow('kill count, a headshot share');
		expect(() => validateKillRate({ maxKills: 0, headshotPct: 0 })).toThrow();
	});
	test('fills defaults and clamps', () => {
		expect(validateKillRate({ maxKills: 20 })).toEqual({
			windowMinutes: 5,
			maxKills: 20,
			headshotPct: 0,
			headshotMinKills: 15,
			cooldownMinutes: 30
		});
		const c = validateKillRate({
			headshotPct: 400,
			windowMinutes: 0,
			headshotMinKills: -3,
			cooldownMinutes: 99999
		});
		expect(c).toMatchObject({
			headshotPct: 100,
			windowMinutes: 1,
			headshotMinKills: 1,
			cooldownMinutes: 1440
		});
	});
});

describe('countsForRate', () => {
	test('hand-held weapon kills by a player only', () => {
		expect(countsForRate({ killer: A, suicide: false, cause: 'Id.Item.AK74M' })).toBe(true);
		expect(countsForRate({ killer: A, suicide: false, cause: 'Id.Item.M67Grenade' })).toBe(true);
		expect(countsForRate({ killer: null, suicide: false, cause: 'Id.Item.AK74M' })).toBe(false);
		expect(countsForRate({ killer: A, suicide: true, cause: 'Id.Item.AK74M' })).toBe(false);
		expect(countsForRate({ killer: A, suicide: false, cause: null })).toBe(false);
		expect(
			countsForRate({
				killer: A,
				suicide: false,
				cause: 'Id.Vehicle.WeaponExtension.STN_02.MainCannon'
			})
		).toBe(false);
		expect(
			countsForRate({
				killer: A,
				suicide: false,
				cause: 'Vehicle.Variant.Land.Wheeled.Kodiak.Pickup'
			})
		).toBe(false);
		expect(countsForRate({ killer: A, suicide: false, cause: 'Id.Buildable.BarbedWire' })).toBe(
			false
		);
	});
});

describe('killRateVerdict', () => {
	test('the kill count', () => {
		expect(killRateVerdict(cfg(), 9, 0)).toBeNull();
		expect(killRateVerdict(cfg(), 10, 0)).toBe('10 kills in 5 min');
		expect(killRateVerdict(cfg({ maxKills: 1 }), 1, 0)).toBe('1 kill in 5 min');
	});
	test('the headshot share, only from its minimum', () => {
		const c = cfg({ maxKills: 0, headshotPct: 70, headshotMinKills: 10 });
		expect(killRateVerdict(c, 9, 9)).toBeNull();
		expect(killRateVerdict(c, 10, 6)).toBeNull();
		expect(killRateVerdict(c, 10, 7)).toBe('70% headshots over 10 kills in 5 min');
	});
	test('0 turns each part off', () => {
		expect(killRateVerdict(cfg({ maxKills: 0 }), 500, 500)).toBeNull();
	});
});

describe('killRateStep', () => {
	test('counts a player over the window only', () => {
		const t: RateTracks = new Map();
		const c = cfg({ maxKills: 3 });
		expect(killRateStep(c, t, A, 0, false)).toBeNull();
		expect(killRateStep(c, t, A, 1 * MIN, false)).toBeNull();
		// the first kill has left the window by the third
		expect(killRateStep(c, t, A, 5 * MIN, false)).toBeNull();
		expect(killRateStep(c, t, A, 5 * MIN + 1, false)).toBe('3 kills in 5 min');
	});
	test('players are counted apart', () => {
		const t: RateTracks = new Map();
		const c = cfg({ maxKills: 2 });
		expect(killRateStep(c, t, A, 0, false)).toBeNull();
		expect(killRateStep(c, t, B, 1, false)).toBeNull();
		expect(killRateStep(c, t, B, 2, false)).toBe('2 kills in 5 min');
	});
	test('a flagged player is not flagged again until the cooldown has passed', () => {
		const t: RateTracks = new Map();
		const c = cfg({ maxKills: 2, cooldownMinutes: 10 });
		killRateStep(c, t, A, 0, false);
		expect(killRateStep(c, t, A, 1000, false)).not.toBeNull();
		expect(killRateStep(c, t, A, 2000, false)).toBeNull();
		expect(killRateStep(c, t, A, 9 * MIN, false)).toBeNull();
		killRateStep(c, t, A, 10 * MIN + 500, false);
		expect(killRateStep(c, t, A, 10 * MIN + 1000, false)).not.toBeNull();
	});
});

describe('pruneTracks', () => {
	test('forgets a player once the window is empty and the cooldown over', () => {
		const t: RateTracks = new Map();
		const c = cfg({ maxKills: 2, cooldownMinutes: 10 });
		killRateStep(c, t, A, 0, false);
		killRateStep(c, t, A, 1, false); // flagged at 1
		killRateStep(c, t, B, 0, false);
		pruneTracks(c, t, 6 * MIN);
		// B's kill is out of the window; A is still cooling down
		expect([...t.keys()]).toEqual([A]);
		pruneTracks(c, t, 11 * MIN);
		expect(t.size).toBe(0);
	});
});

describe('killRateReplay', () => {
	test('flags as the live rule would, in order', () => {
		const kills = [0, 1, 2, 3].map((i) => ({
			at: i * 1000,
			steamId: A,
			name: 'Krieger',
			headshot: true
		}));
		const got = killRateReplay(cfg({ maxKills: 0, headshotPct: 90, headshotMinKills: 3 }), kills);
		expect(got.map((f) => [f.at, f.verdict])).toEqual([
			[2000, '100% headshots over 3 kills in 5 min']
		]);
	});
});

describe('killTimes', () => {
	test('spaces a batch out by the match clock, back from its latest kill', () => {
		expect(killTimes(1_000_000, [100, 400, 700])).toEqual([400_000, 700_000, 1_000_000]);
	});
	test('a batch at one moment stays at its receipt time', () => {
		expect(killTimes(5000, [42, 42])).toEqual([5000, 5000]);
	});
	test('a new match inside the batch sends its kills back, never forward', () => {
		const [before, after] = killTimes(1_000_000, [900, 3]);
		expect(before).toBe(1_000_000);
		expect(after).toBeLessThan(1_000_000 - 5 * MIN);
	});
});

describe('kills out of order', () => {
	test('a late kill lands in its place, not on top of the window', () => {
		const t: RateTracks = new Map();
		const c = cfg({ maxKills: 3 });
		killRateStep(c, t, A, 10 * MIN, false);
		// arrives after the newer one, from before the window that kill judges
		expect(killRateStep(c, t, A, 4 * MIN, false)).toBeNull();
		expect(t.get(A)!.at).toEqual([10 * MIN]);
		expect(killRateStep(c, t, A, 10 * MIN + 1, false)).toBeNull();
		expect(killRateStep(c, t, A, 9 * MIN, false)).toBe('3 kills in 5 min');
	});
	test('a replay sorts what it is given', () => {
		const k = (at: number) => ({ at, steamId: A, name: 'a', headshot: false });
		const got = killRateReplay(cfg({ maxKills: 2 }), [k(2000), k(1000)]);
		expect(got.map((f) => f.at)).toEqual([2000]);
	});
});
