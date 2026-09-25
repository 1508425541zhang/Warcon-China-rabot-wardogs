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
