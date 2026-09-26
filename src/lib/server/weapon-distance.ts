import { sql } from 'drizzle-orm';
import type { Env } from './env';
import { MAX_KILL_DISTANCE_M } from './feed-core';

export interface WeaponDistanceRow {
	steamId: string;
	cause: string;
	samples: number;
	average: number;
	maximum: number;
	serverAverage: number;
	eligiblePlayers: number;
	rank: number | null;
}
// Server-wide aggregation is shared by dossier refreshes, bounded to 32 servers / 60 seconds.
const cache = new Map<string, { until: number; data: Promise<WeaponDistanceRow[]> }>();
export async function loadWeaponDistances(env: Env, serverId: string, steamId: string) {
	const now = new Date();
	let hit = cache.get(serverId);
	if (!hit || hit.until <= now.getTime()) {
		const data = queryWeaponDistances(env, serverId, now);
		hit = { until: now.getTime() + 60000, data };
		cache.delete(serverId);
		cache.set(serverId, hit);
		if (cache.size > 32) cache.delete(cache.keys().next().value!);
		data.catch(() => {
			if (cache.get(serverId)?.data === data) cache.delete(serverId);
		});
	}
	const rows = await hit.data;
	return {
		days: 30,
		minimumSamples: 10,
		refreshedAt: new Date(hit.until - 60000).toISOString(),
		rows: rows
			.filter((row) => row.steamId === steamId)
			.sort((a, b) => b.samples - a.samples || a.cause.localeCompare(b.cause))
	};
}

/** Descriptive UI data only: not consumed by scoring, baselines, committee or enforcement. */
export async function queryWeaponDistances(
	env: Env,
	serverId: string,
	now: Date
): Promise<WeaponDistanceRow[]> {
	const rows = await env.db.execute(sql`
 WITH grouped AS (
 SELECT killer_steam_id AS player, cause, COUNT(*) AS samples,
 AVG(distance_m::double precision) AS average, MAX(distance_m) AS maximum
 FROM kills
 WHERE server_id = ${serverId} AND ts >= ${new Date(now.getTime() - 30 * 86400000)} AND ts <= ${now}
 AND killer_steam_id ~ '^[0-9]{17}$' AND killer_steam_id <> victim_steam_id
 AND NOT suicide AND NOT team_kill AND NOT distance_invalid
 AND distance_m > 0 AND distance_m < ${MAX_KILL_DISTANCE_M}
 AND cause LIKE 'Id.Item.%'
 GROUP BY killer_steam_id, cause
 ), ranked AS (
 SELECT player, cause, RANK() OVER (PARTITION BY cause ORDER BY average DESC) AS rank
 FROM grouped WHERE samples >= 10
 ), totals AS (
 SELECT cause, SUM(average*samples)/SUM(samples) AS server_average,
 COUNT(*) FILTER (WHERE samples >= 10) AS eligible_players FROM grouped GROUP BY cause
 )
 SELECT g.player AS "steamId", g.cause, g.samples, g.average, g.maximum,
 t.server_average AS "serverAverage", t.eligible_players AS "eligiblePlayers", r.rank
 FROM grouped g JOIN totals t USING(cause) LEFT JOIN ranked r ON r.player=g.player AND r.cause=g.cause
 `);
	return (rows as unknown as WeaponDistanceRow[]).map((row) => ({
		...row,
		samples: Number(row.samples),
		average: Number(row.average),
		maximum: Number(row.maximum),
		serverAverage: Number(row.serverAverage),
		eligiblePlayers: Number(row.eligiblePlayers),
		rank: row.rank == null ? null : Number(row.rank)
	}));
}
