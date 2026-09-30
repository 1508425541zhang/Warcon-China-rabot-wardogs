import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import {
	integrityBaselines,
	integrityCases,
	integrityModelState,
	integrityScores,
	integrityAiJobs,
	integrityAiSettings,
	integrityActions,
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
import { getIntegrityRules } from '$lib/server/integrity/rules';
import { saveAiSettings, aiCall } from '$lib/server/integrity/ai';
import { discoverAiJobs, processNextAiJob } from '$lib/server/integrity/ai-queue';

describe.skipIf(!hasTestDb)('AI low-signal pipeline', () => {
	test('enabled AI no longer creates low-signal cases or automatic jobs', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		await getIntegrityRules(env, world.org.id);
		const steamId = '76561198000007890';
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
			// A single expert is not WATCH yet, but must survive storage for later persistence.
			await processIntegrityBatch(env, world.server.id, batch.slice(0, 4));
			const firstScores = await env.db
				.select()
				.from(integrityScores)
				.where(eq(integrityScores.steamId, steamId));
			expect(firstScores.length).toBeGreaterThan(0);
			const first = firstScores.at(-1)!.statistical as StatisticalAssessment;
			expect(first.committee?.decision).toBe('NORMAL');
			expect(first.committee?.verdicts.find((v) => v.modelId === 'tempo')?.decision).toBe(
				'CHEAT_LIKELY'
			);
			expect(
				await env.db.select().from(integrityCases).where(eq(integrityCases.steamId, steamId))
			).toHaveLength(0);
			expect(await env.db.select().from(outbox).where(eq(outbox.steamId, steamId))).toHaveLength(0);

			await saveAiSettings(env, world.org.id, {
				baseUrl: 'https://example.com/v1',
				apiKey: 'test-secret',
				model: 'test-model'
			});
			await processIntegrityBatch(env, world.server.id, batch.slice(4, 5));
			const caseRows = () =>
				env.db.select().from(integrityCases).where(eq(integrityCases.serverId, world.server.id));
			expect(await caseRows()).toHaveLength(0);
			await discoverAiJobs(env);
			expect(await env.db.select().from(integrityAiJobs)).toHaveLength(0);
			expect(
				await processNextAiJob(env, async () => {
					throw Error('no low-signal provider calls');
				})
			).toBe(false);
			expect(await env.db.select().from(integrityActions)).toHaveLength(0);
		} finally {
			resetIntegrityServer(world.server.id);
			await releaseOwnership(env);
		}
	});
});
