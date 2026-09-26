import { beforeAll, describe, expect, test } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { kills } from '$lib/server/db/schema';
import { queryWeaponDistances } from '$lib/server/weapon-distance';
describe.skipIf(!hasTestDb)('descriptive weapon distance ranks', () => {
	test('ten-kill boundary, ties, weapon/server isolation and invalid distances', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const now = new Date();
		const a = '76561198000100001',
			b = '76561198000100002',
			c = '76561198000100003';
		let index = 0;
		const make = (id: string, distance: number, cause = 'Id.Item.AK74M') => ({
			serverId: world.server.id,
			ts: now,
			eventId: `distance-${index++}`,
			instanceId: 'distance',
			matchId: 'one',
			eventTime: 30,
			map: 'Europe',
			killerSteamId: id,
			victimSteamId: '76561198000100009',
			victimName: 'target',
			cause,
			distanceM: distance,
			tags: []
		});
		const rows = [
			...Array.from({ length: 10 }, () => make(a, 100)),
			...Array.from({ length: 10 }, () => make(b, 100)),
			...Array.from({ length: 9 }, () => make(c, 500)),
			...Array.from({ length: 10 }, () => make(a, 200, 'Id.Item.M4')),
			{ ...make(c, 900), distanceInvalid: true },
			{ ...make(c, 900), suicide: true },
			{ ...make(c, 900), teamKill: true },
			make(c, 5000),
			make(c, 0),
			{ ...make(c, 900), ts: new Date(now.getTime() - 31 * 86400000) },
			{ ...make(c, 900), serverId: 'other-server' },
			{ ...make(c, 900), victimSteamId: c }
		];
		await env.db.insert(kills).values(rows);
		const actual = await queryWeaponDistances(env, world.server.id, now);
		const ar = actual.find((r) => r.steamId === a && r.cause === 'Id.Item.AK74M')!;
		expect(ar.samples).toBe(10);
		expect(ar.rank).toBe(1);
		expect(ar.eligiblePlayers).toBe(2);
		expect(ar.serverAverage).toBeCloseTo(6500 / 29);
		expect(actual.find((r) => r.steamId === b)?.rank).toBe(1);
		expect(actual.find((r) => r.steamId === c)?.rank).toBeNull();
		expect(actual.find((r) => r.steamId === c)?.samples).toBe(9);
		expect(actual.find((r) => r.cause === 'Id.Item.M4')?.average).toBe(200);
	});
});
