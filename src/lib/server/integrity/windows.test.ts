import { describe, expect, test } from 'bun:test';
import type { KillView } from '$lib/types';
import { InfantryWindows } from './windows';

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
});
