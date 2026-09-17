import { describe, expect, test } from 'bun:test';
import { publicKill } from './public';
import type { KillView } from '$lib/types';

const kill: KillView = {
	eventId: 'e1',
	ts: '2026-09-17T20:00:00.000Z',
	map: 'Kavkazi',
	eventTime: 412.5,
	killer: { steamId: '76561198100000101', name: 'Ghostpepper', faction: 'Valkyra' },
	victim: { steamId: '76561198100000105', name: 'Nomad', faction: 'Lonestar' },
	cause: 'Id.Item.AK74M',
	distanceM: 61.2,
	headshot: true,
	suicide: false,
	teamKill: false,
	tags: ['Penetration']
};

describe('publicKill', () => {
	test('keeps names, factions, weapon and distance and drops both SteamIDs', () => {
		const k = publicKill(kill);
		expect(k).toEqual({
			eventId: 'e1',
			ts: '2026-09-17T20:00:00.000Z',
			eventTime: 412.5,
			killer: { name: 'Ghostpepper', faction: 'Valkyra' },
			victim: { name: 'Nomad', faction: 'Lonestar' },
			cause: 'Id.Item.AK74M',
			distanceM: 61.2,
			headshot: true,
			suicide: false,
			teamKill: false,
			tags: ['Penetration']
		});
		expect(JSON.stringify(k)).not.toContain('7656119');
	});
	test('an environment kill has no killer', () => {
		expect(publicKill({ ...kill, killer: null }).killer).toBeNull();
	});
});
