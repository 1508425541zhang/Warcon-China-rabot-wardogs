import type { StatisticalAssessment } from './statistics';
import { STATISTICAL_AUTO_ACTION_ENABLED, STATISTICAL_MODEL_CONFIG } from './statistical-config';

export type ExpertDecision = 'NORMAL' | 'SUSPICIOUS' | 'CHEAT_LIKELY' | 'UNKNOWN';
export type EvidenceFamily =
	'TEMPO' | 'PRECISION' | 'CAREER' | 'CHANGE_POINT' | 'PERSISTENCE' | 'WEAPON';
export interface ExpertVerdict {
	modelId: string;
	modelVersion: string;
	evidenceFamily: EvidenceFamily;
	decision: ExpertDecision;
	confidence: number;
	evidenceQuality: number;
	reasons: string[];
	evidenceRefs: string[];
	hardEvidence?: boolean;
}
export interface ModelInput {
	statistical: StatisticalAssessment;
	independentEpisodes: number;
	/** Clean, normal career windows only. Missing history yields UNKNOWN. */
	career?: {
		sampleCount: number;
		uniqueDays: number;
		kpmMedian: number;
		kpmMad: number | null;
		recentKpm: number[];
	};
	/** Completed all-weapon kill-rate buckets from this round only. */
	changeSeries?: number[];
	currentKpm: number;
	eventIds: string[];
}
export interface IntegrityExpertModel {
	id: string;
	version: string;
	evidenceFamily: EvidenceFamily;
	assess(input: ModelInput): ExpertVerdict;
}
export type CommitteeDecision =
	'NORMAL' | 'WATCH' | 'CASE' | 'KICK_CANDIDATE' | 'ESCALATION_CANDIDATE';
export interface CommitteeResult {
	generation: string;
	verdicts: ExpertVerdict[];
	cheatVotes: number;
	suspiciousVotes: number;
	normalVotes: number;
	unknownVotes: number;
	participatingModels: number;
	independentCheatFamilies: number;
	independentSuspiciousFamilies: number;
	decision: CommitteeDecision;
	autoActionBlocked: boolean;
	vetoReasons: string[];
}

/** Persistence may count only a prior episode with an independent abnormal signal. */
export function hasStatisticalAnomaly(assessment: StatisticalAssessment | null): boolean {
	if (
		!assessment ||
		assessment.status !== 'READY' ||
		assessment.modelVersion !== STATISTICAL_MODEL_CONFIG.modelVersion ||
		assessment.featureVersion !== STATISTICAL_MODEL_CONFIG.featureVersion
	)
		return false;
	return (
		assessment.metrics.some(
			(metric) =>
				metric.code !== 'maxKillDistanceWeapon' &&
				metric.extremenessPercentile >= STATISTICAL_MODEL_CONFIG.watchPercentile
		) ||
		!!assessment.committee?.verdicts.some(
			(v) =>
				v.modelId !== 'persistence' &&
				(v.decision === 'SUSPICIOUS' || v.decision === 'CHEAT_LIKELY')
		)
	);
}

const verdict = (
	model: IntegrityExpertModel,
	decision: ExpertDecision,
	reasons: string[],
	refs: string[],
	quality = 1
): ExpertVerdict => ({
	modelId: model.id,
	modelVersion: model.version,
	evidenceFamily: model.evidenceFamily,
	decision,
	confidence: decision === 'UNKNOWN' ? 0 : Math.min(1, quality),
	evidenceQuality: quality,
	reasons,
	evidenceRefs: refs
});
const model = (
	id: string,
	family: EvidenceFamily,
	assess: (input: ModelInput, self: IntegrityExpertModel) => ExpertVerdict
): IntegrityExpertModel => {
	const self: IntegrityExpertModel = {
		id,
		version: '1',
		evidenceFamily: family,
		assess(input) {
			return assess(input, self);
		}
	};
	return self;
};
const strongest = (input: ModelInput, family: 'Tempo' | 'Precision') =>
	input.statistical.metrics
		.filter((metric) =>
			family === 'Tempo'
				? ['kpm180', 'uniqueVictims', 'maxKills15s', 'medianKillInterval'].includes(metric.code)
				: ['headshotRate', 'penetrationRate', 'headshotRateWeapon'].includes(metric.code)
		)
		.filter(
			(metric) =>
				metric.source === 'local' &&
				metric.sampleCount >= STATISTICAL_MODEL_CONFIG.minimumAssessmentSamples
		)
		// A finite reference cannot resolve a more extreme tail than one extra observation.
		.map((metric) => ({
			...metric,
			extremenessPercentile: Math.min(
				metric.extremenessPercentile,
				metric.sampleCount / (metric.sampleCount + 1)
			)
		}))
		.sort((a, b) => b.extremenessPercentile - a.extremenessPercentile)[0];

export const EXPERT_MODELS: readonly IntegrityExpertModel[] = [
	model('tempo', 'TEMPO', (input, self) => {
		const metric = strongest(input, 'Tempo');
		if (!metric) return verdict(self, 'UNKNOWN', ['NO_CLEAN_TEMPO_BASELINE'], input.eventIds, 0);
		const result = verdict(
			self,
			metric.extremenessPercentile >= STATISTICAL_MODEL_CONFIG.kickPercentile
				? 'CHEAT_LIKELY'
				: metric.extremenessPercentile >= STATISTICAL_MODEL_CONFIG.watchPercentile
					? 'SUSPICIOUS'
					: 'NORMAL',
			[metric.code],
			input.eventIds
		);
		return result;
	}),
	model('precision', 'PRECISION', (input, self) => {
		const metric = strongest(input, 'Precision');
		if (!metric)
			return verdict(self, 'UNKNOWN', ['NO_CLEAN_PRECISION_BASELINE'], input.eventIds, 0);
		return verdict(
			self,
			metric.extremenessPercentile >= STATISTICAL_MODEL_CONFIG.kickPercentile
				? 'CHEAT_LIKELY'
				: metric.extremenessPercentile >= STATISTICAL_MODEL_CONFIG.watchPercentile
					? 'SUSPICIOUS'
					: 'NORMAL',
			[metric.code],
			input.eventIds
		);
	}),
	model('career_deviation', 'CAREER', (input, self) => {
		const c = input.career;
		if (!c || c.sampleCount < 100 || c.uniqueDays < 10 || !c.kpmMad || c.kpmMad <= 0)
			return verdict(self, 'UNKNOWN', ['INSUFFICIENT_CLEAN_CAREER'], input.eventIds, 0);
		const robustZ = (0.6745 * (input.currentKpm - c.kpmMedian)) / c.kpmMad;
		const mad = c.kpmMad;
		const sustained =
			c.recentKpm.length >= 3 && c.recentKpm.slice(-3).every((v) => v > c.kpmMedian + 4 * mad);
		return verdict(
			self,
			robustZ >= 8 && sustained ? 'CHEAT_LIKELY' : robustZ >= 4 ? 'SUSPICIOUS' : 'NORMAL',
			[`robust_z:${robustZ.toFixed(2)}`, sustained ? 'SUSTAINED' : 'NOT_SUSTAINED'],
			input.eventIds
		);
	}),
	model('change_point', 'CHANGE_POINT', (input, self) => {
		const series = input.changeSeries;
		if (!series || series.length < 20)
			return verdict(self, 'UNKNOWN', ['INSUFFICIENT_ORDERED_HISTORY'], input.eventIds, 0);
		const historic = series.slice(0, -3);
		const recent = series.slice(-3);
		const mean = historic.reduce((a, b) => a + b, 0) / historic.length;
		const variance = historic.reduce((a, b) => a + (b - mean) ** 2, 0) / historic.length;
		const scale = Math.sqrt(variance);
		if (scale < 0.1)
			return verdict(self, 'UNKNOWN', ['CAREER_VARIANCE_UNRESOLVED'], input.eventIds, 0);
		// One-sided CUSUM: an abrupt, persistent upward shift; a single peak cannot pass.
		let cusum = 0;
		for (const value of recent) cusum = Math.max(0, cusum + (value - mean) / scale - 0.5);
		const persistent = recent.every((value) => value > mean + 2 * scale);
		return verdict(
			self,
			cusum >= 15 && persistent
				? 'CHEAT_LIKELY'
				: cusum >= 8 && persistent
					? 'SUSPICIOUS'
					: 'NORMAL',
			[`cusum:${cusum.toFixed(2)}`],
			input.eventIds
		);
	}),
	model('persistence', 'PERSISTENCE', (input, self) => {
		if (input.statistical.status !== 'READY')
			return verdict(self, 'UNKNOWN', ['NO_CURRENT_ASSESSMENT'], input.eventIds, 0);
		const n = input.independentEpisodes;
		return verdict(
			self,
			n >= 3 ? 'CHEAT_LIKELY' : n >= 2 ? 'SUSPICIOUS' : 'NORMAL',
			[`independent_episodes:${n}`],
			input.eventIds
		);
	})
];

export interface DataQualityInput {
	feedHealthy: boolean;
	backlogSafe: boolean;
	identityReliable: boolean;
	roundReliable: boolean;
	baselineFresh: boolean;
	versionsMatch: boolean;
	baselinePopulationAdequate: boolean;
}
export function dataQualityVeto(input: DataQualityInput): string[] {
	return (Object.entries(input) as [keyof DataQualityInput, boolean][])
		.filter(([, ok]) => !ok)
		.map(([key]) => key.toUpperCase());
}

/** Five expert ballots. Three high or four positive ballots create a review case only. */
export function voteCommittee(
	verdicts: readonly ExpertVerdict[],
	vetoReasons: readonly string[] = []
): CommitteeResult {
	const rank: Record<ExpertDecision, number> = {
		UNKNOWN: 0,
		NORMAL: 1,
		SUSPICIOUS: 2,
		CHEAT_LIKELY: 3
	};
	// One ballot per named expert. Unknowns never count as positive ballots.
	const byModel = new Map<string, ExpertVerdict>();
	for (const item of verdicts) {
		const current = byModel.get(item.modelId);
		if (!current || rank[item.decision] > rank[current.decision]) byModel.set(item.modelId, item);
	}
	const votes = [...byModel.values()];
	const count = (decision: ExpertDecision) => votes.filter((v) => v.decision === decision).length;
	const cheatVotes = count('CHEAT_LIKELY'),
		suspiciousVotes = count('SUSPICIOUS'),
		unknownVotes = count('UNKNOWN');
	const kick = cheatVotes >= 3 || cheatVotes + suspiciousVotes >= 4;

	return {
		generation: STATISTICAL_MODEL_CONFIG.modelVersion,
		verdicts: [...verdicts],
		cheatVotes,
		suspiciousVotes,
		normalVotes: count('NORMAL'),
		unknownVotes,
		participatingModels: votes.length - unknownVotes,
		independentCheatFamilies: cheatVotes,
		independentSuspiciousFamilies: suspiciousVotes,
		decision: kick ? 'CASE' : cheatVotes + suspiciousVotes >= 2 ? 'WATCH' : 'NORMAL',
		autoActionBlocked: vetoReasons.length > 0 || !STATISTICAL_AUTO_ACTION_ENABLED,
		vetoReasons: [
			...vetoReasons,
			...(!STATISTICAL_AUTO_ACTION_ENABLED ? ['SHADOW_CALIBRATION_REQUIRED'] : [])
		]
	};
}

export function assessCommittee(
	input: ModelInput,
	quality: DataQualityInput,
	models: readonly IntegrityExpertModel[] = EXPERT_MODELS
): CommitteeResult {
	const result = voteCommittee(
		models.map((expert) => expert.assess(input)),
		dataQualityVeto(quality)
	);
	const kpmSupport =
		input.currentKpm > 4 &&
		result.verdicts.some(
			(v) => v.modelId !== 'tempo' && (v.decision === 'SUSPICIOUS' || v.decision === 'CHEAT_LIKELY')
		);
	return {
		...result,
		decision: kpmSupport
			? 'KICK_CANDIDATE'
			: result.decision === 'NORMAL' && input.currentKpm > 2
				? 'WATCH'
				: result.decision
	};
}
