import { describe, expect, test } from 'bun:test';
import { validateWebhookUrl } from './webhooks';
import {
	buildEmbed,
	buildTeamKillEmbed,
	buildWatchedJoinEmbed,
	classify,
	dossierUrl
} from './webhook-delivery';

const token = 'a'.repeat(68);

describe('validateWebhookUrl', () => {
	test('accepts Discord webhook URLs and hides the token in the hint', () => {
		const { url, hint } = validateWebhookUrl(
			`https://DISCORD.com/api/webhooks/123456789012345678/${token}?wait=true`
		);
		expect(url).toBe(`https://discord.com/api/webhooks/123456789012345678/${token}`);
		expect(hint).toBe('discord.com/api/webhooks/123456789012345678/…');
		expect(
			validateWebhookUrl(`https://ptb.discord.com/api/webhooks/123456789012345678/${token}`).url
		).toContain('ptb.discord.com');
	});
	test('refuses anything that is not a Discord webhook', () => {
		expect(() => validateWebhookUrl('not a url')).toThrow('not a URL');
		expect(() =>
			validateWebhookUrl(`http://discord.com/api/webhooks/123456789012345678/${token}`)
		).toThrow('Discord webhook URL');
		expect(() =>
			validateWebhookUrl(`https://example.com/api/webhooks/123456789012345678/${token}`)
		).toThrow('Discord webhook URL');
		expect(() => validateWebhookUrl('https://discord.com/api/channels/1/messages')).toThrow(
			'Discord webhook URL'
		);
		expect(() =>
			validateWebhookUrl('https://discord.com/api/webhooks/123456789012345678/short')
		).toThrow('Discord webhook URL');
	});
});

describe('classify', () => {
	test('routes rows to event classes', () => {
		expect(classify({ category: 'rcon', action: 'rcon.ban' })).toBe('bans');
		expect(classify({ category: 'rcon', action: 'rcon.kick' })).toBe('commands');
		expect(classify({ category: 'trigger', action: 'trigger.welcome' })).toBe('triggers');
		expect(classify({ category: 'player', action: 'player.note' })).toBe('players');
		expect(classify({ category: 'org', action: 'org.invite.create' })).toBe('management');
		expect(classify({ category: 'auth', action: 'login' })).toBe('auth');
		expect(classify({ category: 'weird', action: 'x' })).toBeNull();
	});
});

describe('buildEmbed', () => {
	const row = {
		id: 1,
		ts: new Date('2026-09-09T12:00:00Z'),
		actorId: 'u1',
		actorName: 'james',
		serverId: 's1',
		serverName: 'EU #1',
		orgId: 'o1',
		category: 'rcon',
		action: 'rcon.kick',
		target: '76561198000000001',
		detail: { reason: 'tk', password: 'x' },
		outcome: 'ok' as const,
		status: 200,
		message: 'Kicked Nomad.',
		userAgent: 'curl',
		durationMs: 12
	};
	test('names the action, actor, target and server', () => {
		const e = buildEmbed('Warcon', row);
		expect(e.title).toBe('Kick');
		expect(e.description).toContain('**james** → `76561198000000001`');
		expect(e.description).toContain('Server: EU #1');
		expect(e.description).toContain('Kicked Nomad.');
		expect(e.color).toBe(0x7bc462);
		expect(e.timestamp).toBe('2026-09-09T12:00:00.000Z');
	});
	test('failures are red and say so', () => {
		const e = buildEmbed('Warcon', {
			...row,
			outcome: 'denied',
			status: 403,
			message: 'needs admin'
		});
		expect(e.description).toContain('Outcome: **denied** (403)');
		expect(e.color).toBe(0x8a8a90);
	});
	test('a server added, refused or deleted never says where RCON listens', () => {
		for (const action of ['server.create', 'server.update', 'server.delete']) {
			const e = buildEmbed('Warcon', {
				...row,
				category: 'server',
				action,
				outcome: 'denied',
				status: 400,
				target: 'rcon.example.net:7776',
				message: 'rcon.example.net resolves to 10.0.0.5, a private address.'
			});
			expect(e.description).toContain('**james**');
			expect(e.description).toContain('Server: EU #1');
			expect(e.description).not.toContain('example.net');
			expect(e.description).not.toContain('7776');
			expect(e.description).not.toContain('10.0.0.5');
		}
	});
});

describe('buildTeamKillEmbed', () => {
	test('names both players, the weapon, the distance and the server', () => {
		const e = buildTeamKillEmbed('Warcon', 'TLR #1', {
			eventId: 'x',
			ts: '2026-09-16T20:00:00.000Z',
			map: 'Kavkazi',
			eventTime: 12,
			killer: { steamId: '76561198000000001', name: 'Alpha', faction: 'Valkyra' },
			victim: { steamId: '76561198000000002', name: 'Bravo', faction: 'Valkyra' },
			cause: 'Id.Item.AK74M',
			distanceM: 39.6,
			headshot: true,
			suicide: false,
			teamKill: true,
			tags: []
		});
		expect(e.title).toBe('Team kill');
		expect(e.description).toBe(
			'**Alpha** → **Bravo** (Valkyra)\nAK-74M · 40 m · headshot\nServer: TLR #1 · Kavkazi'
		);
		expect(e.timestamp).toBe('2026-09-16T20:00:00.000Z');
		expect(e.footer?.text).toBe('Warcon');
	});
});

describe('watched join and dossier links', () => {
	test("a player's page, only with an origin and a SteamID", () => {
		expect(dossierUrl('https://rcon.example.com/', 's 1', '76561198000000001')).toBe(
			'https://rcon.example.com/server/s%201/players/76561198000000001'
		);
		expect(dossierUrl('', 's1', '76561198000000001')).toBeUndefined();
		expect(dossierUrl('https://rcon.example.com', 's1', 'not-an-id')).toBeUndefined();
	});
	test('the post names the player, why, and where, and opens their page', () => {
		const e = buildWatchedJoinEmbed(
			'Warcon',
			'TLR #1',
			{ steamId: '76561198000000001', name: 'Krieger', reason: 'alt of a banned player' },
			'2026-09-22T00:00:00.000Z',
			'https://rcon.example.com/server/s1/players/76561198000000001'
		);
		expect(e.title).toBe('Watched player joined');
		expect(e.url).toBe('https://rcon.example.com/server/s1/players/76561198000000001');
		expect(e.description).toContain('Krieger');
		expect(e.description).toContain('alt of a banned player');
		expect(e.description).toContain('TLR #1');
		expect(
			buildWatchedJoinEmbed('Warcon', 'x', { steamId: '1', name: 'a', reason: '' }, 't').url
		).toBeUndefined();
	});
});
