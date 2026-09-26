import { and, desc, eq, lt, sql } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import { integrityPlayerCareers, integrityPlayerMetricHistory } from '../db/schema';
import { STATISTICAL_MODEL_CONFIG } from './statistical-config';

type HistoryRow = Pick<
	typeof integrityPlayerMetricHistory.$inferSelect,
	'roundId' | 'observedAt' | 'kpm180' | 'headshotRate' | 'maxKills15s'
>;

const median = (values: readonly number[]): number => {
	const sorted = [...values].sort((a, b) => a - b);
	const middle = Math.floor(sorted.length / 2);
	return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const distribution = (values: readonly number[]) => {
	if (!values.length) return null;
	const sorted = [...values].sort((a, b) => a - b);
	const quantile = (p: number) => {
		const at = (sorted.length - 1) * p;
		const low = Math.floor(at);
		return sorted[low] + (sorted[Math.ceil(at)] - sorted[low]) * (at - low);
	};
	const center = median(sorted);
	return {
		sampleCount: sorted.length,
		median: center,
		mad: median(sorted.map((value) => Math.abs(value - center))),
		p90: quantile(0.9),
		p95: quantile(0.95),
		p99: quantile(0.99)
	};
};

/** Select one clean reference window per round/180s; overlapping kill snapshots are not independent career observations. */
export function summarizeCleanCareer(rows: readonly HistoryRow[]) {
	const ordered = [...rows].sort((a, b) => a.observedAt.getTime() - b.observedAt.getTime());
	const lastByRound = new Map<string, number>();
	const independent = ordered.filter((row) => {
		const at = row.observedAt.getTime();
		const last = lastByRound.get(row.roundId);
		if (last !== undefined && at - last < 180_000) return false;
		lastByRound.set(row.roundId, at);
		return true;
	});
	if (!independent.length) return null;
	const kpm = independent.map((row) => row.kpm180);
	const kpmDistribution = distribution(kpm)!;
	const lastAt = independent.at(-1)!.observedAt.getTime();
	return {
		sampleCount: independent.length,
		uniqueDays: new Set(independent.map((row) => row.observedAt.toISOString().slice(0, 10))).size,
		firstSeenAt: independent[0].observedAt,
		lastSeenAt: independent.at(-1)!.observedAt,
		kpmMedian: kpmDistribution.median,
		kpmMad: kpmDistribution.mad,
		kpmDistribution,
		orderedKpm: kpm,
		lifetimeMatches: new Set(independent.map((row) => row.roundId)).size,
		headshotDistribution: distribution(
			independent.map((row) => row.headshotRate).filter((v): v is number => v !== null)
		),
		burstDistribution: distribution(independent.map((row) => row.maxKills15s)),
		recent7d: distribution(
			independent
				.filter((row) => row.observedAt.getTime() >= lastAt - 7 * 86_400_000)
				.map((row) => row.kpm180)
		),
		recent24h: distribution(
			independent
				.filter((row) => row.observedAt.getTime() >= lastAt - 86_400_000)
				.map((row) => row.kpm180)
		),
		recent30d: distribution(
			independent
				.filter((row) => row.observedAt.getTime() >= lastAt - 30 * 86_400_000)
				.map((row) => row.kpm180)
		)
	};
}

/** Only windows containing case evidence are removed; a case does not erase a player's clean career. */
export async function loadCleanCareerContext(
	db: DbOrTx,
	orgId: string,
	steamId: string,
	before: Date
) {
	const [existing] = await db
		.select()
		.from(integrityPlayerCareers)
		.where(
			and(eq(integrityPlayerCareers.orgId, orgId), eq(integrityPlayerCareers.steamId, steamId))
		)
		.limit(1);
	if (
		existing &&
		existing.status === 'READY' &&
		existing.modelVersion === STATISTICAL_MODEL_CONFIG.modelVersion &&
		existing.featureVersion === STATISTICAL_MODEL_CONFIG.featureVersion &&
		existing.lastSeenAt < before &&
		Date.now() - existing.updatedAt.getTime() < 24 * 3_600_000 &&
		Array.isArray(existing.orderedKpm) &&
		existing.orderedKpm.every((value) => typeof value === 'number' && Number.isFinite(value))
	) {
		const kpm = existing.kpmDistribution as { median?: unknown; mad?: unknown };
		if (typeof kpm.median === 'number' && typeof kpm.mad === 'number')
			return {
				sampleCount: existing.lifetimeWindows,
				uniqueDays: existing.activeDays,
				kpmMedian: kpm.median,
				kpmMad: kpm.mad,
				orderedKpm: existing.orderedKpm as number[]
			};
	}
	const rows = await db
		.select()
		.from(integrityPlayerMetricHistory)
		.where(
			and(
				eq(integrityPlayerMetricHistory.orgId, orgId),
				eq(integrityPlayerMetricHistory.steamId, steamId),
				eq(integrityPlayerMetricHistory.modelVersion, STATISTICAL_MODEL_CONFIG.modelVersion),
				eq(integrityPlayerMetricHistory.featureVersion, STATISTICAL_MODEL_CONFIG.featureVersion),
				lt(integrityPlayerMetricHistory.observedAt, before),
				sql`NOT EXISTS (
					SELECT 1 FROM integrity_case_events ce
					JOIN integrity_cases c ON c.id = ce.case_id
					WHERE c.org_id = ${orgId} AND c.steam_id = ${steamId}
					AND ce.event->>'killerSteamId' = ${steamId}
					AND ce.event->>'ts' IS NOT NULL
					AND (ce.event->>'ts')::timestamptz > ${integrityPlayerMetricHistory.observedAt} - interval '180 seconds'
					AND (ce.event->>'ts')::timestamptz <= ${integrityPlayerMetricHistory.observedAt}
				)`
			)
		)
		.orderBy(desc(integrityPlayerMetricHistory.observedAt))
		.limit(2001);
	if (rows.length > 2000) return null; // Bounded replay; no partial career masquerading as a lifetime reference.
	const summary = summarizeCleanCareer(rows);
	if (!summary) return null;
	const [totals] = (await db.execute(sql`
		SELECT COALESCE(SUM(GREATEST(mp.kills - mp.team_kills - mp.suicides - mp.vehicle_kills, 0)), 0)::int AS valid_kills,
		       COALESCE(SUM(mp.seconds), 0)::int AS playtime_seconds
		FROM match_players mp JOIN servers s ON s.id = mp.server_id
		WHERE s.org_id = ${orgId} AND mp.steam_id = ${steamId}`)) as {
		valid_kills: number;
		playtime_seconds: number;
	}[];
	const values = {
		orgId,
		steamId,
		firstSeenAt: summary.firstSeenAt,
		lastSeenAt: summary.lastSeenAt,
		lifetimeValidKills: Number(totals?.valid_kills ?? 0),
		lifetimePlaytimeSeconds: Number(totals?.playtime_seconds ?? 0),
		lifetimeWindows: summary.sampleCount,
		lifetimeMatches: summary.lifetimeMatches,
		activeDays: summary.uniqueDays,
		kpmDistribution: summary.kpmDistribution,
		headshotDistribution: summary.headshotDistribution,
		burstDistribution: summary.burstDistribution,
		recent24h: summary.recent24h,
		recent7d: summary.recent7d,
		recent30d: summary.recent30d,
		orderedKpm: summary.orderedKpm,
		modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
		featureVersion: STATISTICAL_MODEL_CONFIG.featureVersion,
		status: 'READY',
		updatedAt: new Date()
	};
	await db
		.insert(integrityPlayerCareers)
		.values(values)
		.onConflictDoUpdate({
			target: [integrityPlayerCareers.orgId, integrityPlayerCareers.steamId],
			set: values
		});
	return summary;
}
