import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import {
	integrityBaselines,
	integrityCases,
	integrityModelState,
	integrityScores,
	kills,
	outbox
} from '$lib/server/db/schema';
import { processIntegrityBatch, resetIntegrityServer } from '$lib/server/integrity/pipeline';
import { loadBaselines } from '$lib/server/integrity/baselines';
import type { StatisticalAssessment } from '$lib/server/integrity/statistics';
import { STATISTICAL_MODEL_CONFIG } from '$lib/server/integrity/statistical-config';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import type { KillView } from '$lib/types';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('Statistical Shadow pipeline', () => {
	test('a below-legacy-threshold rare window records a committee WATCH without punishment', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const steamId = '76561198000007888';
		const received = new Date();
		const eventIds = Array.from({ length: 11 }, (_, i) => `shadow-${world.server.id}-${i}`);
		await env.db.insert(integrityModelState).values({
			orgId: world.org.id,
			activeBaselineGeneration: 'shadow-test',
			baselineStatus: 'READY'
		});
		await env.db.insert(integrityBaselines).values({
			id: `baseline-${world.server.id}`,
			orgId: world.org.id,
			metric: 'kpm180',
			level: 3,
			map: null,
			populationBucket: null,
			weaponCategory: 'INFANTRY',
			sampleCount: 200,
			modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
			featureVersion: STATISTICAL_MODEL_CONFIG.featureVersion,
			weaponMapVersion: 1,
			generation: 'shadow-test',
			median: 1,
			mad: 0.3,
			p90: 1.5,
			p95: 1.7,
			p99: 2,
			p995: 2.1,
			p999: 2.3,
			p9995: 2.4,
			histogram: Array.from({ length: 30 }, (_, i) => ({
				from: i / 10,
				to: (i + 1) / 10,
				count: i === 10 ? 200 : 0
			})),
			cdf: [[1, 200]],
			windowDays: 30,
			calculatedAt: received
		});
		const batch: KillView[] = eventIds.map((eventId, i) => ({
			eventId,
			ts: received.toISOString(),
			instanceId: `instance-${world.server.id}`,
			matchId: 'match',
			map: 'Kavkazi',
			eventTime: 1 + i * 16,
			killer: { steamId, name: 'Candidate', faction: 'Blue' },
			victim: {
				steamId: `7656119800000${String(i + 100).padStart(4, '0')}`,
				name: 'Victim',
				faction: 'Red'
			},
			cause: 'Id.Item.AK74M',
			distanceM: 50,
			headshot: false,
			suicide: false,
			teamKill: false,
			tags: []
		}));
		await env.db.insert(kills).values(
			batch.map((item) => ({
				ts: received,
				serverId: world.server.id,
				eventId: item.eventId,
				instanceId: item.instanceId!,
				matchId: item.matchId!,
				eventTime: item.eventTime,
				map: item.map,
				killerSteamId: steamId,
				killerName: 'Candidate',
				killerFaction: 'Blue',
				victimSteamId: item.victim.steamId,
				victimName: 'Victim',
				victimFaction: 'Red',
				cause: item.cause,
				headshot: false,
				suicide: false,
				teamKill: false,
				tags: []
			}))
		);
		expect(await acquireOrRenew(env, 'statistical shadow test')).toBe(true);
		try {
			expect((await loadBaselines(env, world.org.id, 'Kavkazi', null)).has('kpm180')).toBe(true);
			await processIntegrityBatch(env, world.server.id, batch);
			const [saved] = await env.db
				.select()
				.from(integrityScores)
				.where(eq(integrityScores.steamId, steamId));
			expect(saved.score).toBeLessThan(54);
			expect(saved.level).toBe('NORMAL');
			const assessment = saved.statistical as StatisticalAssessment;
			expect(assessment.status).toBe('READY');
			expect(assessment.level).toBe('WATCH');
			expect(assessment.committee?.decision).toBe('WATCH');
			expect(assessment.tempoPercentile).toBe(1);
			expect(
				await env.db.select().from(integrityCases).where(eq(integrityCases.steamId, steamId))
			).toHaveLength(0);
			expect(await env.db.select().from(outbox).where(eq(outbox.steamId, steamId))).toHaveLength(0);
			const next: KillView = {
				...batch[10],
				eventId: `shadow-${world.server.id}-11`,
				eventTime: 177,
				victim: { ...batch[10].victim, steamId: '76561198000000999' }
			};
			await env.db.insert(kills).values({
				ts: received,
				serverId: world.server.id,
				eventId: next.eventId,
				instanceId: next.instanceId!,
				matchId: next.matchId!,
				eventTime: next.eventTime,
				map: next.map,
				killerSteamId: steamId,
				killerName: 'Candidate',
				killerFaction: 'Blue',
				victimSteamId: next.victim.steamId,
				victimName: 'Victim',
				victimFaction: 'Red',
				cause: next.cause,
				headshot: false,
				suicide: false,
				teamKill: false,
				tags: []
			});
			await processIntegrityBatch(env, world.server.id, [next]);
			expect(
				await env.db.select().from(integrityCases).where(eq(integrityCases.steamId, steamId))
			).toHaveLength(0);
		} finally {
			resetIntegrityServer(world.server.id);
			await releaseOwnership(env);
		}
	});
});
