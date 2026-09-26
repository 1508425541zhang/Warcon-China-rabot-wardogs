import { test, expect } from 'bun:test';
import { consecutiveMinutes } from './sustained-kpm';
import { assessCommittee, type IntegrityExpertModel } from './committee';
import type { StatisticalAssessment } from './statistics';
const steady = [1, 2, 3, 4, 61, 62, 63, 64, 121, 122, 123, 124];
test('equal 180-second means differ: one burst does not pass three sustained minutes', () => {
	expect(consecutiveMinutes(steady, 180, 3, 4).passed).toBe(true);
	expect(
		consecutiveMinutes(
			Array.from({ length: 12 }, (_, i) => i + 1),
			180,
			3,
			4
		).passed
	).toBe(false);
	expect(
		consecutiveMinutes(
			steady.filter((t) => t !== 64),
			180,
			3,
			4
		).passed
	).toBe(false);
});
test('completed, adjacent minutes only; half-open boundaries and two minute option', () => {
	expect(consecutiveMinutes(steady, 179, 3, 4).passed).toBe(false);
	expect(consecutiveMinutes(steady, 180, 2, 4).passed).toBe(true);
	expect(consecutiveMinutes([0, 60, 120, 180], 181, 3, 1).windows.map((w) => w.kills)).toEqual([
		1, 1, 1
	]);
	expect(consecutiveMinutes(steady, 240, 3, 4).passed).toBe(false);
});
test('career, tempo and change point cannot bypass missing sustained minutes, even hard evidence', () => {
	const models = ['TEMPO', 'CAREER', 'CHANGE_POINT'].map((family) => ({
		id: family,
		version: 'test',
		evidenceFamily: family,
		assess: () => ({
			modelId: family,
			modelVersion: 'test',
			evidenceFamily: family,
			decision: 'CHEAT_LIKELY',
			confidence: 1,
			evidenceQuality: 1,
			reasons: [],
			evidenceRefs: ['event'],
			hardEvidence: true
		})
	})) as IntegrityExpertModel[];
	const quality = {
		feedHealthy: true,
		backlogSafe: true,
		identityReliable: true,
		roundReliable: true,
		baselineFresh: true,
		versionsMatch: true,
		baselinePopulationAdequate: true
	};
	const input = {
		statistical: {
			sustainedKpm: consecutiveMinutes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], 180, 3, 4)
		} as StatisticalAssessment,
		currentKpm: 4,
		eventIds: ['event'],
		independentEpisodes: 3
	};
	const result = assessCommittee(input, quality, models);
	expect(result.verdicts.every((v) => v.decision === 'UNKNOWN' && !v.hardEvidence)).toBe(true);
	expect(result.decision).toBe('WATCH');
});
