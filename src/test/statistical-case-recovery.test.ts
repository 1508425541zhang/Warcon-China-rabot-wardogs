import { describe, expect, test } from 'bun:test';
import { desc, eq } from 'drizzle-orm';
import {
	integrityBaselines,
	integrityCases,
	integrityModelState,
	integrityScores,
	kills,
	matches,
	integrityRules,
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
import { DEFAULT_INTEGRITY_RULES } from '$lib/server/integrity/score';

describe.skipIf(!hasTestDb)('Statistical case recovery', () => {
	test('two simultaneous high votes create a case, repair a missing case and do not duplicate it', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const steamId = '76561198000007889';
		const received = new Date();
		await env.db
			.insert(integrityRules)
			.values({
				orgId: world.org.id,
				config: DEFAULT_INTEGRITY_RULES,
				assessmentMode: 'statistical'
			});
		const [round] = await env.db
			.insert(matches)
			.values({
				serverId: world.server.id,
				map: 'Kavkazi',
				startedAt: new Date(received.getTime() - 600000)
			})
			.returning();
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
			matchRow: round.id,
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
			headshot: true,
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
				matchRow: round.id,
				eventTime: item.eventTime,
				map: item.map,
				killerSteamId: steamId,
				killerName: 'Candidate',
				killerFaction: 'Blue',
				victimSteamId: item.victim.steamId,
				victimName: 'Victim',
				victimFaction: 'Red',
				cause: item.cause,
				headshot: true,
				suicide: false,
				teamKill: false,
				tags: []
			}))
		);
		expect(await acquireOrRenew(env, 'case recovery test')).toBe(true);
		try {
			expect((await loadBaselines(env, world.org.id, 'Kavkazi', null)).has('kpm180')).toBe(true);

			await processIntegrityBatch(env, world.server.id, batch.slice(0, 5), false);
			const caseRows = () =>
				env.db.select().from(integrityCases).where(eq(integrityCases.serverId, world.server.id));
			const [first] = await caseRows();
			expect(first).toBeDefined();
			expect((first.statistical as StatisticalAssessment).committee?.cheatVotes).toBe(2);
			expect((first.statistical as StatisticalAssessment).level).toBe('CASE');
			// Simulate an orphaned saved assessment in this disposable test database.
			await env.db.delete(integrityCases).where(eq(integrityCases.id, first.id));
			await processIntegrityBatch(env, world.server.id, batch.slice(5, 6), false);
			const restored = await caseRows();
			expect(restored).toHaveLength(1);
			expect(restored[0].id).not.toBe(first.id);
			await processIntegrityBatch(env, world.server.id, batch.slice(6, 7), false);
			expect(await caseRows()).toHaveLength(1);
			await processIntegrityBatch(env, world.server.id, batch.slice(6, 7), false);
			expect(await caseRows()).toHaveLength(1);
			expect(await env.db.select().from(outbox).where(eq(outbox.steamId, steamId))).toHaveLength(0);
		} finally {
			resetIntegrityServer(world.server.id);
			await releaseOwnership(env);
		}
	});
});
