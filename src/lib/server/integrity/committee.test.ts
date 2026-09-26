import { describe, expect, test } from 'bun:test';
import {
	dataQualityVeto,
	EXPERT_MODELS,
	hasStatisticalAnomaly,
	voteCommittee,
	type EvidenceFamily,
	type ExpertDecision,
	type ExpertVerdict
} from './committee';
import type { StatisticalAssessment } from './statistics';
import { STATISTICAL_MODEL_CONFIG } from './statistical-config';

const v = (
	family: EvidenceFamily,
	decision: ExpertDecision,
	modelId: string = family
): ExpertVerdict => ({
	modelId,
	modelVersion: '1',
	evidenceFamily: family,
	decision,
	confidence: 1,
	evidenceQuality: 1,
	reasons: [],
	evidenceRefs: ['event-1']
});

describe('independent expert committee', () => {
	test('50-sample preliminary review participates but cannot claim a P99 extreme or hard evidence', () => {
		const tempo = EXPERT_MODELS.find((item) => item.id === 'tempo')!;
		const result = tempo.assess({
			statistical: {
				metrics: [
					{ source: 'local', code: 'kpm180', value: 8, sampleCount: 50, extremenessPercentile: 1 }
				]
			} as StatisticalAssessment,
			currentKpm: 8,
			independentEpisodes: 1,
			eventIds: Array.from({ length: 24 }, (_, i) => `e-${i}`)
		});
		expect(result.decision).toBe('SUSPICIOUS');
		expect(result.hardEvidence).toBe(false);
	});
	test('two independent CHEAT votes qualify for a Kick candidate', () => {
		expect(
			voteCommittee([v('TEMPO', 'CHEAT_LIKELY'), v('PRECISION', 'CHEAT_LIKELY')]).decision
		).toBe('KICK_CANDIDATE');
	});
	test('one CHEAT plus two independent SUSPICIOUS votes qualifies', () => {
		expect(
			voteCommittee([
				v('TEMPO', 'CHEAT_LIKELY'),
				v('PRECISION', 'SUSPICIOUS'),
				v('CAREER', 'SUSPICIOUS')
			]).decision
		).toBe('KICK_CANDIDATE');
	});
	test('career does not create an independent third vote from the same tempo', () => {
		expect(
			voteCommittee([
				v('TEMPO', 'SUSPICIOUS'),
				v('PRECISION', 'SUSPICIOUS'),
				v('CAREER', 'SUSPICIOUS')
			]).decision
		).toBe('WATCH');
	});
	test('three correlated tempo models count as one vote', () => {
		const result = voteCommittee([
			v('TEMPO', 'SUSPICIOUS', 'kpm'),
			v('TEMPO', 'SUSPICIOUS', 'burst'),
			v('TEMPO', 'SUSPICIOUS', 'interval')
		]);
		expect(result.decision).toBe('WATCH');
		expect(result.independentSuspiciousFamilies).toBe(1);
	});
	test('one ordinary CHEAT vote does not qualify', () => {
		expect(voteCommittee([v('TEMPO', 'CHEAT_LIKELY')]).decision).toBe('WATCH');
	});
	test('one verified hard-evidence vote qualifies, while weak or undocumented claims do not', () => {
		const strong = { ...v('PRECISION', 'CHEAT_LIKELY'), hardEvidence: true, confidence: 0.999 };
		expect(voteCommittee([strong]).decision).toBe('KICK_CANDIDATE');
		expect(voteCommittee([{ ...strong, evidenceQuality: 0.8 }]).decision).toBe('WATCH');
		expect(voteCommittee([{ ...strong, evidenceRefs: [] }]).decision).toBe('WATCH');
	});
	test('tempo marks only an extraordinary clean-baseline KPM window as hard evidence', () => {
		const tempo = EXPERT_MODELS.find((item) => item.id === 'tempo')!;
		const statistical = (value: number, percentile: number) =>
			({
				metrics: [
					{
						code: 'kpm180',
						source: 'local',
						sampleCount: 10_000,
						uniquePlayers: 100,
						uniquePlayerDays: 100,
						effectiveSampleSize: 1000,
						value,
						extremenessPercentile: percentile
					}
				]
			}) as StatisticalAssessment;
		const input = {
			statistical: statistical(8, 0.9999),
			independentEpisodes: 1,
			currentKpm: 8,
			eventIds: Array.from({ length: 24 }, (_, i) => `e-${i}`)
		};
		expect(tempo.assess(input).hardEvidence).toBe(true);
		expect(tempo.assess({ ...input, statistical: statistical(7.9, 0.9999) }).hardEvidence).toBe(
			false
		);
	});
	test('data quality veto blocks action despite unanimous CHEAT votes', () => {
		const result = voteCommittee(
			[v('TEMPO', 'CHEAT_LIKELY'), v('PRECISION', 'CHEAT_LIKELY')],
			['FEED_STALE']
		);
		expect(result.decision).toBe('KICK_CANDIDATE');
		expect(result.autoActionBlocked).toBe(true);
		expect(result.vetoReasons).toContain('FEED_STALE');
	});
	test('UNKNOWN is never counted as NORMAL', () => {
		const result = voteCommittee([v('CAREER', 'UNKNOWN')]);
		expect(result.unknownVotes).toBe(1);
		expect(result.normalVotes).toBe(0);
		expect(result.decision).toBe('WATCH');
	});
	test('missing identity and stale baseline are separate veto reasons', () => {
		expect(
			dataQualityVeto({
				feedHealthy: true,
				backlogSafe: true,
				identityReliable: false,
				roundReliable: true,
				baselineFresh: false,
				versionsMatch: true,
				baselinePopulationAdequate: true
			})
		).toEqual(['IDENTITYRELIABLE', 'BASELINEFRESH']);
	});
	test('persistence ignores ordinary and distance-only prior windows', () => {
		const assessment = (code: string, percentile: number) =>
			({
				status: 'READY',
				modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
				featureVersion: STATISTICAL_MODEL_CONFIG.featureVersion,
				metrics: [{ code, extremenessPercentile: percentile }]
			}) as StatisticalAssessment;
		expect(hasStatisticalAnomaly(null)).toBe(false);
		expect(hasStatisticalAnomaly(assessment('kpm180', 0.5))).toBe(false);
		expect(hasStatisticalAnomaly(assessment('maxKillDistanceWeapon', 1))).toBe(false);
		expect(hasStatisticalAnomaly(assessment('kpm180', 0.995))).toBe(true);
	});
});
