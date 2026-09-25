import { describe, expect, test } from 'bun:test';
import { summarizeCommitteeShadow } from './shadow-dashboard';

describe('committee shadow calibration summary', () => {
	test('counts one latest vote per episode and latest human label per candidate case', () => {
		const watch = {
			committee: {
				decision: 'WATCH',
				verdicts: [
					{ modelId: 'tempo', decision: 'SUSPICIOUS' },
					{ modelId: 'precision', decision: 'NORMAL' }
				]
			}
		};
		const kick = {
			committee: {
				decision: 'KICK_CANDIDATE',
				verdicts: [
					{ modelId: 'tempo', decision: 'CHEAT_LIKELY' },
					{ modelId: 'career', decision: 'CHEAT_LIKELY' }
				]
			}
		};
		const result = summarizeCommitteeShadow(
			[
				{ windowId: 1, statistical: watch },
				{ windowId: 1, statistical: kick },
				{ windowId: 2, statistical: kick }
			],
			[
				{ caseId: 'c1', label: 'FALSE_POSITIVE', statistical: kick },
				{ caseId: 'c1', label: 'INSUFFICIENT_EVIDENCE', statistical: kick },
				{ caseId: 'c2', label: 'CONFIRMED_ABUSE', statistical: kick }
			],
			false
		);
		expect(result.counts.WATCH).toBe(1);
		expect(result.counts.KICK_CANDIDATE).toBe(1);
		expect(result.models.tempo.SUSPICIOUS).toBe(1);
		expect(result.disagreementRate).toBe(0.5);
		expect(result.falsePositive).toBe(1);
		expect(result.confirmedAbuse).toBe(1);
	});
});
