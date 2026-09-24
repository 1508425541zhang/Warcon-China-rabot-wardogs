import { describe, expect, test } from 'bun:test';
import type { KillView } from '$lib/types';
import { burstPoints, InfantryWindows } from './windows';
import { DEFAULT_INTEGRITY_RULES } from './score';

const kill = (clock: number, id: string, changes: Partial<KillView> = {}): KillView => ({
	eventId: id,
	instanceId: 'boot-1',
	matchId: 'boot-match-1',
	ts: new Date(1000 * (clock + 500)).toISOString(),
	map: 'Kavkazi',
	eventTime: clock,
	killer: { steamId: '76561198000000001', name: 'One', faction: 'Blue' },
	victim: {
		steamId: `765611980000${String(10000 + clock).slice(-5)}`,
		name: 'Two',
		faction: 'Red'
	},
	cause: 'Id.Item.AK74M',
	distanceM: 12,
	headshot: false,
	suicide: false,
	teamKill: false,
	tags: [],
	...changes
});

describe('180-second infantry windows', () => {
	test('twelve distinct valid kills yield KPM 4, with a traceable finding', () => {
		const windows = new InfantryWindows();
		const events = Array.from({ length: 12 }, (_, i) => kill(i, `event-${i}`));
		const findings = windows.observe('server', events, new Map());
		expect(findings).toHaveLength(1);
		expect(findings[0].kpm180).toBe(4);
		expect(findings[0].infantryKills).toBe(12);
		expect(findings[0].eventIds).toHaveLength(12);
		expect(findings[0].uniqueVictims).toBe(12);
	});

	test('the same overlapping window cannot repeatedly add findings', () => {
		const windows = new InfantryWindows();
		windows.observe(
			'server',
			Array.from({ length: 12 }, (_, i) => kill(i, `a-${i}`)),
			new Map()
		);
		expect(windows.observe('server', [kill(12, 'extra')], new Map())).toHaveLength(0);
		expect(
			windows.observe(
				'server',
				Array.from({ length: 12 }, (_, i) => kill(190 + i, `b-${i}`)),
				new Map()
			)
		).toHaveLength(1);
	});

	test('a kill exactly 180 seconds old is outside the rolling window', () => {
		const windows = new InfantryWindows();
		windows.observe(
			'server',
			Array.from({ length: 11 }, (_, i) => kill(i, `old-${i}`)),
			new Map()
		);
		expect(windows.observe('server', [kill(180, 'boundary')], new Map())).toHaveLength(0);
		expect(windows.current('server', '76561198000000001')?.kpm180).toBe(11 / 3);
	});

	test('vehicle, unknown, team kill and missing faction never contaminate KPM', () => {
		const windows = new InfantryWindows();
		const excluded = [
			kill(1, 'vehicle', { tags: ['VehicleExplosion'] }),
			kill(2, 'unknown', { cause: 'Id.Item.Unmapped' }),
			kill(3, 'team', { victim: { steamId: '76561198000000003', name: 'Ally', faction: 'Blue' } }),
			kill(4, 'unknown-team', {
				victim: { steamId: '76561198000000004', name: 'Unknown', faction: null }
			})
		];
		expect(windows.observe('server', excluded, new Map())).toHaveLength(0);
		expect(windows.current('server', '76561198000000001')).toBeNull();
	});

	test('map change and clock rewind reset the window', () => {
		const windows = new InfantryWindows();
		windows.observe(
			'server',
			Array.from({ length: 11 }, (_, i) => kill(100 + i, `a-${i}`)),
			new Map()
		);
		expect(
			windows.observe('server', [kill(1, 'next-map', { map: 'North' })], new Map())
		).toHaveLength(0);
		expect(windows.current('server', '76561198000000001')?.kpm180).toBe(1 / 3);
		expect(
			windows.observe('server', [kill(0, 'new-boot', { instanceId: 'boot-2' })], new Map())
		).toHaveLength(0);
		expect(windows.current('server', '76561198000000001')?.kpm180).toBe(1 / 3);
	});

	test('a same-map round reset cannot carry earlier kills forward', () => {
		const windows = new InfantryWindows();
		windows.observe(
			'server',
			Array.from({ length: 11 }, (_, i) => kill(20 + i, `a-${i}`)),
			new Map()
		);
		expect(windows.observe('server', [kill(0, 'new-round')], new Map())).toHaveLength(0);
		expect(windows.current('server', '76561198000000001')?.kpm180).toBe(1 / 3);
	});

	test("duplicate event IDs cannot increase a player's count", () => {
		const windows = new InfantryWindows();
		windows.observe('server', [kill(10, 'once'), kill(10, 'once')], new Map());
		expect(windows.current('server', '76561198000000001')?.kpm180).toBe(1 / 3);
	});

	test('eight kills in 15 game seconds independently trigger burst below abnormal KPM', () => {
		const findings = new InfantryWindows().observe(
			'server',
			Array.from({ length: 8 }, (_, i) => kill(i * 2, `burst-${i}`)),
			new Map()
		);
		expect(findings).toHaveLength(1);
		expect(findings[0].kpm180).toBe(8 / 3);
		expect(findings[0].burstPoints).toBe(12);
		expect(findings[0].reasons).toEqual(['burst']);
		const active = new InfantryWindows();
		const first = active.observe(
			'server',
			Array.from({ length: 8 }, (_, i) => kill(i * 2, `stable-${i}`)),
			new Map()
		);
		active.markPersisted('server', first[0], 10);
		expect(active.observe('server', [kill(50, 'stable-extra')], new Map())).toEqual([]);
	});

	test('headshot and penetration each create a finding without KPM', () => {
		const rules = { ...DEFAULT_INTEGRITY_RULES, headshotMinKills: 5, penetrationMinKills: 5 };
		const headshots = new InfantryWindows().observe(
			'server',
			Array.from({ length: 5 }, (_, i) => kill(i * 20, `head-${i}`, { headshot: true })),
			new Map(),
			rules
		);
		expect(headshots).toHaveLength(1);
		expect(headshots[0].reasons).toEqual(['headshot']);
		expect(headshots[0].headshots).toBe(5);
		expect(headshots[0].headshotPct).toBe(100);
		const penetrations = new InfantryWindows().observe(
			'server',
			Array.from({ length: 5 }, (_, i) => kill(i * 20, `pen-${i}`, { tags: ['Penetration'] })),
			new Map(),
			rules
		);
		expect(penetrations).toHaveLength(1);
		expect(penetrations[0].reasons).toEqual(['penetration']);
		expect(penetrations[0].penetrations).toBe(5);
		expect(penetrations[0].penetrationPct).toBe(100);
	});

	test('excluded kills never enter any infantry behavior metric', () => {
		const excluded = [
			kill(1, 'suicide', { suicide: true, headshot: true, tags: ['Penetration'] }),
			kill(2, 'team-kill', { teamKill: true, headshot: true, tags: ['Penetration'] }),
			kill(3, 'vehicle', { tags: ['VehicleExplosion', 'Penetration'], headshot: true }),
			kill(4, 'mortar', { cause: 'Id.Item.Mortar', headshot: true, tags: ['Penetration'] }),
			kill(5, 'fixed', { cause: 'Id.Buildable.Turret', headshot: true, tags: ['Penetration'] }),
			kill(6, 'unknown', { cause: 'Id.Item.Unmapped', headshot: true, tags: ['Penetration'] }),
			kill(7, 'unknown-faction', {
				victim: { steamId: '76561198000000007', name: 'X', faction: null },
				headshot: true,
				tags: ['Penetration']
			}),
			kill(8, 'same-faction', {
				victim: { steamId: '76561198000000008', name: 'X', faction: 'Blue' },
				headshot: true,
				tags: ['Penetration']
			})
		];
		const windows = new InfantryWindows();
		expect(windows.observe('server', excluded, new Map([['Id.Item.Mortar', 'MORTAR']]))).toEqual(
			[]
		);
		expect(windows.current('server', '76561198000000001')).toBeNull();
	});

	test('burst uses eventTime despite delayed receipt and sorts out-of-order events', () => {
		const events = Array.from({ length: 8 }, (_, i) => kill(i * 2, `clock-${i}`));
		const realTime = new InfantryWindows().observe('server', events, new Map());
		const delayed = new InfantryWindows().observe(
			'server',
			events.reverse().map((item) => ({
				...item,
				ts: new Date(Date.parse(item.ts) + 30_000).toISOString()
			})),
			new Map()
		);
		expect(delayed[0].burstPoints).toBe(realTime[0].burstPoints);
		expect(delayed[0].eventIds).toEqual(realTime[0].eventIds);
		expect(burstPoints([{ clock: 5 }, { clock: 0 }, { clock: 4 }])).toBe(2);
		const split = new InfantryWindows();
		split.observe(
			'server',
			[8, 10, 12, 14].map((clock) => kill(clock, `split-${clock}`)),
			new Map()
		);
		const late = split.observe(
			'server',
			[6, 0, 4, 2].map((clock) => kill(clock, `split-${clock}`)),
			new Map()
		);
		expect(late[0].burstPoints).toBe(12);
		expect(late[0].eventIds).toEqual([0, 2, 4, 6, 8, 10, 12, 14].map((clock) => `split-${clock}`));
	});

	test('an active window emits an upgrade but ignores unchanged tiers and duplicate IDs', () => {
		const windows = new InfantryWindows();
		const first = windows.observe(
			'server',
			Array.from({ length: 12 }, (_, i) => kill(i * 12, `slow-${i}`)),
			new Map()
		);
		expect(first).toHaveLength(1);
		expect(first[0].reasons).toEqual(['kpm']);
		windows.markPersisted('server', first[0], 42);
		expect(
			windows.observe('server', [kill(135, 'slow-extra'), kill(135, 'slow-extra')], new Map())
		).toEqual([]);
		const upgraded = windows.observe(
			'server',
			Array.from({ length: 8 }, (_, i) => kill(140 + i * 2, `fast-${i}`, { headshot: true })),
			new Map()
		);
		expect(upgraded).toHaveLength(1);
		expect(upgraded[0].windowId).toBe(42);
		expect(upgraded[0].reasons).toContain('burst');
		expect(upgraded[0].eventIds).toContain('fast-7');
		expect(windows.observe('server', [kill(154, 'fast-7')], new Map())).toEqual([]);
	});
});
