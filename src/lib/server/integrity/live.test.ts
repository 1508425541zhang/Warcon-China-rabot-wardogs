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
	test('60 and 180 second windows count independently, even without any risk finding', () => {
		const rows = [make(240, 'a'), make(200, 'b'), make(100, 'c'), make(60, 'd')];
		const m = liveInfantryMetrics(rows, { map: 'map-a', matchSeconds: 240 }, new Map()).get(
			'76561198000000101'
		);
		expect(m?.infantryKills180).toBe(3);
		expect(m?.kpm180).toBe(1);
		expect(m?.infantryKills60).toBe(2);
		expect(m?.kpm60).toBe(2);
	});
	test('a quiet server without a game clock still ages old kills out', () => {
		const rows = [make(240, 'a', { ts: new Date(Date.now() - 181_000) })];
		const m = liveInfantryMetrics(rows, { map: 'map-a', matchSeconds: null }, new Map()).get(
			'76561198000000101'
		);
		expect(m?.kpm180).toBe(0);
		expect(m?.kpm60).toBe(0);
	});
	test('a live display name does not hide accepted kills with a catalog map ID', () => {
		const result = liveInfantryMetrics(
			[
				make(100, 'victim', {
					map: 'Europe',
					factionBracketed: true,
					factionObservedAt: new Date()
				})
			],
			{ map: 'Ozeti', matchSeconds: 101 },
			new Map()
		).get('76561198000000101');
		expect(result?.kpm180).toBeCloseTo(1 / 3);
		expect(result?.reliable).toBe(true);
	});
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
		expect(result?.reliable).toBe(false); // An unclassified cause may have been infantry.
	});
	test('missing faction makes a plausible infantry KPM unavailable instead of zero', () => {
		const result = liveInfantryMetrics(
			[make(100, 'victim', { killerFaction: null })],
			{ map: 'map-a', matchSeconds: 101 },
			new Map()
		).get('76561198000000101');
		expect(result?.kpm180).toBe(0);
		expect(result?.reliable).toBe(false);
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
