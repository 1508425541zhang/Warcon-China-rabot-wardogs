import { and, eq, isNull, or } from 'drizzle-orm';
import type { Env } from '../env';
import { integrityBaselines, integrityModelState } from '../db/schema';
import type { DistributionChartMetric } from '$lib/integrity-distribution';
import { METRICS, type MetricCode, type PopulationBucket } from './statistics';
import { STATISTICAL_MODEL_CONFIG } from './statistical-config';

type Baseline = typeof integrityBaselines.$inferSelect;
export function emptyDistributionDashboard(): Awaited<
	ReturnType<typeof loadDistributionDashboard>
> {
	return {
		status: 'DISABLED',
		updatedAt: null,
		lastFailureAt: null,
		refreshMinutes: STATISTICAL_MODEL_CONFIG.baselineRefreshMinutes,
		dataBefore: null,
		metrics: []
	};
}

/** Descriptive projection only: never re-score a player or send exact CDF frequencies. */
export function distributionChart(row: Baseline): DistributionChartMetric {
	const code = row.metric as MetricCode;
	return {
		code,
		serverId: row.serverId,
		source: row.source as 'local' | 'external',
		value: null,
		percentile: null,
		extremenessPercentile: null,
		robustZ: null,
		tail: METRICS[code].tail,
		median: row.median,
		mad: row.mad,
		p95: row.p95,
		p99: row.p99,
		p999: row.p999,
		sampleCount: row.sampleCount,
		uniquePlayers: row.uniquePlayers,
		uniquePlayerDays: row.uniquePlayerDays,
		effectiveSampleSize: row.effectiveSampleSize,
		modelVersion: row.modelVersion,
		featureVersion: row.featureVersion,
		weaponMapVersion: row.weaponMapVersion,
		baselineGeneration: row.generation,
		baselineId: row.id,
		map: row.map,
		populationBucket: row.populationBucket as PopulationBucket | null,
		weaponCategory: row.weaponCategory,
		windowDays: row.windowDays,
		calculatedAt: row.calculatedAt.toISOString(),
		histogram: row.histogram as DistributionChartMetric['histogram']
	};
}

export async function loadDistributionDashboard(env: Env, orgId: string, serverId: string) {
	return env.db.transaction(
		async (tx) => {
			const [state] = await tx
				.select()
				.from(integrityModelState)
				.where(eq(integrityModelState.orgId, orgId));
			const rows = state?.activeBaselineGeneration
				? await tx
						.select()
						.from(integrityBaselines)
						.where(
							and(
								eq(integrityBaselines.orgId, orgId),
								eq(integrityBaselines.generation, state.activeBaselineGeneration),
								eq(integrityBaselines.modelVersion, STATISTICAL_MODEL_CONFIG.modelVersion),
								eq(integrityBaselines.featureVersion, STATISTICAL_MODEL_CONFIG.featureVersion),
								eq(integrityBaselines.weaponMapVersion, state.weaponMapVersion),
								eq(integrityBaselines.level, 3),
								or(eq(integrityBaselines.serverId, serverId), isNull(integrityBaselines.serverId))
							)
						)
				: [];
			const updatedAt = state?.lastRefreshAt ?? null;
			const stale =
				!updatedAt ||
				Date.now() - updatedAt.getTime() >
					STATISTICAL_MODEL_CONFIG.maximumBaselineAgeHours * 3_600_000;
			return {
				status: stale ? 'STALE' : (state?.baselineStatus ?? 'STALE'),
				updatedAt: updatedAt?.toISOString() ?? null,
				lastFailureAt: state?.lastFailureAt?.toISOString() ?? null,
				refreshMinutes: STATISTICAL_MODEL_CONFIG.baselineRefreshMinutes,
				dataBefore: updatedAt ? new Date(updatedAt.getTime() - 10 * 60_000).toISOString() : null,
				metrics: rows
					.filter((row) => row.metric in METRICS)
					.sort(
						(a, b) =>
							a.metric.localeCompare(b.metric) ||
							a.weaponCategory.localeCompare(b.weaponCategory) ||
							a.source.localeCompare(b.source)
					)
					.map(distributionChart)
			};
		},
		{ isolationLevel: 'repeatable read', accessMode: 'read only' }
	);
}
