import { describe, expect, test } from 'bun:test';
import { ACTIONS, readPlayersForObservation } from './actions';
import type { WardogsClient } from './rcon';

describe('raw training observations', () => {
	test('raw players preserve missing counters and future fields while UI normalization stays compatible', async () => {
		const raw = {
			players: [
				{ steamId: '76561198000000001', name: 'Player', customWeapon: 'AK74M', health: null }
			],
			count: 1
		};
		const client = { json: async () => raw } as unknown as WardogsClient;
		const result = await readPlayersForObservation(client);
		expect(result.raw).toEqual(raw);
		expect('kills' in result.raw.players[0]).toBe(false);
		expect(result.players[0].kills).toBe(0);
		expect((await ACTIONS.players.run(client, {})) as any).not.toHaveProperty('raw');
		expect((await ACTIONS.players.run(client, { raw: true })) as any).not.toHaveProperty('raw');
	});
	test('raw status preserves source scores and unknown map-clock fields', async () => {
		const raw = { map: 'Ozeti', customClock: 0.125, factionScores: [{ name: 'A', score: 7.25 }] };
		const result = (await ACTIONS.status.run(
			{ json: async () => raw } as unknown as WardogsClient,
			{ raw: true }
		)) as any;
		expect(result.raw).toEqual(raw);
		expect(result.scores[0].score).toBe(7.25);
	});
});
