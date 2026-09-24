// Hourly rollups of the samples table. The worker runs rollupSamples once an hour: it re-derives
// every complete hour since the last rollup (recomputing the newest one, whose trailing sample's
// cover was cut short last time) with the same duration weighting analytics.ts uses on raw rows,
// so the 30-day charts read the same numbers from far fewer rows. Nothing here deletes: raw
// samples and rollups are kept for good (on TimescaleDB, samples are compressed as they age).
import { sql } from 'drizzle-orm';
import type { Env } from './env';

/** A sample never covers more than this (matches analytics.ts). */
export const MAX_COVER_S = 600;

export async function rollupSamples(env: Env): Promise<void> {
	// One window for both statements: from the newest rollup (recomputed, since its trailing
	// sample's cover was cut short last time) or else the oldest raw sample, to the last complete
	// hour. Starting from the oldest sample means an upgrade rolls up everything it holds.
	const [b] = await env.db.execute<{ since: Date | null; until: Date }>(sql`
		SELECT COALESCE((SELECT MAX(bucket) - interval '1 hour' FROM sample_rollups), (SELECT MIN(ts) FROM samples)) AS since,
		       date_trunc('hour', now()) AS until`);
	if (!b || !b.since || new Date(b.since) >= new Date(b.until)) return;
	// The successor of the window's last sample lies beyond it, so the cover is computed over an
	// open-ended scan and each sample's cover is clipped to the window it is rolled into.
	const covered = sql`
		SELECT server_id, ts, ok, player_count, max_players, map,
		       LEAST(${MAX_COVER_S},
		             EXTRACT(EPOCH FROM (COALESCE(LEAD(ts) OVER (PARTITION BY server_id ORDER BY ts), now()) - ts)),
		             EXTRACT(EPOCH FROM (date_trunc('hour', ts) + interval '1 hour' - ts))) AS dur
		  FROM samples WHERE ts >= ${b.since}`;
	const inWindow = sql`ts < ${b.until}`;
	await env.db.transaction(async (tx) => {
		await tx.execute(sql`
		WITH s AS (${covered})
		INSERT INTO sample_rollups (server_id, bucket, samples, ok_samples, up_s, down_s, player_s, max_players, max_cap)
		SELECT server_id, date_trunc('hour', ts), COUNT(*), COUNT(*) FILTER (WHERE ok),
		       COALESCE(SUM(dur) FILTER (WHERE ok), 0), COALESCE(SUM(dur) FILTER (WHERE NOT ok), 0),
		       COALESCE(SUM(player_count * dur) FILTER (WHERE ok), 0),
		       MAX(player_count) FILTER (WHERE ok), MAX(max_players)
		  FROM s WHERE ${inWindow} GROUP BY server_id, date_trunc('hour', ts)
		ON CONFLICT (server_id, bucket) DO UPDATE SET samples = EXCLUDED.samples, ok_samples = EXCLUDED.ok_samples,
		       up_s = EXCLUDED.up_s, down_s = EXCLUDED.down_s, player_s = EXCLUDED.player_s,
		       max_players = EXCLUDED.max_players, max_cap = EXCLUDED.max_cap`);
		await tx.execute(sql`
		WITH s AS (${covered})
		INSERT INTO sample_map_rollups (server_id, bucket, map, secs)
		SELECT server_id, date_trunc('hour', ts), map, SUM(dur)
		  FROM s WHERE ${inWindow} AND ok AND map IS NOT NULL AND map <> ''
		 GROUP BY server_id, date_trunc('hour', ts), map
		ON CONFLICT (server_id, bucket, map) DO UPDATE SET secs = EXCLUDED.secs`);
	});
}
