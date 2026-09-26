import { expect, test } from 'bun:test';
import { actionBaselineEligible } from './statistical-config';

test('operational baselines accept a balanced 200-window cohort but reject concentration and external evidence', () => {
	const metric = {
		source: 'local',
		code: 'kpm180',
		sampleCount: 200,
		uniquePlayers: 20,
		uniquePlayerDays: 20,
		effectiveSampleSize: 100
	};
	expect(actionBaselineEligible(metric)).toBe(true);
	for (const patch of [
		{ sampleCount: 199 },
		{ uniquePlayers: 1 },
		{ effectiveSampleSize: 99 },
		{ source: 'external' },
		{ code: 'maxKillDistanceWeapon' }
	])
		expect(actionBaselineEligible({ ...metric, ...patch })).toBe(false);
});
