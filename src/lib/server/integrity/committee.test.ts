import { consecutiveMinutes } from '$lib/server/integrity/sustained-kpm';
import { describe, expect, test } from 'bun:test';
import {
	dataQualityVeto,
	EXPERT_MODELS,
	assessCommittee,
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
	test('P90 is suspicious, P95 is high, without a consecutive-KPM gate', () => {
		const expert = EXPERT_MODELS.find((m) => m.id === 'tempo')!;
		for (const [p, decision] of [
			[0.899, 'NORMAL'],
			[0.9, 'SUSPICIOUS'],
			[0.949, 'SUSPICIOUS'],
			[0.95, 'CHEAT_LIKELY']
		] as const) {
			const result = expert.assess({
				statistical: {
					metrics: [{ source: 'local', code: 'kpm180', sampleCount: 200, extremenessPercentile: p }]
				} as StatisticalAssessment,
				currentKpm: 1,
				independentEpisodes: 0,
				eventIds: ['e']
			});
			expect(result.decision).toBe(decision);
			expect(result.hardEvidence).not.toBe(true);
		}
	});
	test('five ballots: two positive watch, three high or four positive create review cases only', () => {
		expect(voteCommittee([v('TEMPO', 'SUSPICIOUS'), v('CAREER', 'SUSPICIOUS')]).decision).toBe(
			'WATCH'
		);
		expect(
			voteCommittee([
				v('TEMPO', 'CHEAT_LIKELY'),
				v('CAREER', 'CHEAT_LIKELY'),
				v('CHANGE_POINT', 'CHEAT_LIKELY')
			]).decision
		).toBe('CASE');
		expect(
			voteCommittee([
				v('TEMPO', 'SUSPICIOUS'),
				v('CAREER', 'SUSPICIOUS'),
				v('CHANGE_POINT', 'SUSPICIOUS'),
				v('PRECISION', 'SUSPICIOUS')
			]).decision
		).toBe('CASE');
		expect(voteCommittee([v('TEMPO', 'CHEAT_LIKELY'), v('CAREER', 'CHEAT_LIKELY')]).decision).toBe(
			'WATCH'
		);
		expect(voteCommittee([v('TEMPO', 'CHEAT_LIKELY'), v('TEMPO', 'CHEAT_LIKELY')]).cheatVotes).toBe(
			1
		);
	});
	test('single extraordinary or undocumented signal is not a direct kick', () => {
		expect(
			voteCommittee([{ ...v('TEMPO', 'CHEAT_LIKELY'), hardEvidence: true }]).decision
		).not.toBe('KICK_CANDIDATE');
	});
	test('unknowns do not manufacture positive votes', () => {
		const r = voteCommittee([v('CAREER', 'UNKNOWN'), v('PRECISION', 'UNKNOWN')]);
		expect(r.unknownVotes).toBe(2);
		expect(r.cheatVotes + r.suspiciousVotes + r.normalVotes).toBe(0);
	});
	test('data quality veto is retained for review cases', () => {
		const r = voteCommittee(
			[v('TEMPO', 'CHEAT_LIKELY'), v('CAREER', 'CHEAT_LIKELY'), v('CHANGE_POINT', 'CHEAT_LIKELY')],
			['FEED_STALE']
		);
		expect(r.decision).toBe('CASE');
		expect(r.autoActionBlocked).toBe(true);
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
				sustainedKpm: consecutiveMinutes(
					[1, 2, 3, 4, 61, 62, 63, 64, 121, 122, 123, 124],
					180,
					3,
					4
				),
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

test('KPM bypass requires >4 and a non-tempo expert; absence of KPM never stops five ballots', () => {
	const quality = {
		feedHealthy: true,
		backlogSafe: true,
		identityReliable: true,
		roundReliable: true,
		baselineFresh: true,
		versionsMatch: true,
		baselinePopulationAdequate: true
	};
	const ids = ['tempo', 'precision', 'career_deviation', 'change_point', 'persistence'];
	const models = ids.map((id, i) => ({
		id,
		version: 'test',
		evidenceFamily: 'TEMPO' as const,
		assess: () => v('TEMPO', i === 1 ? 'SUSPICIOUS' : 'NORMAL', id)
	}));
	const input = {
		statistical: { status: 'READY', metrics: [] } as unknown as StatisticalAssessment,
		currentKpm: 4,
		independentEpisodes: 0,
		eventIds: ['e']
	};
	expect(assessCommittee(input, quality, models).decision).not.toBe('KICK_CANDIDATE');
	expect(assessCommittee({ ...input, currentKpm: 4.01 }, quality, models).decision).toBe(
		'KICK_CANDIDATE'
	);
	const three = models.map((m, i) => ({
		...m,
		assess: () => v('TEMPO', i < 3 ? 'CHEAT_LIKELY' : 'NORMAL', m.id)
	}));
	expect(assessCommittee({ ...input, currentKpm: 1 }, quality, three).decision).toBe('CASE');
	expect(
		assessCommittee({ ...input, currentKpm: 4.01 }, { ...quality, feedHealthy: false }, models)
			.autoActionBlocked
	).toBe(true);
});
