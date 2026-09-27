import { describe, expect, test } from 'bun:test';
import type { KillView } from '$lib/types';
import { InfantryWindows } from './windows';
import { generateBatchFeatures } from './features';
import { replayReferenceFeatures, type ReplayRow } from './baseline-replay';

const row = (
	clock: number,
	id: string,
	player = '76561198000000001',
	map = 'Kavkazi',
	matchRow: number | null = 10
): ReplayRow => ({
	source: 'local',
	server_id: 'server',
	event_id: id,
	at: new Date(Date.UTC(2026, 0, 1, 0, 0, clock)),
	instance_id: 'boot',
	match_id: 'same-boot',
	match_row: matchRow,
	event_time: clock,
	map,
	killer_steam_id: player,
	victim_steam_id: `765611990000${String(10000 + clock).slice(-5)}`,
	killer_faction: 'Blue',
	victim_faction: 'Red',
	cause: 'Id.Item.AK74M',
	distance_m: 10,
	headshot: false,
	penetration: false,
	player_count: 40
});
const view = (r: ReplayRow): KillView => ({
	eventId: r.event_id,
	instanceId: r.instance_id,
	matchId: r.match_id,
	matchRow: r.match_row === null ? null : Number(r.match_row),
	ts: (r.at as Date).toISOString(),
	map: r.map,
	eventTime: r.event_time,
	killer: { steamId: r.killer_steam_id, name: '', faction: 'Blue' },
	victim: { steamId: r.victim_steam_id, name: '', faction: 'Red' },
	cause: r.cause,
	distanceM: r.distance_m,
	headshot: false,
	suicide: false,
	teamKill: false,
	tags: []
});

describe('clean rolling baseline replay', () => {
	test('Postgres bigint string rounds 9 to 10 and 99 to 100 keep producing samples', () => {
		const rows: ReplayRow[] = ['9', '10', '11', '99', '100'].map((match, i) => ({
			...row(i * 300, `round-${match}`),
			match_row: match,
			event_time: 5
		}));
		const values = replayReferenceFeatures(rows, new Map()).filter((s) => s.metric === 'kpm180');
		expect(values.map((s) => s.eventId).sort()).toEqual(rows.map((r) => r.event_id).sort());
		expect(values.every((s) => s.value === 1 / 3)).toBe(true);
		const numeric = replayReferenceFeatures(
			rows.map((r) => ({ ...r, match_row: Number(r.match_row) })),
			new Map()
		);
		expect(replayReferenceFeatures(rows, new Map())).toEqual(numeric);
	});
	test('invalid or unsafe round IDs cannot reset a reference window', () => {
		const rows: ReplayRow[] = ['not-a-round', '9007199254740993', '0', '-1'].map((match, i) => ({
			...row(i, `invalid-${i}`),
			match_row: match
		}));
		expect(replayReferenceFeatures(rows, new Map())).toEqual([]);
	});
	test('historical replay and live use identical feature values at every kill', () => {
		const rows = Array.from({ length: 12 }, (_, i) => row(i, `e-${i}`));
		const live = generateBatchFeatures(
			new InfantryWindows(),
			'local:server',
			rows.map(view),
			new Map()
		).features;
		const history = replayReferenceFeatures(rows, new Map()).filter(
			(sample) => sample.metric === 'kpm180'
		);
		expect(history.map((sample) => [sample.eventId, sample.value]).sort()).toEqual(
			live.map((feature) => [feature.eventId, feature.kpm180]).sort()
		);
	});
	test('same matchId across map transitions never combines rolling samples', () => {
		const rows = [
			row(3000, 'a1', undefined, 'A', 10),
			row(0, 'b1', undefined, 'B', 11),
			row(0, 'a2', undefined, 'A', 12)
		];
		const values = replayReferenceFeatures(rows, new Map()).filter(
			(sample) => sample.metric === 'kpm180'
		);
		expect(values.map((sample) => sample.value)).toEqual([1 / 3, 1 / 3, 1 / 3]);
	});
	test('one prolific player is capped within a player-day context', () => {
		const rows = Array.from({ length: 200 }, (_, i) => row(i, `e-${i}`));
		const values = replayReferenceFeatures(rows, new Map()).filter(
			(sample) => sample.metric === 'kpm180'
		);
		expect(values.length).toBe(20);
	});
	test('100 ordinary players dominate one prolific extreme player', () => {
		const ordinary = Array.from({ length: 100 }, (_, i) =>
			row(1, `normal-${i}`, `765611980000${String(10000 + i).slice(-5)}`)
		);
		const extreme = Array.from({ length: 500 }, (_, i) =>
			row(i, `extreme-${i}`, '76561198000000999')
		);
		const values = replayReferenceFeatures([...ordinary, ...extreme], new Map()).filter(
			(sample) => sample.metric === 'kpm180'
		);
		expect(values.filter((sample) => sample.player === '76561198000000999').length).toBe(20);
		expect(new Set(values.map((sample) => sample.player)).size).toBe(101);
	});
});
