import type { BehaviorFinding } from './windows';
import type { IntegrityScore, IntegrityRuleConfig } from './score';
import type { StatisticalAssessment } from './statistics';

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

/** First statistical enforcement release permits only a protective kick with independent evidence. */
export function decideStatisticalAction(
	input: Omit<DecisionInput, 'score' | 'rules'> & { assessment: StatisticalAssessment }
): IntegrityDecision {
	const { assessment, finding, settings } = input;
	if (
		settings.autoSuspendedAt ||
		!settings.autoKickEnabled ||
		!input.feedHealthy ||
		!input.playerOnline ||
		!input.identityReliable ||
		(input.confidence !== 'A' && input.confidence !== 'B') ||
		assessment.status !== 'READY' ||
		assessment.level !== 'KICK_CANDIDATE' ||
		assessment.sampleCount < 5000 ||
		assessment.tempoPercentile === null ||
		assessment.tempoPercentile < 0.9995 ||
		!(
			(assessment.precisionPercentile !== null && assessment.precisionPercentile >= 0.995) ||
			(input.priorIndependentWindow && assessment.independentEpisodes >= 2)
		)
	)
		return 'OBSERVE';
	// Existing extreme behavior is a hard safety floor: rarity alone cannot lower the action bar.
	if (finding.kpm180 < 8) return 'OBSERVE';
	return 'KICK';
}
