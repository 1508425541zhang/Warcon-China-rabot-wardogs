import { describe, expect, test } from 'bun:test';
import { publicKill, publicStatus, type PublicServer } from './public';
import type { KillView, LiveView } from '$lib/types';

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

const live = {
	serverId: 's1',
	ok: true,
	error: '',
	tier: 'hot',
	build: '',
	gameServerId: 'abc',
	startedAt: null,
	reservedSlots: null,
	status: {
		serverName: 'x',
		map: 'Kavkazi',
		experiences: [],
		lighting: 'DayClear',
		alternator: '',
		scoreTick: null,
		scoreTickMin: null,
		scoreTickMax: null,
		scoreCap: 100,
		matchSeconds: null,
		playerCount: 1,
		maxPlayers: 32,
		scores: [],
		rotationNow: 0,
		rotationNext: 0
	},
	players: [
		{
			name: 'Nomad',
			steamId: '76561198100000105',
			faction: 'Lonestar',
			kills: 3,
			deaths: 1,
			cash: 0,
			ping: 20
		}
	],
	statusAt: null,
	playersAt: null,
	observedAt: '2026-09-17T20:00:00.000Z'
} as unknown as LiveView;
const server = (leaderboards: boolean) =>
	({
		server: { id: 's1', name: 'One', publicKills: false },
		org: { name: 'Org', discordInviteUrl: '' },
		features: { status: true, leaderboards }
	}) as unknown as PublicServer;

describe('publicStatus roster', () => {
	test('carries the SteamID as a link only while the career pages are open', () => {
		expect(publicStatus(server(true), live, null).roster[0].steamId).toBe('76561198100000105');
		expect(publicStatus(server(false), live, null).roster[0].steamId).toBeNull();
	});
});
