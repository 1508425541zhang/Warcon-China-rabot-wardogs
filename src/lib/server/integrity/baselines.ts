import { and, eq, gte, or, sql } from 'drizzle-orm';
import type { Env } from '../env';
import { integrityBaselines, organizations, samples } from '../db/schema';
import { DEFAULT_WEAPON_MAP } from './weapons';
import {
	METRICS,
	populationBucket,
	type DistributionStats,
	type MetricCode,
	type PopulationBucket
} from './statistics';

const WINDOW_DAYS = 30;
const HISTOGRAM_BINS = 30;
const causes = sql.join(
	Object.keys(DEFAULT_WEAPON_MAP).map((cause) => sql`${cause}`),
	sql`, `
);
let timer: ReturnType<typeof setInterval> | null = null;
let initial: ReturnType<typeof setTimeout> | null = null;

/** One non-overlapping 180-second player/round sample; no abnormal-window selection bias. */
export async function refreshIntegrityBaselines(env: Env, orgId: string): Promise<number> {
	const calculatedAt = new Date();
	const rows = await env.db.execute(sql`
		WITH valid AS (
			SELECT k.server_id, k.instance_id, k.match_id, k.killer_steam_id,
			       k.map, k.event_time, k.victim_steam_id, k.headshot,
			       (k.tags ? 'Penetration') AS penetration,
			       floor(k.event_time / 180)::integer AS slot,
			       CASE WHEN pop.player_count BETWEEN 1 AND 20 THEN '1–20'
			            WHEN pop.player_count BETWEEN 21 AND 40 THEN '21–40'
			            WHEN pop.player_count BETWEEN 41 AND 60 THEN '41–60'
			            WHEN pop.player_count BETWEEN 61 AND 80 THEN '61–80'
			            WHEN pop.player_count >= 81 THEN '81+' END AS bucket
			FROM kills k JOIN servers s ON s.id = k.server_id
			LEFT JOIN integrity_weapon_map wm ON wm.org_id = s.org_id AND wm.cause = k.cause
			LEFT JOIN LATERAL (
				SELECT player_count FROM samples sm
				WHERE sm.server_id = k.server_id AND sm.ok AND sm.ts <= k.ts
				  AND sm.ts > k.ts - interval '2 minutes'
				ORDER BY sm.ts DESC LIMIT 1
			) pop ON true
			WHERE s.org_id = ${orgId} AND k.ts >= ${new Date(calculatedAt.getTime() - WINDOW_DAYS * 86_400_000)}
			  AND k.ts < ${new Date(calculatedAt.getTime() - 10 * 60_000)}
			  AND k.killer_steam_id IS NOT NULL AND k.killer_steam_id <> k.victim_steam_id
			  AND k.killer_faction IS NOT NULL AND k.victim_faction IS NOT NULL
			  AND k.killer_faction <> k.victim_faction AND NOT k.team_kill AND NOT k.suicide
			  AND NOT (k.tags ?| ARRAY['Suicide','Falling','RoadKill','VehicleExplosion'])
			  AND coalesce(wm.category, CASE WHEN k.cause IN (${causes}) THEN 'INFANTRY' ELSE 'UNKNOWN' END) = 'INFANTRY'
		), timed AS (
			SELECT *, lag(event_time) OVER w AS previous_clock,
			       count(*) OVER (PARTITION BY server_id, instance_id, match_id, killer_steam_id, slot
			                      ORDER BY event_time RANGE BETWEEN 15 PRECEDING AND CURRENT ROW) AS burst15
			FROM valid WINDOW w AS (PARTITION BY server_id, instance_id, match_id, killer_steam_id, slot
			                        ORDER BY event_time)
		), windows AS (
			SELECT map, bucket, server_id, instance_id, match_id, killer_steam_id, slot,
			       count(*)::double precision AS infantry_kills,
			       count(*)::double precision / 3 AS kpm180,
			       count(DISTINCT victim_steam_id)::double precision AS unique_victims,
			       max(burst15)::double precision AS max_kills_15s,
			       percentile_cont(0.5) WITHIN GROUP (ORDER BY event_time - previous_clock)
			           FILTER (WHERE previous_clock IS NOT NULL) AS median_kill_interval,
			       count(*) FILTER (WHERE headshot)::double precision / count(*) AS headshot_rate,
			       count(*) FILTER (WHERE penetration)::double precision / count(*) AS penetration_rate
			FROM timed GROUP BY map, bucket, server_id, instance_id, match_id, killer_steam_id, slot
		), metric_values AS (
			SELECT w.map, w.bucket, m.metric, m.value
			FROM windows w CROSS JOIN LATERAL (VALUES
				('kpm180', w.kpm180), ('uniqueVictims', w.unique_victims),
				('maxKills15s', w.max_kills_15s), ('medianKillInterval', w.median_kill_interval),
				('headshotRate', CASE WHEN w.infantry_kills >= 10 THEN w.headshot_rate END),
				('penetrationRate', CASE WHEN w.infantry_kills >= 10 THEN w.penetration_rate END)
			) m(metric, value) WHERE m.value IS NOT NULL
		), expanded AS (
			SELECT 1 AS level, map, bucket, metric, value FROM metric_values WHERE bucket IS NOT NULL
			UNION ALL SELECT 2, NULL::text, bucket, metric, value FROM metric_values WHERE bucket IS NOT NULL
			UNION ALL SELECT 3, NULL::text, NULL::text, metric, value FROM metric_values
		), keyed AS (
			SELECT md5(jsonb_build_array(${orgId}::text, level, map, bucket, metric, 'INFANTRY')::text) AS id,
			       level, map, bucket, metric, value FROM expanded
		), medians AS (
			SELECT id, min(level) AS level, min(map) AS map, min(bucket) AS bucket, min(metric) AS metric,
			       count(*)::integer AS sample_count, min(value) AS minimum, max(value) AS maximum,
			       percentile_cont(ARRAY[0.5,0.9,0.95,0.99,0.995,0.999,0.9995])
			           WITHIN GROUP (ORDER BY value) AS quantiles
			FROM keyed GROUP BY id
		), deviations AS (
			SELECT k.id, percentile_cont(0.5) WITHIN GROUP (ORDER BY abs(k.value - m.quantiles[1])) AS mad
			FROM keyed k JOIN medians m USING (id) GROUP BY k.id
		), frequencies AS (
			SELECT id, value, count(*)::integer AS n FROM keyed GROUP BY id, value
		), cdfs AS (
			SELECT id, jsonb_agg(jsonb_build_array(value,n) ORDER BY value) AS cdf
			FROM frequencies GROUP BY id
		), bin_counts AS (
			SELECT f.id, CASE WHEN m.maximum = m.minimum THEN 0 ELSE
			       least(${HISTOGRAM_BINS - 1}, greatest(0, width_bucket(f.value, m.minimum, m.maximum, ${HISTOGRAM_BINS}) - 1)) END AS bin,
			       sum(f.n)::integer AS n
			FROM frequencies f JOIN medians m USING (id) GROUP BY f.id, bin
		), histograms AS (
			SELECT m.id, jsonb_agg(jsonb_build_object(
				'from', m.minimum + (m.maximum - m.minimum) * b.i / ${HISTOGRAM_BINS},
				'to', m.minimum + (m.maximum - m.minimum) * (b.i + 1) / ${HISTOGRAM_BINS},
				'count', coalesce(bc.n,0)) ORDER BY b.i) AS histogram
			FROM medians m CROSS JOIN generate_series(0, ${HISTOGRAM_BINS - 1}) b(i)
			LEFT JOIN bin_counts bc ON bc.id = m.id AND bc.bin = b.i
			GROUP BY m.id
		)
		SELECT m.*, d.mad, c.cdf, h.histogram FROM medians m
		JOIN deviations d USING (id) JOIN cdfs c USING (id) JOIN histograms h USING (id)
	`);
	await env.db.transaction(async (tx) => {
		await tx.delete(integrityBaselines).where(eq(integrityBaselines.orgId, orgId));
		if (rows.length)
			await tx.insert(integrityBaselines).values(
				rows.map((row) => {
					const q = row.quantiles as number[];
					return {
						id: String(row.id),
						orgId,
						metric: String(row.metric),
						level: Number(row.level),
						map: row.map === null ? null : String(row.map),
						populationBucket: row.bucket === null ? null : String(row.bucket),
						weaponCategory: 'INFANTRY',
						sampleCount: Number(row.sample_count),
						median: q[0],
						mad: Number(row.mad),
						p90: q[1],
						p95: q[2],
						p99: q[3],
						p995: q[4],
						p999: q[5],
						p9995: q[6],
						histogram: row.histogram,
						cdf: row.cdf,
						windowDays: WINDOW_DAYS,
						calculatedAt
					};
				})
			);
	});
	return rows.length;
}

export async function populationAt(
	env: Env,
	serverId: string,
	at: Date
): Promise<PopulationBucket | null> {
	const [sample] = await env.db
		.select({ playerCount: samples.playerCount })
		.from(samples)
		.where(
			sql`${samples.serverId} = ${serverId} AND ${samples.ok} AND ${samples.ts} <= ${at}
			AND ${samples.ts} > ${new Date(at.getTime() - 120_000)}`
		)
		.orderBy(sql`${samples.ts} DESC`)
		.limit(1);
	return populationBucket(sample?.playerCount ?? null);
}

export function selectBaselines(
	rows: readonly (typeof integrityBaselines.$inferSelect)[],
	map: string,
	bucket: PopulationBucket | null,
	now = new Date()
): Map<MetricCode, DistributionStats> {
	const selected = new Map<MetricCode, DistributionStats>();
	for (const metric of Object.keys(METRICS) as MetricCode[]) {
		const row = rows
			.filter(
				(candidate) =>
					candidate.metric === metric &&
					candidate.sampleCount >= 200 &&
					candidate.windowDays === WINDOW_DAYS &&
					now.getTime() - candidate.calculatedAt.getTime() <= 24 * 60 * 60_000
			)
			.sort((a, b) => a.level - b.level)
			.find((candidate) =>
				candidate.level === 1
					? candidate.map === map && candidate.populationBucket === bucket && bucket !== null
					: candidate.level === 2
						? candidate.populationBucket === bucket && bucket !== null
						: candidate.level === 3
			);
		if (row)
			selected.set(metric, {
				id: row.id,
				metric,
				map: row.map,
				populationBucket: row.populationBucket as PopulationBucket | null,
				weaponCategory: row.weaponCategory,
				sampleCount: row.sampleCount,
				median: row.median,
				mad: row.mad,
				p90: row.p90,
				p95: row.p95,
				p99: row.p99,
				p995: row.p995,
				p999: row.p999,
				p9995: row.p9995,
				histogram: row.histogram as DistributionStats['histogram'],
				cdf: row.cdf as DistributionStats['cdf'],
				windowDays: row.windowDays,
				calculatedAt: row.calculatedAt
			});
	}
	return selected;
}

export async function loadBaselines(
	env: Env,
	orgId: string,
	map: string,
	bucket: PopulationBucket | null
) {
	const scope = bucket
		? or(
				eq(integrityBaselines.level, 3),
				and(eq(integrityBaselines.level, 2), eq(integrityBaselines.populationBucket, bucket)),
				and(
					eq(integrityBaselines.level, 1),
					eq(integrityBaselines.populationBucket, bucket),
					eq(integrityBaselines.map, map)
				)
			)
		: eq(integrityBaselines.level, 3);
	return selectBaselines(
		await env.db
			.select()
			.from(integrityBaselines)
			.where(
				and(
					eq(integrityBaselines.orgId, orgId),
					gte(integrityBaselines.sampleCount, 200),
					gte(integrityBaselines.calculatedAt, new Date(Date.now() - 24 * 60 * 60_000)),
					scope
				)
			),
		map,
		bucket
	);
}

export async function shadowComparison(env: Env, orgId: string, koThreshold: number) {
	const [row] = await env.db.execute(sql`
		WITH latest AS (
			SELECT DISTINCT ON (window_id) score, scored_at, statistical
			FROM integrity_scores
			WHERE org_id = ${orgId} AND source = 'window' AND statistical IS NOT NULL
			  AND scored_at >= now() - interval '30 days'
			ORDER BY window_id, id DESC
		)
		SELECT min(scored_at) AS since, count(*)::int AS total,
		count(*) FILTER (WHERE score < ${koThreshold} AND statistical->>'level' = 'NORMAL')::int AS normal_normal,
		count(*) FILTER (WHERE score < ${koThreshold} AND statistical->>'level' IN ('WATCH','CASE','KICK_CANDIDATE'))::int AS normal_abnormal,
		count(*) FILTER (WHERE score >= ${koThreshold} AND statistical->>'level' = 'NORMAL')::int AS abnormal_normal,
		count(*) FILTER (WHERE score >= ${koThreshold} AND statistical->>'level' IN ('WATCH','CASE','KICK_CANDIDATE'))::int AS abnormal_abnormal
		FROM latest WHERE statistical->>'status' = 'READY'
	`);
	return {
		since: row?.since ? new Date(row.since as string | Date).toISOString() : null,
		total: Number(row?.total ?? 0),
		normalNormal: Number(row?.normal_normal ?? 0),
		normalAbnormal: Number(row?.normal_abnormal ?? 0),
		abnormalNormal: Number(row?.abnormal_normal ?? 0),
		abnormalAbnormal: Number(row?.abnormal_abnormal ?? 0)
	};
}

export function startIntegrityBaselines(env: Env): void {
	const run = async () => {
		const orgs = await env.db.select({ id: organizations.id }).from(organizations);
		for (const org of orgs) {
			try {
				await refreshIntegrityBaselines(env, org.id);
			} catch (error) {
				console.error(`[warcon] Integrity baseline ${org.id}:`, error);
			}
		}
	};
	timer = setInterval(
		() => void run().catch((error) => console.error('[warcon] Integrity baselines:', error)),
		60 * 60_000
	);
	initial = setTimeout(
		() => void run().catch((error) => console.error('[warcon] Integrity baselines:', error)),
		10_000
	);
}

export function stopIntegrityBaselines(): void {
	if (timer) clearInterval(timer);
	if (initial) clearTimeout(initial);
	timer = null;
	initial = null;
}
