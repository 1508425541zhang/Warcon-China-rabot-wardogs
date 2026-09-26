import { expect, test } from 'bun:test';
import { shouldRetryActionEligibility, canReuseStatisticalCase } from './action-retry';
import type { StatisticalAssessment } from './statistics';

test('a refreshed candidate cannot reuse a stale generation or frozen quality veto', () => {
	const current = {
		modelVersion: 'v2',
		featureVersion: 'f2',
		weaponMapVersion: 1,
		baselineGeneration: 'g2',
		committee: { autoActionBlocked: false }
	} as StatisticalAssessment;
	expect(canReuseStatisticalCase(current, current)).toBe(true);
	expect(canReuseStatisticalCase({ ...current, baselineGeneration: 'g1' }, current)).toBe(false);
	expect(
		canReuseStatisticalCase(
			{ ...current, committee: { ...current.committee!, autoActionBlocked: true } },
			current
		)
	).toBe(false);
});

test('offline candidate can retry on same case after eligibility cooldown', () => {
	const now = new Date('2026-01-01T00:01:00Z');
	expect(
		shouldRetryActionEligibility({
			caseOpen: true,
			hasAction: false,
			lastAttemptAt: new Date(now.getTime() - 60_000),
			now
		})
	).toBe(true);
	expect(
		shouldRetryActionEligibility({
			caseOpen: true,
			hasAction: false,
			lastAttemptAt: new Date(now.getTime() - 1_000),
			now
		})
	).toBe(false);
});
test('an action or closed case can never queue a duplicate eligibility attempt', () => {
	const now = new Date();
	expect(
		shouldRetryActionEligibility({ caseOpen: true, hasAction: true, lastAttemptAt: null, now })
	).toBe(false);
	expect(
		shouldRetryActionEligibility({ caseOpen: false, hasAction: false, lastAttemptAt: null, now })
	).toBe(false);
});
