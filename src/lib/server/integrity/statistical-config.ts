/** Statistical mathematics is versioned in code and cannot be changed by org admins. */
export const STATISTICAL_MODEL_CONFIG = Object.freeze({
	modelVersion: 'ensemble-shadow-v1',
	featureVersion: 'rolling-infantry-v1',
	persistenceEpisodeHorizonHours: 24,
	minimumEpisodeSeparationSeconds: 180,
	minimumIndependentEpisodes: 2,
	minimumBaselineSamples: 5000,
	minimumBaselinePlayers: 100,
	minimumBaselinePlayerDays: 300,
	minimumEffectiveSampleSize: 1000,
	maximumBaselineAgeHours: 48
});

/** Release gate: unit tests and synthetic baselines are insufficient for real-server enforcement. */
export const STATISTICAL_AUTO_ACTION_ENABLED = false;
