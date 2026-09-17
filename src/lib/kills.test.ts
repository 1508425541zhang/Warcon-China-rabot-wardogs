import { describe, expect, test } from 'bun:test';
import {
	EMPTY_FILTER,
	isEmptyFilter,
	killFilterParams,
	killMatches,
	parseKillFilter,
	type KillFilter
} from './kills';
import type { KillView } from './types';

const kill = (over: Partial<KillView> = {}): KillView => ({
	eventId: 'e1',
	ts: '2026-09-16T20:00:00.000Z',
	map: 'Kavkazi',
	eventTime: 100,
	killer: { steamId: '76561198000000001', name: '[TLR] Alice', faction: 'Valkyra' },
	victim: { steamId: '76561198000000002', name: 'Bob', faction: 'Lonestar' },
	cause: 'Id.Item.AK74M',
	distanceM: 42,
	headshot: false,
	suicide: false,
	teamKill: false,
	tags: [],
	...over
});
const f = (over: Partial<KillFilter>): KillFilter => ({ ...EMPTY_FILTER, ...over });

describe('parseKillFilter', () => {
	test('reads what the page sends and drops the rest', () => {
		const p = new URLSearchParams(
			'player=ali&killer=76561198000000001&victim=%20bob%20&cause=Id.Item.AK74M&kind=headshot&minM=250.6&foo=bar'
		);
		expect(parseKillFilter(p)).toEqual({
			player: 'ali',
			killer: '76561198000000001',
			victim: 'bob',
			cause: 'Id.Item.AK74M',
			kind: 'headshot',
			minM: 251
		});
	});

	test('an unknown kind or a bad distance is no filter', () => {
		expect(parseKillFilter(new URLSearchParams('kind=melee&minM=-5'))).toEqual(EMPTY_FILTER);
		expect(parseKillFilter(new URLSearchParams('minM=abc'))).toEqual(EMPTY_FILTER);
		expect(isEmptyFilter(parseKillFilter(new URLSearchParams('')))).toBe(true);
	});

	test('round-trips through its query parameters', () => {
		const filter = f({ victim: 'bob', kind: 'vehicle', minM: 300 });
		const p = new URLSearchParams(killFilterParams(filter));
		expect(parseKillFilter(p)).toEqual(filter);
		expect(killFilterParams(EMPTY_FILTER)).toEqual({});
	});
});

describe('killMatches', () => {
	test('names by fragment, SteamIDs exactly, on the side asked for', () => {
		expect(killMatches(f({ killer: 'alice' }), kill())).toBe(true);
		expect(killMatches(f({ killer: 'bob' }), kill())).toBe(false);
		expect(killMatches(f({ victim: '76561198000000002' }), kill())).toBe(true);
		expect(killMatches(f({ victim: '7656119800000000' }), kill())).toBe(false);
		expect(killMatches(f({ player: 'BOB' }), kill())).toBe(true);
		expect(killMatches(f({ player: '76561198000000001' }), kill())).toBe(true);
		expect(killMatches(f({ player: 'carol' }), kill())).toBe(false);
	});

	test('an environment kill has no killer to match', () => {
		const fall = kill({ killer: null, cause: null, tags: ['Falling'] });
		expect(killMatches(f({ killer: 'alice' }), fall)).toBe(false);
		expect(killMatches(f({ player: 'bob' }), fall)).toBe(true);
		expect(killMatches(f({ kind: 'environment' }), fall)).toBe(true);
		expect(killMatches(f({ kind: 'environment' }), kill())).toBe(false);
	});

	test('cause exactly, distance at least', () => {
		expect(killMatches(f({ cause: 'Id.Item.AK74M' }), kill())).toBe(true);
		expect(killMatches(f({ cause: 'Id.Item.Mosin' }), kill())).toBe(false);
		expect(killMatches(f({ minM: 42 }), kill())).toBe(true);
		expect(killMatches(f({ minM: 43 }), kill())).toBe(false);
		expect(killMatches(f({ minM: 1 }), kill({ distanceM: null }))).toBe(false);
	});

	test('kinds', () => {
		expect(killMatches(f({ kind: 'headshot' }), kill({ headshot: true }))).toBe(true);
		expect(killMatches(f({ kind: 'headshot' }), kill())).toBe(false);
		expect(killMatches(f({ kind: 'teamKill' }), kill({ teamKill: true }))).toBe(true);
		expect(killMatches(f({ kind: 'suicide' }), kill({ suicide: true }))).toBe(true);
		const v = f({ kind: 'vehicle' });
		expect(killMatches(v, kill({ cause: 'Vehicle.Variant.Air.Rotary.Littlebird.Default' }))).toBe(
			true
		);
		expect(killMatches(v, kill({ cause: 'Id.Vehicle.WeaponExtension.STN_03.MainBarrel' }))).toBe(
			true
		);
		expect(killMatches(v, kill({ cause: null, tags: ['VehicleExplosion'] }))).toBe(true);
		expect(killMatches(v, kill({ tags: ['RoadKill'] }))).toBe(true);
		expect(killMatches(v, kill())).toBe(false);
	});

	test('filters combine', () => {
		const both = f({ killer: 'alice', kind: 'headshot', minM: 40 });
		expect(killMatches(both, kill({ headshot: true }))).toBe(true);
		expect(killMatches(both, kill({ headshot: true, distanceM: 10 }))).toBe(false);
	});
});
