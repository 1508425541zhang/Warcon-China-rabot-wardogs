import { describe, expect, test } from 'bun:test';
import { scoreIntegrity, type IntegritySignals } from './score';
import { validateIntegrityRules } from './rules';

const normal: IntegritySignals = {
	kpm180: 3.99,
	uniqueVictims: 7,
	previousKpm: [],
	uniqueReporters: 0,
	repeatAutoKo: false,
	infantryKills: 11,
	headshots: 11,
	penetrations: 11,
	burstPoints: 0,
	vacBans: 0,
	gameBans: 0,
	daysSinceLastBan: null,
	wardogsPlaytimeHours: null
};

describe('explainable Integrity score', () => {
	test('below 4 KPM is normal even with high headshot share on a small sample', () => {
		const score = scoreIntegrity(normal);
		expect(score.score).toBe(0);
		expect(score.level).toBe('NORMAL');
		expect(score.currentBehaviorAnomaly).toBe(false);
	});

	test('KPM bands are exclusive and award only the highest matching tier', () => {
		for (const [kpm180, points] of [
			[3.99, 0],
			[4, 18],
			[4.49, 18],
			[4.5, 24],
			[4.99, 24],
			[5, 32],
			[5.99, 32],
			[6, 42],
			[7.99, 42],
			[8, 52]
		] as const) {
			const score = scoreIntegrity({ ...normal, kpm180 });
			expect(score.score).toBe(points);
		}
	});

	test('KPM bands, unique victims and independent windows are counted once', () => {
		const score = scoreIntegrity({ ...normal, kpm180: 5, uniqueVictims: 12, previousKpm: [5] });
		expect(score.score).toBe(32 + 6 + 8 + 8);
		expect(score.breakdown.map((component) => component.code)).toEqual([
			'infantry_kpm_180',
			'unique_victims',
			'repeat_window',
			'repeat_extreme'
		]);
		expect(score.level).toBe('AUTO_KO');
	});

	test('reports and Steam history alone never supply a current behavior anomaly', () => {
		const score = scoreIntegrity({
			...normal,
			uniqueReporters: 20,
			vacBans: 10,
			gameBans: 10,
			daysSinceLastBan: 1,
			repeatAutoKo: true
		});
		expect(score.breakdown.find((component) => component.code === 'steam_ban_prior')?.points).toBe(
			15
		);
		expect(score.currentBehaviorAnomaly).toBe(false);
		expect(score.score).toBe(16 + 15 + 10);
	});

	test('an ancient single VAC has minimal weight; private playtime is zero weight', () => {
		const score = scoreIntegrity({ ...normal, vacBans: 1, daysSinceLastBan: 3650 });
		expect(score.score).toBe(1);
		expect(score.breakdown.some((component) => component.code === 'low_playtime')).toBe(false);
	});

	test('extreme multi-victim behavior reaches quarantine eligibility, not an action', () => {
		const score = scoreIntegrity({
			...normal,
			kpm180: 8,
			uniqueVictims: 18,
			uniqueReporters: 5,
			infantryKills: 24
		});
		expect(score.score).toBe(70);
		expect(score.level).toBe('AUTO_QUARANTINE_ELIGIBLE');
		expect(score.currentBehaviorAnomaly).toBe(true);
	});

	test('the rule editor rejects overlapping thresholds and early enforcement', () => {
		expect(() => validateIntegrityRules({ koThreshold: 20 })).toThrow();
		expect(() => validateIntegrityRules({ mode: 'enforce' })).toThrow();
		expect(() =>
			validateIntegrityRules({
				kpmBands: [
					{ min: 4, points: 18 },
					{ min: 4, points: 24 }
				]
			})
		).toThrow();
	});
});
