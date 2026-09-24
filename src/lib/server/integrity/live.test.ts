import { describe, expect, test } from 'bun:test';
import type { KillRow } from '../db/schema';
import { liveInfantryMetrics } from './live';

const make = (clock: number, victim: string, changes: Partial<KillRow> = {}): KillRow =>
	({
		eventId: `event-${clock}-${victim}`,
		instanceId: 'round-1',
		map: 'map-a',
		eventTime: clock,
		killerSteamId: '76561198000000101',
		killerFaction: 'A',
		victimSteamId: victim,
		victimFaction: 'B',
		cause: 'Id.Item.AK74M',
		tags: [],
		suicide: false,
		...changes
	}) as KillRow;

describe('online infantry metrics', () => {
	test('uses the match clock, distinct victims and exact infantry classification', () => {
		const rows = Array.from({ length: 12 }, (_, i) => make(100 + i * 10, `victim-${i}`));
		rows.push(make(215, 'vehicle-victim', { tags: ['VehicleExplosion'] }));
		rows.push(make(216, 'unknown-victim', { cause: 'Id.Item.Unknown' }));
		const result = liveInfantryMetrics(
			rows.reverse(),
			{ map: 'map-a', matchSeconds: 220 },
			new Map()
		).get('76561198000000101');
		expect(result?.infantryKills180).toBe(12);
		expect(result?.kpm180).toBe(4);
		expect(result?.uniqueVictims180).toBe(12);
		expect(result?.peakKpm180).toBe(4);
	});

	test('old kills leave the rolling window and a new round clears the old map', () => {
		const rows = [make(300, 'b'), make(100, 'a')];
		expect(
			liveInfantryMetrics(rows, { map: 'map-a', matchSeconds: 301 }, new Map()).get(
				'76561198000000101'
			)?.infantryKills180
		).toBe(1);
		expect(liveInfantryMetrics(rows, { map: 'map-b', matchSeconds: 10 }, new Map()).size).toBe(0);
		expect(liveInfantryMetrics(rows, { map: 'map-a', matchSeconds: 10 }, new Map()).size).toBe(0);
	});
});
