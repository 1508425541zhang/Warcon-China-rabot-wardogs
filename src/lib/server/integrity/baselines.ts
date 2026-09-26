import { and, eq, gte, or, sql } from 'drizzle-orm';
import type { Env } from '../env';
import { integrityBaselines, integrityModelState, organizations, samples } from '../db/schema';
import { refreshCleanIntegrityBaselines as refreshIntegrityBaselines } from './baseline-replay';
import { STATISTICAL_MODEL_CONFIG } from './statistical-config';
import {
	METRICS,
	populationBucket,
	type DistributionStats,
	type MetricCode,
	type PopulationBucket
} from './statistics';

const WINDOW_DAYS = 30;
let timer: ReturnType<typeof setInterval> | null = null;
let initial: ReturnType<typeof setTimeout> | null = null;

/** Replays the same rolling feature generator used by the live consumer. */
export { refreshIntegrityBaselines };

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
	now = new Date(),
	state?: {
		weaponMapVersion: number;
		activeBaselineGeneration: string | null;
		baselineStatus: string;
	},
	serverId?: string
): Map<MetricCode, DistributionStats> {
	const selected = new Map<MetricCode, DistributionStats>();
	for (const metric of Object.keys(METRICS) as MetricCode[]) {
		const row = rows
			.filter(
				(candidate) =>
					candidate.metric === metric &&
					(!['headshotRate', 'penetrationRate', 'headshotRateWeapon'].includes(metric) ||
						(!!serverId && candidate.serverId === serverId)) &&
					candidate.weaponCategory === 'INFANTRY' &&
					candidate.sampleCount >=
						(candidate.source === 'external'
							? 30
							: STATISTICAL_MODEL_CONFIG.minimumAssessmentSamples) &&
					candidate.windowDays === WINDOW_DAYS &&
					candidate.modelVersion === STATISTICAL_MODEL_CONFIG.modelVersion &&
					candidate.featureVersion === STATISTICAL_MODEL_CONFIG.featureVersion &&
					(!state ||
						(state.baselineStatus === 'READY' &&
							candidate.generation === state.activeBaselineGeneration &&
							candidate.weaponMapVersion === state.weaponMapVersion)) &&
					now.getTime() - candidate.calculatedAt.getTime() <=
						STATISTICAL_MODEL_CONFIG.maximumBaselineAgeHours * 60 * 60_000
			)
			.sort((a, b) => (a.source === b.source ? a.level - b.level : a.source === 'local' ? -1 : 1))
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
				serverId: row.serverId,
				source: row.source as DistributionStats['source'],
				metric,
				map: row.map,
				populationBucket: row.populationBucket as PopulationBucket | null,
				weaponCategory: row.weaponCategory,
				sampleCount: row.sampleCount,
				uniquePlayers: row.uniquePlayers,
				uniquePlayerDays: row.uniquePlayerDays,
				effectiveSampleSize: row.effectiveSampleSize,
				modelVersion: row.modelVersion,
				featureVersion: row.featureVersion,
				weaponMapVersion: row.weaponMapVersion,
				baselineGeneration: row.generation,
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

/** Exact cause is the comparison cohort; unknown and mixed weapons never receive a proxy baseline. */
export function selectWeaponBaselines(
	rows: readonly (typeof integrityBaselines.$inferSelect)[],
	map: string,
	bucket: PopulationBucket | null,
	now = new Date(),
	state?: {
		weaponMapVersion: number;
		activeBaselineGeneration: string | null;
		baselineStatus: string;
	},
	serverId?: string
): Map<string, DistributionStats> {
	const selected = new Map<string, DistributionStats>();
	for (const row of [...rows].sort((a, b) =>
		a.source === b.source ? a.level - b.level : a.source === 'local' ? -1 : 1
	)) {
		if (
			(row.metric === 'headshotRateWeapon' && (!serverId || row.serverId !== serverId)) ||
			(row.metric !== 'headshotRateWeapon' && row.metric !== 'maxKillDistanceWeapon') ||
			row.weaponCategory === 'INFANTRY' ||
			row.sampleCount <
				(row.source === 'external' ? 30 : STATISTICAL_MODEL_CONFIG.minimumAssessmentSamples) ||
			row.windowDays !== WINDOW_DAYS ||
			row.modelVersion !== STATISTICAL_MODEL_CONFIG.modelVersion ||
			row.featureVersion !== STATISTICAL_MODEL_CONFIG.featureVersion ||
			(state &&
				(state.baselineStatus !== 'READY' ||
					row.generation !== state.activeBaselineGeneration ||
					row.weaponMapVersion !== state.weaponMapVersion)) ||
			now.getTime() - row.calculatedAt.getTime() >
				STATISTICAL_MODEL_CONFIG.maximumBaselineAgeHours * 60 * 60_000 ||
			(row.level === 1 &&
				(row.map !== map || row.populationBucket !== bucket || bucket === null)) ||
			(row.level === 2 && (row.populationBucket !== bucket || bucket === null))
		)
			continue;
		const key = `${row.metric}:${row.weaponCategory}`;
		if (selected.has(key)) continue;
		selected.set(key, {
			id: row.id,
			serverId: row.serverId,
			source: row.source as DistributionStats['source'],
			metric: row.metric as DistributionStats['metric'],
			map: row.map,
			populationBucket: row.populationBucket as PopulationBucket | null,
			weaponCategory: row.weaponCategory,
			sampleCount: row.sampleCount,
			uniquePlayers: row.uniquePlayers,
			uniquePlayerDays: row.uniquePlayerDays,
			effectiveSampleSize: row.effectiveSampleSize,
			modelVersion: row.modelVersion,
			featureVersion: row.featureVersion,
			weaponMapVersion: row.weaponMapVersion,
			baselineGeneration: row.generation,
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

export async function loadWeaponBaselines(
	env: Env,
	orgId: string,
	map: string,
	bucket: PopulationBucket | null,
	serverId?: string
): Promise<Map<string, DistributionStats>> {
	const [state] = await env.db
		.select()
		.from(integrityModelState)
		.where(eq(integrityModelState.orgId, orgId));
	if (!state || state.baselineStatus !== 'READY') return new Map();
	const rows = await env.db
		.select()
		.from(integrityBaselines)
		.where(
			and(
				eq(integrityBaselines.orgId, orgId),
				or(
					eq(integrityBaselines.metric, 'headshotRateWeapon'),
					eq(integrityBaselines.metric, 'maxKillDistanceWeapon')
				),
				gte(integrityBaselines.sampleCount, 30),
				gte(
					integrityBaselines.calculatedAt,
					new Date(Date.now() - STATISTICAL_MODEL_CONFIG.maximumBaselineAgeHours * 60 * 60_000)
				)
			)
		);
	return selectWeaponBaselines(rows, map, bucket, new Date(), state, serverId);
}

export async function loadBaselines(
	env: Env,
	orgId: string,
	map: string,
	bucket: PopulationBucket | null,
	serverId?: string
) {
	const [state] = await env.db
		.select()
		.from(integrityModelState)
		.where(eq(integrityModelState.orgId, orgId));
	if (!state || state.baselineStatus !== 'READY') return new Map<MetricCode, DistributionStats>();
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
					gte(integrityBaselines.sampleCount, 30),
					gte(
						integrityBaselines.calculatedAt,
						new Date(Date.now() - STATISTICAL_MODEL_CONFIG.maximumBaselineAgeHours * 60 * 60_000)
					),
					scope
				)
			),
		map,
		bucket,
		new Date(),
		state,
		serverId
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
	let running = false;
	const run = async () => {
		if (running) return;
		running = true;
		try {
			const orgs = await env.db.select({ id: organizations.id }).from(organizations);
			for (const org of orgs) {
				try {
					await refreshIntegrityBaselines(env, org.id);
				} catch (error) {
					console.error(`[warcon] Integrity baseline ${org.id}:`, error);
				}
			}
		} finally {
			running = false;
		}
	};
	timer = setInterval(
		() => void run().catch((error) => console.error('[warcon] Integrity baselines:', error)),
		STATISTICAL_MODEL_CONFIG.baselineRefreshMinutes * 60_000
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
