import { describe, expect, test } from 'bun:test';
import { matches, matchPlayers } from '$lib/server/db/schema';
import { loadMatchRetention } from '$lib/server/match-retention';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('adjacent match retention', () => {
	test('counts intersections, excludes newcomers, keeps missing rounds and range predecessor', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const rounds = await env.db
			.insert(matches)
			.values(
				[0, 1, 2, 3].map((i) => ({
					serverId: world.server.id,
					map: 'SameMap',
					startedAt: new Date(1_700_000_000_000 + i * 3600_000),
					endedAt: i === 3 ? null : new Date(1_700_000_000_000 + (i + 1) * 3600_000)
				}))
			)
			.returning();
		await env.db.insert(matchPlayers).values([
			...[1, 2, 3].map((i) => ({
				matchId: rounds[0].id,
				serverId: world.server.id,
				steamId: `7656119800000000${i}`,
				name: 'Same name'
			})),
			...[2, 3, 4].map((i) => ({
				matchId: rounds[1].id,
				serverId: world.server.id,
				steamId: `7656119800000000${i}`,
				name: 'Changed name'
			})),
			{
				matchId: rounds[3].id,
				serverId: world.server.id,
				steamId: '76561198000000002',
				name: 'Returned'
			}
		]);
		const rows = await loadMatchRetention(env, world.server.id, rounds[1].startedAt);
		expect(rows).toHaveLength(3);
		expect(rows[0]).toMatchObject({
			fromId: rounds[0].id,
			toId: rounds[1].id,
			total: 3,
			retained: 2,
			lost: 1,
			percent: 66.7,
			provisional: false
		});
		expect(rows[1]).toMatchObject({ total: 3, retained: null, lost: null, percent: null });
		expect(rows[2]).toMatchObject({
			fromId: rounds[2].id,
			total: 0,
			percent: null,
			provisional: true
		});
		expect(await loadMatchRetention(env, world.otherServer.id, rounds[0].startedAt)).toEqual([]);
	});
});
