import { describe, expect, test } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { kills, matches } from '../lib/server/db/schema';
import { loadRoundPrecision } from '../lib/server/integrity/precision-round-data';
describe.skipIf(!hasTestDb)('round precision database', () => {
	test('whole round, peer population, duplicate and future exclusion', async () => {
		const env = await testEnv();
		const w = await seedWorld(env);
		const now = new Date();
		const [m] = await env.db
			.insert(matches)
			.values({ serverId: w.server.id, map: 'test', startedAt: new Date(now.getTime() - 3600000) })
			.returning();
		const make = (id: string, killer: string, clock: number, headshot: boolean) => ({
			serverId: w.server.id,
			eventId: id,
			instanceId: 'boot',
			matchId: 'match',
			matchRow: m.id,
			eventTime: clock,
			map: 'test',
			killerSteamId: killer,
			victimSteamId: 'victim',
			victimName: 'victim',
			cause: 'Id.Item.AK74M',
			tags: [],
			headshot,
			ts: new Date(now.getTime() - 1000)
		});
		await env.db
			.insert(kills)
			.values([
				...Array.from({ length: 5 }, (_, i) => make(`own-${i}`, 'a', i * 100, true)),
				make('own-0', 'a', 0, true),
				make('peer', 'b', 100, false),
				make('future', 'a', 900, false),
				{ ...make('team', 'a', 200, true), teamKill: true },
				{ ...make('wrong-round', 'a', 200, true), matchRow: null },
				{ ...make('melee', 'a', 200, true), tags: ['WeaponMelee'] },
				{ ...make('future-receipt', 'a', 200, true), ts: new Date(now.getTime() + 1000) }
			]);
		const [r] = await loadRoundPrecision(
			env.db,
			{
				serverId: w.server.id,
				steamId: 'a',
				instanceId: 'boot',
				matchRow: m.id,
				clock: 500,
				at: now
			},
			new Map()
		);
		expect(r.kills).toBe(5);
		expect(r.headshots).toBe(5);
		expect(r.peerKills).toBe(1);
		expect(r.serverRate).toBe(0);
	});
});
