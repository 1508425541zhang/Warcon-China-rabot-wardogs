import type { MetricAssessment } from './server/integrity/statistics';

/** Live reference charts have no player observation. Saved assessments retain their marker. */
export type DistributionChartMetric = Omit<
	MetricAssessment,
	'value' | 'percentile' | 'extremenessPercentile' | 'robustZ'
> & {
	value: number | null;
	percentile: number | null;
	extremenessPercentile: number | null;
	robustZ: number | null;
};
