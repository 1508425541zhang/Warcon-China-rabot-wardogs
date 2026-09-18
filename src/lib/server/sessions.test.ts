import { describe, expect, test } from 'bun:test';
import { diffPresence, LEAVE_GRACE_MS, newPresence, type OpenSession } from './sessions';
import type { Player } from '$lib/types';

const player = (steamId: string, name = steamId): Player => ({
	name,
	steamId,
	faction: null,
	kills: 0,
	deaths: 0,
	cash: 0,
	ping: null
});
const open = (steamId: string): OpenSession => ({
	id: Number(steamId.slice(-3)),
	steamId,
	name: steamId,
	faction: null,
	kills: 0,
	deaths: 0,
	cash: 0,
	seedMs: 0,
	pendingSeedMs: 0,
	joinedAt: 1000,
	lastSeen: 2000,
	writtenAt: 2000,
	firstVisit: false,
	lastFaction: null
});

// The open sessions were last seen at 2000; this look is long after the leave grace.
const LATER = 2000 + LEAVE_GRACE_MS + 1;

describe('diffPresence', () => {
	test('splits the observed list into joined, stayed and left', () => {
		const p = newPresence();
		for (const id of ['76561198100000001', '76561198100000002']) p.open.set(id, open(id));
		const d = diffPresence(p, [player('76561198100000002'), player('76561198100000003')], LATER);
		expect(d.joined.map((x) => x.steamId)).toEqual(['76561198100000003']);
		expect(d.stayed.map((x) => x.session.steamId)).toEqual(['76561198100000002']);
		expect(d.left.map((x) => x.steamId)).toEqual(['76561198100000001']);
	});

	test('ignores duplicates and players without a SteamID', () => {
		const p = newPresence();
		const d = diffPresence(
			p,
			[player('76561198100000009'), player('76561198100000009'), player('')],
			LATER
		);
		expect(d.joined).toHaveLength(1);
		expect(d.stayed).toHaveLength(0);
	});

	test('reports players who have just picked or changed faction', () => {
		const p = newPresence();
		p.open.set('76561198100000001', open('76561198100000001'));
		p.open.set('76561198100000002', {
			...open('76561198100000002'),
			faction: 'Valkyra',
			lastFaction: 'Valkyra'
		});
		const d = diffPresence(
			p,
			[
				{ ...player('76561198100000001'), faction: 'Valkyra' },
				{ ...player('76561198100000002'), faction: 'Kessler' },
				{ ...player('76561198100000003'), faction: 'Valkyra' }
			],
			LATER
		);
		expect(d.factioned.map((x) => [x.player.steamId, x.from])).toEqual([
			['76561198100000001', null],
			['76561198100000002', 'Valkyra']
		]);
		expect(d.joined.map((x) => x.steamId)).toEqual(['76561198100000003']);
	});

	test('a side cleared at match start and picked again is not a new pick', () => {
		const p = newPresence();
		p.open.set('76561198100000001', {
			...open('76561198100000001'),
			faction: null,
			lastFaction: 'Valkyra'
		});
		p.open.set('76561198100000002', {
			...open('76561198100000002'),
			faction: null,
			lastFaction: 'Valkyra'
		});
		const d = diffPresence(
			p,
			[
				{ ...player('76561198100000001'), faction: 'Valkyra' },
				{ ...player('76561198100000002'), faction: 'Kessler' }
			],
			LATER
		);
		expect(d.factioned.map((x) => [x.player.steamId, x.from])).toEqual([
			['76561198100000002', 'Valkyra']
		]);
	});

	test('an empty list means everyone left once the grace has passed', () => {
		const p = newPresence();
		p.open.set('76561198100000001', open('76561198100000001'));
		const d = diffPresence(p, [], LATER);
		expect(d.left).toHaveLength(1);
		expect(d.joined).toHaveLength(0);
	});

	test('a player missing for less than the grace has neither left nor stayed', () => {
		const p = newPresence();
		p.open.set('76561198100000001', open('76561198100000001'));
		const d = diffPresence(p, [], 2000 + LEAVE_GRACE_MS);
		expect(d.left).toHaveLength(0);
		expect(d.stayed).toHaveLength(0);
		expect(d.joined).toHaveLength(0);
	});

	test('the list emptied at a map change and refilled is the same sessions, not joins', () => {
		const p = newPresence();
		for (const id of ['76561198100000001', '76561198100000002']) p.open.set(id, open(id));
		// the game reports nobody while the next map loads
		expect(diffPresence(p, [], 2000 + 35_000).left).toHaveLength(0);
		// the same players are back, with the sides they picked
		const d = diffPresence(
			p,
			[
				{ ...player('76561198100000001'), faction: 'Valkyra' },
				{ ...player('76561198100000002'), faction: 'Kessler' }
			],
			2000 + 36_000
		);
		expect(d.joined).toHaveLength(0);
		expect(d.left).toHaveLength(0);
		expect(d.stayed.map((x) => x.session.steamId)).toEqual([
			'76561198100000001',
			'76561198100000002'
		]);
	});

	test('a leave after the grace keeps the last time the player was seen', () => {
		const p = newPresence();
		p.open.set('76561198100000001', open('76561198100000001'));
		const d = diffPresence(p, [], LATER);
		expect(d.left[0].lastSeen).toBe(2000);
	});
});
