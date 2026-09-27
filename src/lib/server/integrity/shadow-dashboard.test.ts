import { COMMITTEE_VOTING_VERSION } from './committee';
import { describe, expect, test } from 'bun:test';
import { summarizeCommitteeShadow } from './shadow-dashboard';
import { STATISTICAL_MODEL_CONFIG } from './statistical-config';

describe('committee shadow calibration summary', () => {
	test('counts one latest vote per episode and latest human label per candidate case', () => {
		const watch = {
			modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
			committee: {
				votingVersion: COMMITTEE_VOTING_VERSION,
				decision: 'WATCH',
				verdicts: [
					{ modelId: 'tempo', decision: 'SUSPICIOUS' },
					{ modelId: 'precision', decision: 'NORMAL' }
				]
			}
		};
		const kick = {
			modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
			committee: {
				votingVersion: COMMITTEE_VOTING_VERSION,
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

test('unknown reasons remain visible and are deduplicated per episode', () => {
	const row = {
		windowId: 7,
		statistical: {
			modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
			committee: {
				votingVersion: COMMITTEE_VOTING_VERSION,
				decision: 'WATCH',
				verdicts: [
					{ modelId: 'precision', decision: 'UNKNOWN', reasons: ['NO_CLEAN_PRECISION_BASELINE'] }
				]
			}
		}
	};
	const result = summarizeCommitteeShadow([row, row], [], false);
	expect(result.models.precision.UNKNOWN).toBe(1);
	expect(result.unknownReasons.precision.NO_CLEAN_PRECISION_BASELINE).toBe(1);
});

test('old rule reasons cannot appear as current model votes or review outcomes', () => {
	const old = {
		modelVersion: 'ensemble-operational-v2',
		committee: {
			votingVersion: COMMITTEE_VOTING_VERSION,
			decision: 'KICK_CANDIDATE',
			verdicts: [
				{ modelId: 'tempo', decision: 'UNKNOWN', reasons: ['CONSECUTIVE_60S_KPM_NOT_MET'] }
			]
		}
	};
	const current = {
		...old,
		modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
		committee: {
			votingVersion: COMMITTEE_VOTING_VERSION,
			decision: 'WATCH',
			verdicts: [{ modelId: 'tempo', decision: 'SUSPICIOUS' }]
		}
	};
	const result = summarizeCommitteeShadow(
		[
			{ windowId: 1, statistical: old },
			{ windowId: 1, statistical: current }
		],
		[{ caseId: 'old-case', label: 'CONFIRMED_ABUSE', statistical: old }],
		false
	);
	expect(result.assessed).toBe(1);
	expect(result.counts.WATCH).toBe(1);
	expect(result.unknownReasons).toEqual({});
	expect(result.confirmedAbuse).toBe(0);
});

test('previous voting policy is excluded even when the statistical model is unchanged', () => {
	const old = {
		modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
		committee: { decision: 'WATCH', verdicts: [{ modelId: 'tempo', decision: 'CHEAT_LIKELY' }] }
	};
	const result = summarizeCommitteeShadow([{ windowId: 99, statistical: old }], [], false);
	expect(result.assessed).toBe(0);
	expect(result.models).toEqual({});
});
