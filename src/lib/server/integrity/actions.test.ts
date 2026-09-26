import { expect, test } from 'bun:test';
import { effectiveActionKinds } from './actions';
import type { integrityActions } from '../db/schema';

type Action = Pick<
	typeof integrityActions.$inferSelect,
	'action' | 'source' | 'effectiveAt' | 'revertedAt' | 'deliveryState'
> & { expiresAt?: Date | null };
const action = (overrides: Partial<Action>): Action => ({
	action: 'KICK',
	source: 'RULE',
	effectiveAt: new Date(),
	revertedAt: null,
	deliveryState: 'failed',
	expiresAt: new Date(Date.now() + 24 * 3_600_000),
	...overrides
});

test('quarantine remains effective when its separate immediate kick fails', () => {
	expect(
		effectiveActionKinds([
			action({ action: 'QUARANTINE_24H' }),
			action({ action: 'KICK' }),
			action({ action: 'QUARANTINE_7D', revertedAt: new Date() })
		])
	).toEqual(['QUARANTINE_24H']);
	expect(
		effectiveActionKinds([action({ action: 'QUARANTINE_24H', expiresAt: new Date(0) })])
	).toEqual([]);
});
