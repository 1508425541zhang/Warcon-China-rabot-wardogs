import { consecutiveMinutes } from '$lib/server/integrity/sustained-kpm';
import { describe, expect, test } from 'bun:test';
import { decideStatisticalAction, DEFAULT_ENFORCEMENT } from './decisions';
import { DEFAULT_INTEGRITY_RULES } from './score';
import { maxKillsWithin, medianKillInterval, type BehaviorFinding } from './windows';
import {
	assessDistribution,
	extremenessPercentile,
	percentilePosition,
	populationBucket,
	robustZ,
	sampleQuality,
	type DistributionStats
} from './statistics';

const distribution = (
	metric: DistributionStats['metric'],
	cdf: [number, number][]
): DistributionStats => ({
	id: metric,
	source: 'local',
	metric,
	map: null,
	populationBucket: null,
	weaponCategory: 'INFANTRY',
	sampleCount: cdf.reduce((sum, [, n]) => sum + n, 0),
	uniquePlayers: 100,
	uniquePlayerDays: 100,
	effectiveSampleSize: 1000,
	median: 5,
	mad: 2,
	p90: 8,
	p95: 9,
	p99: 10,
	p995: 11,
	p999: 12,
	p9995: 13,
	histogram: [{ from: 0, to: 20, count: 1000 }],
	cdf,
	windowDays: 30,
	calculatedAt: new Date()
});
const ranking = Array.from({ length: 1000 }, (_, i) => [i + 1, 1] as [number, number]);

describe('empirical Integrity statistics', () => {
	test('population buckets have exact boundaries and unknown is not guessed', () => {
		expect([null, 0, 1, 20, 21, 40, 41, 60, 61, 80, 81].map(populationBucket)).toEqual([
			null,
			null,
			'1–20',
			'1–20',
			'21–40',
			'21–40',
			'41–60',
			'41–60',
			'61–80',
			'61–80',
			'81+'
		]);
	});
	test('empirical rank and upper/lower tails move in opposite directions', () => {
		const high = percentilePosition(ranking, 999);
		const low = percentilePosition(ranking, 2);
		expect(high).toBeCloseTo(0.9985);
		expect(low).toBeCloseTo(0.0015);
		expect(extremenessPercentile(high, 'upper')).toBeGreaterThan(
			extremenessPercentile(low, 'upper')
		);
		expect(extremenessPercentile(low, 'lower')).toBeGreaterThan(
			extremenessPercentile(high, 'lower')
		);
	});
	test('MAD is explanatory and a zero MAD has no invented z-score', () => {
		expect(robustZ(9, 5, 2)).toBeCloseTo(1.349);
		expect(robustZ(9, 5, 0)).toBeNull();
		expect(sampleQuality(49)).toBe('INSUFFICIENT_DATA');
		expect(sampleQuality(199)).toBe('LOW_SAMPLE');
		expect(sampleQuality(200)).toBe('LOW_SAMPLE');
		expect(sampleQuality(1000)).toBe('NORMAL_SAMPLE');
		expect(sampleQuality(5000)).toBe('HIGH_SAMPLE');
	});
	test('Tempo and Precision take family maxima, not additive points; five precision kills do not count', () => {
		const baselines = new Map([
			['kpm180', distribution('kpm180', ranking)],
			['maxKills15s', distribution('maxKills15s', ranking)],
			[
				'headshotRate',
				distribution(
					'headshotRate',
					Array.from({ length: 1000 }, (_, i) => [i / 1000, 1])
				)
			]
		] as const);
		const small = assessDistribution(
			{ kpm180: 999, maxKills15s: 998, headshotRate: 0.9 },
			baselines,
			5,
			1
		);
		expect(small.tempoPercentile).toBeCloseTo(0.9985);
		expect(small.precisionPercentile).toBeNull();
		const enough = assessDistribution(
			{ kpm180: 999, maxKills15s: 998, headshotRate: 0.9 },
			baselines,
			30,
			1
		);
		expect(enough.precisionPercentile).toBeGreaterThan(0.8);
		expect(enough.metrics).toHaveLength(3);
	});
	test('fifteen-second burst and median game-clock interval are raw measurements', () => {
		const entries = [100, 104, 109, 111, 120].map((clock) => ({ clock }));
		expect(medianKillInterval(entries)).toBe(4.5);
		expect(maxKillsWithin(entries, 15)).toBe(4);
	});
	test('a weapon is compared only with its own headshot and distance history', () => {
		const rifle = 'Id.Item.AK74M';
		const weaponBaselines = new Map([
			[
				`headshotRateWeapon:${rifle}`,
				{
					...distribution(
						'headshotRateWeapon',
						Array.from({ length: 1000 }, (_, i) => [i / 1000, 1])
					),
					weaponCategory: rifle
				}
			],
			[
				`maxKillDistanceWeapon:${rifle}`,
				{ ...distribution('maxKillDistanceWeapon', ranking), weaponCategory: rifle }
			]
		] as const);
		const result = assessDistribution(
			{},
			new Map(),
			13,
			1,
			[
				{ cause: rifle, kills: 10, headshots: 10, maxKillDistanceM: 1200 },
				{ cause: 'Id.Item.SVDM', kills: 3, headshots: 3, maxKillDistanceM: 3000 }
			],
			weaponBaselines
		);
		expect(result.metrics.map((metric) => metric.code)).toEqual([
			'headshotRateWeapon',
			'maxKillDistanceWeapon'
		]);
		expect(result.metrics.every((metric) => metric.weaponCategory === rifle)).toBe(true);
		expect(result.level).toBe('CASE');
	});
	test('single maximum kill distance remains review-only even with 5000 baseline windows', () => {
		const rifle = 'Id.Item.AK74M';
		const large = Array.from({ length: 5000 }, (_, i) => [i + 1, 1] as [number, number]);
		const result = assessDistribution(
			{ kpm180: 6000 },
			new Map([['kpm180', distribution('kpm180', large)]]),
			24,
			1,
			[{ cause: rifle, kills: 3, headshots: 0, maxKillDistanceM: 6000 }],
			new Map([
				[
					`maxKillDistanceWeapon:${rifle}`,
					{ ...distribution('maxKillDistanceWeapon', large), weaponCategory: rifle }
				]
			])
		);
		expect(result.precisionPercentile).toBe(1);
		expect(result.actionPrecisionPercentile).toBeNull();
		expect(result.level).toBe('CASE');
	});
});

const finding: BehaviorFinding = {
	steamId: '76561198000000001',
	roundId: 'i-1:derived:1',
	instanceId: 'i',
	map: 'Kavkazi',
	anchorClock: 1,
	windowId: 1,
	clockFrom: 0,
	clockTo: 100,
	infantryKills: 24,
	kpm180: 8,
	uniqueVictims: 20,
	headshots: 20,
	headshotPct: 83,
	penetrations: 0,
	penetrationPct: 0,
	burstPoints: 12,
	maxKills15s: 8,
	medianKillInterval: 2,
	reasons: ['kpm', 'burst', 'headshot'],
	eventIds: ['e1']
};
test('statistical decision has independent evidence, live gates and a hard safety floor', () => {
	const enoughHistory = Array.from({ length: 5000 }, (_, i) => [i + 1, 1] as [number, number]);
	const assessment = assessDistribution(
		{ kpm180: 5000, headshotRate: 0.9998 },
		new Map([
			['kpm180', distribution('kpm180', enoughHistory)],
			[
				'headshotRate',
				distribution(
					'headshotRate',
					Array.from({ length: 5000 }, (_, i) => [i / 5000, 1])
				)
			]
		]),
		24,
		1
	);
	assessment.sustainedKpm = consecutiveMinutes(
		[1, 2, 3, 4, 61, 62, 63, 64, 121, 122, 123, 124],
		180,
		3,
		4
	);
	assessment.committee = { decision: 'KICK_CANDIDATE', autoActionBlocked: false } as NonNullable<
		typeof assessment.committee
	>;
	const input = {
		assessment,
		finding,
		settings: { ...DEFAULT_ENFORCEMENT, autoKickEnabled: true },
		confidence: 'B' as const,
		feedHealthy: true,
		playerOnline: true,
		onlinePlayers: 20,
		rules: DEFAULT_INTEGRITY_RULES,
		identityReliable: true,
		priorIndependentWindow: false,
		previousActions: [] as const
	};
	expect(assessment.level).toBe('KICK_CANDIDATE');
	expect(decideStatisticalAction(input)).toBe('KICK');
	expect(decideStatisticalAction({ ...input, onlinePlayers: 19 })).toBe('OBSERVE');
	expect(
		decideStatisticalAction({
			...input,
			assessment: { ...assessment, independentEpisodes: 2 },
			previousActions: ['KICK'],
			settings: { ...input.settings, autoQuarantine24hEnabled: true }
		})
	).toBe('QUARANTINE_24H');
	expect(
		decideStatisticalAction({
			...input,
			assessment: { ...assessment, independentEpisodes: 3 },
			previousActions: ['QUARANTINE_24H'],
			settings: { ...input.settings, autoQuarantine7dEnabled: true }
		})
	).toBe('QUARANTINE_7D');
	expect(
		decideStatisticalAction({
			...input,
			finding: { ...finding, kpm180: DEFAULT_INTEGRITY_RULES.kpmBands[0].min }
		})
	).toBe('KICK');
	expect(
		decideStatisticalAction({
			...input,
			assessment: {
				...assessment,
				sustainedKpm: consecutiveMinutes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], 180, 3, 4)
			}
		})
	).toBe('OBSERVE');
	expect(decideStatisticalAction({ ...input, feedHealthy: false })).toBe('OBSERVE');
	expect(
		decideStatisticalAction({ ...input, assessment: { ...assessment, sampleCount: 199 } })
	).toBe('OBSERVE');
});
