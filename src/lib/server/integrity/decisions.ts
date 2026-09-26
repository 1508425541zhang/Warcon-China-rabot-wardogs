import type { BehaviorFinding } from './windows';
import type { IntegrityScore, IntegrityRuleConfig } from './score';
import type { StatisticalAssessment } from './statistics';
import { STATISTICAL_MODEL_CONFIG } from './statistical-config';

export type IntegrityDecision = 'OBSERVE' | 'KICK' | 'QUARANTINE_24H' | 'QUARANTINE_7D';
export interface EnforcementSettings {
	autoKickEnabled: boolean;
	autoQuarantine24hEnabled: boolean;
	autoQuarantine7dEnabled: boolean;
	autoActionMaxPerHour: number;
	autoActionMaxPercentOnline: number;
	autoSuspendedAt: Date | null;
}
export const DEFAULT_ENFORCEMENT: EnforcementSettings = {
	autoKickEnabled: false,
	autoQuarantine24hEnabled: false,
	autoQuarantine7dEnabled: false,
	autoActionMaxPerHour: 10,
	autoActionMaxPercentOnline: 10,
	autoSuspendedAt: null
};

export interface DecisionInput {
	score: IntegrityScore;
	finding: BehaviorFinding;
	confidence: 'A' | 'B' | 'C' | 'D';
	feedHealthy: boolean;
	playerOnline: boolean;
	onlinePlayers?: number;
	identityReliable: boolean;
	priorIndependentWindow: boolean;
	previousActions: readonly ('KICK' | 'QUARANTINE_24H' | 'QUARANTINE_7D')[];
	rules: IntegrityRuleConfig;
	settings: EnforcementSettings;
}

/** Only accepted game events count toward automatic punishment; Steam priors never open the gate. */
export function decideIntegrityAction(input: DecisionInput): IntegrityDecision {
	const {
		score,
		finding,
		confidence,
		feedHealthy,
		playerOnline,
		identityReliable,
		rules,
		settings
	} = input;
	if (
		settings.autoSuspendedAt ||
		!score.currentBehaviorAnomaly ||
		!feedHealthy ||
		!playerOnline ||
		(input.onlinePlayers ?? 0) < rules.minimumOnlineForAutoAction ||
		!identityReliable ||
		(confidence !== 'A' && confidence !== 'B')
	)
		return 'OBSERVE';
	const reasons = new Set(finding.reasons);
	const extreme = finding.kpm180 >= 8 || finding.burstPoints >= 12;
	const strong = reasons.size >= 2 || (extreme && input.priorIndependentWindow);
	const ko = reasons.size >= 2 || extreme;
	if (!ko || score.score < rules.koThreshold) return 'OBSERVE';
	if (strong && score.score >= rules.quarantineThreshold) {
		const repeat =
			input.previousActions.includes('KICK') ||
			input.previousActions.includes('QUARANTINE_24H') ||
			input.previousActions.includes('QUARANTINE_7D');
		const exceptional = finding.kpm180 >= 8 && finding.burstPoints >= 12 && reasons.size >= 3;
		if (settings.autoQuarantine7dEnabled && (repeat || exceptional)) return 'QUARANTINE_7D';
		if (settings.autoQuarantine24hEnabled) return 'QUARANTINE_24H';
	}
	return settings.autoKickEnabled ? 'KICK' : 'OBSERVE';
}

/** Statistical actions escalate only after a previously effective, independently evidenced action. */
export function decideStatisticalAction(
	input: Omit<DecisionInput, 'score'> & { assessment: StatisticalAssessment }
): IntegrityDecision {
	const { assessment, finding, settings } = input;
	const hardTempo =
		assessment.committee?.verdicts?.some(
			(vote) =>
				vote.evidenceFamily === 'TEMPO' &&
				vote.decision === 'CHEAT_LIKELY' &&
				vote.hardEvidence === true &&
				vote.confidence >= 0.995 &&
				vote.evidenceQuality >= 0.995 &&
				vote.evidenceRefs.length >= 24 &&
				finding.kpm180 >= 8
		) ?? false;
	if (
		settings.autoSuspendedAt ||
		!settings.autoKickEnabled ||
		!input.feedHealthy ||
		!input.playerOnline ||
		(input.onlinePlayers ?? 0) < input.rules.minimumOnlineForAutoAction ||
		!input.identityReliable ||
		(input.confidence !== 'A' && input.confidence !== 'B') ||
		assessment.status !== 'READY' ||
		assessment.committee?.decision !== 'KICK_CANDIDATE' ||
		assessment.committee.autoActionBlocked ||
		assessment.level !== 'KICK_CANDIDATE' ||
		assessment.sampleCount < STATISTICAL_MODEL_CONFIG.minimumBaselineSamples ||
		assessment.actionTempoPercentile === null ||
		assessment.actionTempoPercentile < STATISTICAL_MODEL_CONFIG.kickPercentile ||
		!(
			(assessment.actionPrecisionPercentile !== null &&
				assessment.actionPrecisionPercentile >= STATISTICAL_MODEL_CONFIG.watchPercentile) ||
			(input.priorIndependentWindow && assessment.independentEpisodes >= 2) ||
			hardTempo
		)
	)
		return 'OBSERVE';
	// Keep a configurable absolute floor in addition to the independent statistical evidence.
	if (finding.kpm180 < input.rules.kpmBands[0].min) return 'OBSERVE';
	if (
		settings.autoQuarantine7dEnabled &&
		assessment.independentEpisodes >= 3 &&
		input.previousActions.includes('QUARANTINE_24H')
	)
		return 'QUARANTINE_7D';
	if (
		settings.autoQuarantine24hEnabled &&
		assessment.independentEpisodes >= 2 &&
		input.previousActions.includes('KICK')
	)
		return 'QUARANTINE_24H';
	return 'KICK';
}
