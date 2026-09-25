/** Empirical, server-history-based Integrity assessment. Values are never treated as normal data. */
export type MetricTail = 'upper' | 'lower';
export type MetricCode =
	| 'kpm180'
	| 'uniqueVictims'
	| 'maxKills15s'
	| 'medianKillInterval'
	| 'headshotRate'
	| 'penetrationRate'
	| 'headshotRateWeapon'
	| 'maxKillDistanceWeapon';
export type AssessmentMode = 'legacy' | 'statistical_shadow' | 'statistical';
export type SampleQuality = 'INSUFFICIENT_DATA' | 'LOW_SAMPLE' | 'NORMAL_SAMPLE' | 'HIGH_SAMPLE';
export type StatisticalLevel = 'NORMAL' | 'WATCH' | 'CASE' | 'KICK_CANDIDATE';
export type PopulationBucket = '1–20' | '21–40' | '41–60' | '61–80' | '81+';

export const METRICS: Record<MetricCode, { tail: MetricTail; family: 'Tempo' | 'Precision' }> = {
	kpm180: { tail: 'upper', family: 'Tempo' },
	uniqueVictims: { tail: 'upper', family: 'Tempo' },
	maxKills15s: { tail: 'upper', family: 'Tempo' },
	medianKillInterval: { tail: 'lower', family: 'Tempo' },
	headshotRate: { tail: 'upper', family: 'Precision' },
	penetrationRate: { tail: 'upper', family: 'Precision' },
	headshotRateWeapon: { tail: 'upper', family: 'Precision' },
	maxKillDistanceWeapon: { tail: 'upper', family: 'Precision' }
};

export interface HistogramBin {
	from: number;
	to: number;
	count: number;
}
export interface DistributionStats {
	id: string;
	source: 'local' | 'external';
	metric: MetricCode;
	map: string | null;
	populationBucket: PopulationBucket | null;
	weaponCategory: string;
	sampleCount: number;
	median: number;
	mad: number | null;
	p90: number;
	p95: number;
	p99: number;
	p995: number;
	p999: number;
	p9995: number;
	histogram: HistogramBin[];
	/** Exact value frequencies stay in Postgres and never go to the browser. */
	cdf: [number, number][];
	windowDays: number;
	calculatedAt: Date;
}

export interface MetricAssessment {
	code: MetricCode;
	source: 'local' | 'external';
	value: number;
	tail: MetricTail;
	percentile: number;
	extremenessPercentile: number;
	median: number;
	mad: number | null;
	robustZ: number | null;
	sampleCount: number;
	baselineId: string;
	map: string | null;
	populationBucket: PopulationBucket | null;
	weaponCategory: string;
	windowDays: number;
	calculatedAt: string;
	p95: number;
	p99: number;
	p999: number;
	histogram: HistogramBin[];
}

export interface StatisticalAssessment {
	status: 'READY' | 'INSUFFICIENT_DATA';
	level: StatisticalLevel | null;
	tempoPercentile: number | null;
	precisionPercentile: number | null;
	strongestMetric: { code: MetricCode; value: number; percentile: number } | null;
	independentEpisodes: number;
	sampleCount: number;
	metrics: MetricAssessment[];
}

export interface WeaponObservation {
	cause: string;
	kills: number;
	headshots: number;
	maxKillDistanceM: number | null;
}

export function populationBucket(count: number | null): PopulationBucket | null {
	if (count === null || !Number.isInteger(count) || count < 1) return null;
	if (count <= 20) return '1–20';
	if (count <= 40) return '21–40';
	if (count <= 60) return '41–60';
	if (count <= 80) return '61–80';
	return '81+';
}

export function sampleQuality(count: number): SampleQuality {
	return count < 200
		? 'INSUFFICIENT_DATA'
		: count < 1000
			? 'LOW_SAMPLE'
			: count < 5000
				? 'NORMAL_SAMPLE'
				: 'HIGH_SAMPLE';
}

/** Mid-rank empirical CDF makes ties symmetric in upper and lower tails. */
export function percentilePosition(cdf: readonly [number, number][], value: number): number {
	const total = cdf.reduce((sum, [, n]) => sum + n, 0);
	if (!total) throw new Error('Cannot rank against an empty distribution.');
	let less = 0;
	for (const [sample, count] of cdf) {
		if (value < sample) return less / total;
		if (value === sample) return (less + count / 2) / total;
		less += count;
	}
	return 1;
}

export const extremenessPercentile = (percentile: number, tail: MetricTail): number =>
	tail === 'upper' ? percentile : 1 - percentile;

export function robustZ(value: number, median: number, mad: number | null): number | null {
	return mad === null || mad === 0 ? null : (0.6745 * (value - median)) / mad;
}

export function assessDistribution(
	values: Partial<Record<MetricCode, number | null>>,
	baselines: ReadonlyMap<MetricCode, DistributionStats>,
	infantryKills: number,
	independentEpisodes: number,
	weaponObservations: readonly WeaponObservation[] = [],
	weaponBaselines: ReadonlyMap<string, DistributionStats> = new Map()
): StatisticalAssessment {
	const metrics: MetricAssessment[] = [];
	const observations: {
		code: MetricCode;
		value: number | null | undefined;
		baseline: DistributionStats | undefined;
		kills: number;
	}[] = (Object.keys(METRICS) as MetricCode[])
		.filter((code) => code !== 'headshotRateWeapon' && code !== 'maxKillDistanceWeapon')
		.map((code) => ({
			code,
			value: values[code],
			baseline: baselines.get(code),
			kills: infantryKills
		}));
	for (const weapon of weaponObservations) {
		if (weapon.kills >= 10)
			observations.push({
				code: 'headshotRateWeapon',
				value: weapon.headshots / weapon.kills,
				baseline: weaponBaselines.get(`headshotRateWeapon:${weapon.cause}`),
				kills: weapon.kills
			});
		if (weapon.kills >= 3)
			observations.push({
				code: 'maxKillDistanceWeapon',
				value: weapon.maxKillDistanceM,
				baseline: weaponBaselines.get(`maxKillDistanceWeapon:${weapon.cause}`),
				kills: weapon.kills
			});
	}
	for (const { code, value, baseline, kills } of observations) {
		if (
			value === undefined ||
			value === null ||
			!baseline ||
			baseline.sampleCount < (baseline.source === 'external' ? 30 : 200)
		)
			continue;
		if (
			(code === 'headshotRate' || code === 'penetrationRate' || code === 'headshotRateWeapon') &&
			kills < 10
		)
			continue;
		const percentile = percentilePosition(baseline.cdf, value);
		metrics.push({
			code,
			source: baseline.source,
			value,
			tail: METRICS[code].tail,
			percentile,
			extremenessPercentile: extremenessPercentile(percentile, METRICS[code].tail),
			median: baseline.median,
			mad: baseline.mad,
			robustZ: robustZ(value, baseline.median, baseline.mad),
			sampleCount: baseline.sampleCount,
			baselineId: baseline.id,
			map: baseline.map,
			populationBucket: baseline.populationBucket,
			weaponCategory: baseline.weaponCategory,
			windowDays: baseline.windowDays,
			calculatedAt: baseline.calculatedAt.toISOString(),
			p95: baseline.p95,
			p99: baseline.p99,
			p999: baseline.p999,
			histogram: baseline.histogram
		});
	}
	const maxFor = (family: 'Tempo' | 'Precision', source = metrics) => {
		const group = source.filter((metric) => METRICS[metric.code].family === family);
		return group.length ? Math.max(...group.map((metric) => metric.extremenessPercentile)) : null;
	};
	const tempo = maxFor('Tempo');
	const precision = maxFor('Precision');
	const strongest = metrics.reduce<MetricAssessment | null>(
		(best, current) =>
			!best || current.extremenessPercentile > best.extremenessPercentile ? current : best,
		null
	);
	const ready = metrics.length > 0;
	const caseLevel =
		(tempo !== null && tempo >= 0.999) ||
		(precision !== null && precision >= 0.999) ||
		(tempo !== null && tempo >= 0.99 && precision !== null && precision >= 0.99);
	// A second Tempo measurement from the same episode is not independent evidence.
	// P99.95 needs enough observations to resolve the tail; low-sample baselines remain review-only.
	const actionMetrics = metrics.filter((metric) => metric.sampleCount >= 5000);
	const actionTempo = maxFor('Tempo', actionMetrics);
	const actionPrecision = maxFor('Precision', actionMetrics);
	const kickCandidate =
		actionTempo !== null &&
		actionTempo >= 0.9995 &&
		((actionPrecision !== null && actionPrecision >= 0.995) || independentEpisodes >= 2);
	const level: StatisticalLevel | null = !ready
		? null
		: kickCandidate
			? 'KICK_CANDIDATE'
			: caseLevel
				? 'CASE'
				: (tempo !== null && tempo >= 0.99) || (precision !== null && precision >= 0.99)
					? 'WATCH'
					: 'NORMAL';
	return {
		status: ready ? 'READY' : 'INSUFFICIENT_DATA',
		level,
		tempoPercentile: tempo,
		precisionPercentile: precision,
		strongestMetric: strongest
			? {
					code: strongest.code,
					value: strongest.value,
					percentile: strongest.extremenessPercentile
				}
			: null,
		independentEpisodes,
		sampleCount: kickCandidate
			? Math.min(...actionMetrics.map((metric) => metric.sampleCount))
			: metrics.length
				? Math.min(...metrics.map((metric) => metric.sampleCount))
				: 0,
		metrics
	};
}
