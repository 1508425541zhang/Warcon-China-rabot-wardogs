import type { StatisticalAssessment } from './statistics';

export const AI_PRESCREEN_TRIGGER = 'AI_SINGLE_SIGNAL';

/** Additional AI triage only; never changes the committee verdict or action eligibility. */
export function needsAiPrescreen(assessment: StatisticalAssessment | null): boolean {
	return (
		!!assessment &&
		assessment.status === 'READY' &&
		(assessment.level === 'NORMAL' || assessment.level === 'WATCH') &&
		!!assessment.committee?.verdicts.some(
			(vote) => vote.decision === 'SUSPICIOUS' || vote.decision === 'CHEAT_LIKELY'
		)
	);
}
