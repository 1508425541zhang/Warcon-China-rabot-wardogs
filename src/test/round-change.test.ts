import { describe, test, expect } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { kills, matches, playerSessions } from '$lib/server/db/schema';
import { loadRoundChangeSeries } from '$lib/server/integrity/round-change';
describe.skipIf(!hasTestDb)('same round all-weapon change series', () => {
	test('guns and vehicles count; other rounds, suicides and known teamkills do not', async () => {
		const env = await testEnv(),
			world = await seedWorld(env),
			at = new Date(),
			steamId = '76561198000000001';
		const [round, old] = await env.db
			.insert(matches)
			.values([
				{ serverId: world.server.id, map: 'A', startedAt: new Date(at.getTime() - 360000) },
				{
					serverId: world.server.id,
					map: 'B',
					startedAt: new Date(at.getTime() - 900000),
					endedAt: new Date(at.getTime() - 400000)
				}
			])
			.returning();
		await env.db
			.insert(playerSessions)
			.values({
				serverId: world.server.id,
				steamId,
				name: 'test',
				joinedAt: new Date(at.getTime() - 360000),
				lastSeen: at
			});
		await env.db
			.insert(kills)
			.values(
				['gun', 'vehicle', 'grenade', 'unknown', 'suicide', 'team', 'old'].map((cause, i) => ({
					serverId: world.server.id,
					matchRow: i === 6 ? old.id : round.id,
					instanceId: 'i',
					matchId: 'm',
					ts: at,
					eventId: crypto.randomUUID(),
					eventTime: 10 + i * 20,
					map: 'A',
					killerSteamId: steamId,
					victimSteamId: '76561198000000002',
					victimName: 'test',
					cause,
					suicide: i === 4,
					teamKill: i === 5,
					tags: []
				}))
			);
		const result = await loadRoundChangeSeries(env.db, {
			serverId: world.server.id,
			steamId,
			instanceId: 'i',
			matchRow: round.id,
			clock: 360,
			at
		});
		expect(result).toHaveLength(24);
		expect(result.reduce((a, b) => a + b, 0)).toBe(16);
	});
});
