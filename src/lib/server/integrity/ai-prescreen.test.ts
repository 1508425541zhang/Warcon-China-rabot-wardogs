import { expect, test } from 'bun:test';
import { needsAiPrescreen } from './ai-prescreen';
import type { StatisticalAssessment } from './statistics';

test('AI prescreen accepts either positive vote without altering formal gates', () => {
	const assessment = (decision: string, level = 'NORMAL', status = 'READY') =>
		({
			status,
			level,
			committee: { verdicts: [{ decision }] }
		}) as unknown as StatisticalAssessment;
	expect(needsAiPrescreen(assessment('SUSPICIOUS'))).toBe(true);
	expect(needsAiPrescreen(assessment('CHEAT_LIKELY', 'WATCH'))).toBe(true);
	for (const vote of ['NORMAL', 'UNKNOWN'])
		expect(needsAiPrescreen(assessment(vote, 'WATCH'))).toBe(false);
	for (const level of ['CASE', 'KICK_CANDIDATE'])
		expect(needsAiPrescreen(assessment('CHEAT_LIKELY', level))).toBe(false);
	expect(needsAiPrescreen(assessment('CHEAT_LIKELY', 'NORMAL', 'INSUFFICIENT_DATA'))).toBe(false);
	expect(needsAiPrescreen(null)).toBe(false);
});
