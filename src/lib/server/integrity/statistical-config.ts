/** Statistical mathematics is versioned in code and cannot be changed by org admins. */
export const STATISTICAL_MODEL_CONFIG = Object.freeze({
	modelVersion: 'ensemble-server-round-v3',
	featureVersion: 'rolling-infantry-v2',
	persistenceEpisodeHorizonHours: 24,
	minimumEpisodeSeparationSeconds: 60,
	minimumIndependentEpisodes: 2,
	minimumAssessmentSamples: 50,
	minimumBaselineSamples: 200,
	minimumBaselinePlayers: 20,
	minimumBaselinePlayerDays: 20,
	// Player-balanced weights must still retain enough effective observations.
	minimumEffectiveSampleSize: 100,
	watchPercentile: 0.9,
	kickPercentile: 0.95,
	baselineRefreshMinutes: 5,
	maximumBaselineAgeHours: 48
});

/** Capability gate; each organization must still opt into statistical mode and automatic kick. */
export const STATISTICAL_AUTO_ACTION_ENABLED = true;

export function actionBaselineEligible(metric: {
	source: string;
	code?: string;
	sampleCount: number;
	uniquePlayers?: number;
	uniquePlayerDays?: number;
	effectiveSampleSize?: number;
}): boolean {
	return (
		metric.source === 'local' &&
		metric.code !== 'maxKillDistanceWeapon' &&
		metric.sampleCount >= STATISTICAL_MODEL_CONFIG.minimumBaselineSamples &&
		(metric.uniquePlayers ?? 0) >= STATISTICAL_MODEL_CONFIG.minimumBaselinePlayers &&
		(metric.uniquePlayerDays ?? 0) >= STATISTICAL_MODEL_CONFIG.minimumBaselinePlayerDays &&
		(metric.effectiveSampleSize ?? 0) >= STATISTICAL_MODEL_CONFIG.minimumEffectiveSampleSize
	);
}
