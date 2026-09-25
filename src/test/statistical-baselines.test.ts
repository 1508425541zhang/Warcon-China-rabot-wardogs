import { describe, expect, test } from 'bun:test';
import { sql } from 'drizzle-orm';
import { integrityBaselines } from '$lib/server/db/schema';
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
		await env.db.execute(sql`
			INSERT INTO samples (ts,server_id,ok,player_count,map)
			SELECT ${base}::timestamptz + slot * interval '3 minutes', ${world.server.id}, true,
			       CASE WHEN slot < 200 THEN 15 ELSE 70 END, 'Kavkazi'
			FROM generate_series(0,399) slot`);
		await env.db.execute(sql`
			INSERT INTO kills (ts,server_id,event_id,instance_id,match_id,event_time,map,
				killer_steam_id,killer_name,killer_faction,victim_steam_id,victim_name,victim_faction,cause,headshot,team_kill,suicide,tags)
			SELECT ${base}::timestamptz + slot * interval '3 minutes' + n * interval '1 second',
			       ${world.server.id}, 'base-' || slot || '-' || n, 'stat-test', 'round',
			       slot * 180 + n, 'Kavkazi', '76561198000000888', 'Test', 'Blue',
			       '7656119800000' || lpad(n::text,4,'0'), 'Victim', 'Red', 'Id.Item.AK74M',
			       (n % 2 = 0), false, false, '[]'::jsonb
			FROM generate_series(0,399) slot
			CROSS JOIN LATERAL generate_series(1, CASE WHEN slot < 200 THEN 3 + slot % 7 ELSE 9 + slot % 7 END) n`);
		expect(await refreshIntegrityBaselines(env, world.org.id)).toBeGreaterThan(0);
		const rows = await env.db.select().from(integrityBaselines);
		const own = rows.filter((row) => row.orgId === world.org.id);
		const low = selectBaselines(own, 'Kavkazi', '1–20').get('kpm180')!;
		const high = selectBaselines(own, 'Kavkazi', '61–80').get('kpm180')!;
		expect(low.sampleCount).toBe(200);
		expect(high.sampleCount).toBe(200);
		expect(low.p99).toBeLessThan(high.p99);
		expect(percentilePosition(low.cdf, 3.5)).toBeGreaterThan(percentilePosition(high.cdf, 3.5));
		const limited = own.map((row) =>
			row.metric === 'kpm180' && row.level === 1 ? { ...row, sampleCount: 50 } : row
		);
		expect(selectBaselines(limited, 'Kavkazi', '1–20').get('kpm180')?.map).toBeNull();
		expect(selectBaselines(limited, 'Kavkazi', null).get('kpm180')?.populationBucket).toBeNull();
	});
});
