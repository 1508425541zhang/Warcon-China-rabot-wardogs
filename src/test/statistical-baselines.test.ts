import { describe, expect, test } from 'bun:test';
import { sql } from 'drizzle-orm';
import { feedProcessingJobs, integrityBaselines, kills, samples } from '$lib/server/db/schema';
import { refreshIntegrityBaselines, selectBaselines } from '$lib/server/integrity/baselines';
import { percentilePosition } from '$lib/server/integrity/statistics';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('PostgreSQL empirical baselines', () => {
	test('percentile_cont returns exact quantiles and MAD for 1..1000', async () => {
		const env = await testEnv();
		const [row] = await env.db.execute(sql`
			WITH values AS (SELECT n::double precision AS value FROM generate_series(1,1000) n),
			median AS (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY value) AS value FROM values)
			SELECT percentile_cont(ARRAY[0.5,0.95,0.99,0.999]) WITHIN GROUP (ORDER BY v.value) AS quantiles,
			       percentile_cont(0.5) WITHIN GROUP (ORDER BY abs(v.value - m.value)) AS mad
			FROM values v CROSS JOIN median m`);
		expect(row.quantiles).toEqual([500.5, 950.05, 990.01, 999.001]);
		expect(Number(row.mad)).toBe(250);
	});

	test('30-day kill history separates population groups and falls back only to sufficient layers', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const base = new Date(Date.now() - 3 * 86_400_000);
		for (const [group, playerCount, killsPerPlayer] of [
			[0, 15, 1],
			[1, 70, 2]
		] as const) {
			const at = new Date(base.getTime() + group * 3_600_000);
			await env.db.insert(samples).values({
				ts: new Date(at.getTime() - 60_000),
				serverId: world.server.id,
				ok: true,
				playerCount,
				map: 'Kavkazi'
			});
			const events = Array.from({ length: 200 }, (_, player) =>
				Array.from({ length: killsPerPlayer }, (_, n) => ({
					ts: at,
					serverId: world.server.id,
					eventId: `base-${group}-${player}-${n}`,
					instanceId: 'stat-test',
					matchId: 'round',
					matchRow: group + 1,
					eventTime: n + 1,
					map: 'Kavkazi',
					killerSteamId: String(76561198000000000n + BigInt(group * 200 + player)),
					killerName: 'Test',
					killerFaction: 'Blue',
					factionBracketed: true,
					factionObservedAt: at,
					victimSteamId: String(76561198100000000n + BigInt(group * 400 + player * 2 + n)),
					victimName: 'Victim',
					victimFaction: 'Red',
					cause: 'Id.Item.AK74M',
					headshot: false,
					teamKill: false,
					suicide: false,
					tags: []
				}))
			).flat();
			for (let i = 0; i < events.length; i += 200)
				await env.db.insert(kills).values(events.slice(i, i + 200));
			await env.db.insert(feedProcessingJobs).values({
				serverId: world.server.id,
				killTs: at,
				eventIds: events.map((event) => event.eventId),
				createdAt: at,
				doneAt: new Date(at.getTime() + 60_000),
				state: 'done',
				attempts: 1,
				consumer: 'integrity'
			});
		}
		expect(await refreshIntegrityBaselines(env, world.org.id)).toBeGreaterThan(0);
		const rows = await env.db.select().from(integrityBaselines);
		const own = rows.filter((row) => row.orgId === world.org.id);
		const low = selectBaselines(own, 'Kavkazi', '1–20').get('kpm180')!;
		const high = selectBaselines(own, 'Kavkazi', '61–80').get('kpm180')!;
		expect(low.sampleCount).toBe(200);
		expect(high.sampleCount).toBe(400);
		expect(low.p99).toBeLessThan(high.p99);
		expect(percentilePosition(low.cdf, 0.5)).toBeGreaterThan(percentilePosition(high.cdf, 0.5));
		const limited = own.map((row) =>
			row.metric === 'kpm180' && row.level === 1 ? { ...row, sampleCount: 50 } : row
		);
		expect(selectBaselines(limited, 'Kavkazi', '1–20').get('kpm180')?.map).toBeNull();
		expect(selectBaselines(limited, 'Kavkazi', null).get('kpm180')?.populationBucket).toBeNull();
	});
});
