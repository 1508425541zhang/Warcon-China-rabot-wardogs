import { describe, test, expect } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { kills, matches } from '$lib/server/db/schema';
import { loadSustainedKpm } from '$lib/server/integrity/sustained-kpm';
describe.skipIf(!hasTestDb)('persisted consecutive minute evidence', () => {
	test('counts reliable infantry once; excludes other rounds, vehicles, incomplete minutes and unreliable factions', async () => {
		const env = await testEnv(),
			w = await seedWorld(env),
			now = new Date();
		const [round] = await env.db
			.insert(matches)
			.values({ serverId: w.server.id, startedAt: new Date(now.getTime() - 240000) })
			.returning();
		const subject = '76561198000111111';
		const row = (id: string, t: number, extra: Partial<typeof kills.$inferInsert> = {}) => ({
			ts: now,
			serverId: w.server.id,
			instanceId: 'boot',
			matchId: 'match',
			matchRow: round.id,
			eventId: id,
			eventTime: t,
			map: 'Map',
			killerSteamId: subject,
			killerFaction: 'A',
			victimSteamId: '76561198000222222',
			victimName: 'victim',
			victimFaction: 'B',
			factionBracketed: true,
			factionObservedAt: now,
			cause: 'Id.Item.AK74M',
			tags: [],
			...extra
		});
		await env.db
			.insert(kills)
			.values([
				row('a', 1),
				row('b', 61),
				row('c', 121),
				row('a', 1, { ts: new Date(now.getTime() - 1) }),
				row('next', 181),
				row('vehicle', 2, { cause: 'Vehicle.Tank' }),
				row('unknown', 3, { factionBracketed: false }),
				row('other-round', 4, { matchRow: round.id + 1 })
			]);
		const input = {
			serverId: w.server.id,
			steamId: subject,
			instanceId: 'boot',
			roundId: `boot:match:${round.id}`,
			clock: 190,
			at: now,
			minutes: 3,
			threshold: 1
		};
		const s = await loadSustainedKpm(env.db, input, new Map());
		expect(s.windows.map((w) => w.kills)).toEqual([1, 1, 1]);
		expect(s.passed).toBe(true);
		expect((await loadSustainedKpm(env.db, { ...input, threshold: 2 }, new Map())).passed).toBe(
			false
		);
		expect(
			(await loadSustainedKpm(env.db, { ...input, roundId: 'boot:derived:1' }, new Map())).passed
		).toBe(false);
	});
});
