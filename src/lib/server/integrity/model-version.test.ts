import { describe, expect, test } from 'bun:test';
import { statisticalActionVersionValid } from './enforcement';
import { STATISTICAL_MODEL_CONFIG } from './statistical-config';
import type { StatisticalAssessment } from './statistics';

describe('statistical action generation gate', () => {
	test('weapon mapping, baseline generation and status drift invalidate a frozen candidate', () => {
		const assessment = {
			modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
			featureVersion: STATISTICAL_MODEL_CONFIG.featureVersion,
			weaponMapVersion: 7,
			baselineGeneration: 'generation-1',
			committee: {
				generation: STATISTICAL_MODEL_CONFIG.modelVersion,
				decision: 'KICK_CANDIDATE',
				autoActionBlocked: false
			}
		} as StatisticalAssessment;
		const state = {
			weaponMapVersion: 7,
			activeBaselineGeneration: 'generation-1',
			baselineStatus: 'READY'
		};
		expect(statisticalActionVersionValid(assessment, state)).toBe(true);
		expect(statisticalActionVersionValid(assessment, { ...state, weaponMapVersion: 8 })).toBe(
			false
		);
		expect(
			statisticalActionVersionValid(assessment, {
				...state,
				activeBaselineGeneration: 'generation-2'
			})
		).toBe(false);
		expect(statisticalActionVersionValid(assessment, { ...state, baselineStatus: 'STALE' })).toBe(
			false
		);
	});
});
