import { expect, test } from 'bun:test';
import {
	factionLockDecision as decide,
	originalFactionLeading as leading
} from './faction-lock-policy';
const normal = {
	from: 'A',
	to: 'B',
	teams: ['A', 'B', 'C'],
	authorized: false,
	transition: false,
	full: null,
	leading: null
};
test('ordered exemptions and seat fallback follow requested policy', () => {
	expect(decide(normal)).toBe('RESTORE');
	expect(decide({ ...normal, from: null })).toBe('INITIAL_OR_UNASSIGNED');
	expect(decide({ ...normal, from: 'White' })).toBe('INITIAL_OR_UNASSIGNED');
	expect(decide({ ...normal, authorized: true, full: true, leading: true })).toBe(
		'AUTHORIZED_MOVE'
	);
	expect(decide({ ...normal, transition: true })).toBe('ROUND_GRACE');
	expect(decide({ ...normal, full: true, leading: false })).toBe('FULL_NOT_LEADING');
	expect(decide({ ...normal, full: true, leading: true })).toBe('WARN_KICK');
	expect(decide({ ...normal, full: true })).toBe('UNKNOWN_SCORE');
});
test('original faction must be strictly ahead; ties and missing score are not leadership', () => {
	expect(
		leading(
			[
				{ name: 'A', score: 10 },
				{ name: 'B', score: 9 },
				{ name: 'C', score: 8 }
			],
			'A'
		)
	).toBe(true);
	expect(
		leading(
			[
				{ name: 'A', score: 10 },
				{ name: 'B', score: 10 }
			],
			'A'
		)
	).toBe(false);
	expect(leading([{ name: 'A', score: 10 }], 'A')).toBeNull();
});
