export const ACTION_RETRY_MS = 30_000;

/** A candidate can be retried after a reversible eligibility failure on the same frozen case. */
export function shouldRetryActionEligibility(input: {
	caseOpen: boolean;
	hasAction: boolean;
	lastAttemptAt: Date | null;
	now: Date;
}): boolean {
	return (
		input.caseOpen &&
		!input.hasAction &&
		(input.lastAttemptAt === null ||
			input.now.getTime() - input.lastAttemptAt.getTime() >= ACTION_RETRY_MS)
	);
}
import type { StatisticalAssessment } from './statistics';

/** Frozen evidence is immutable; refreshed eligibility gets a new case instead of retrying a stale veto forever. */
export function canReuseStatisticalCase(
	saved: StatisticalAssessment | null,
	current: StatisticalAssessment
): boolean {
	return (
		!!saved &&
		saved.modelVersion === current.modelVersion &&
		saved.featureVersion === current.featureVersion &&
		saved.weaponMapVersion === current.weaponMapVersion &&
		saved.baselineGeneration === current.baselineGeneration &&
		!(saved.committee?.autoActionBlocked && current.committee?.autoActionBlocked === false)
	);
}
