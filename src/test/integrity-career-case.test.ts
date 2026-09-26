import { describe, expect, test } from 'bun:test';
import {
	integrityCaseEvents,
	integrityCases,
	integrityPlayerMetricHistory
} from '$lib/server/db/schema';
import { loadCleanCareerContext } from '$lib/server/integrity/career-context';
import { STATISTICAL_MODEL_CONFIG } from '$lib/server/integrity/statistical-config';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('career selection after a case', () => {
	test('keeps clean history on either side of the case episode', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const steamId = '76561198000004567';
		const at = new Date(Date.now() - 12 * 3_600_000);
		const observations = [
			new Date(at.getTime() - 3_600_000),
			at,
			new Date(at.getTime() + 3_600_000)
		];
		await env.db.insert(integrityPlayerMetricHistory).values(
			observations.map((observedAt, i) => ({
				orgId: world.org.id,
				steamId,
				serverId: world.server.id,
				roundId: `round-${i}`,
				eventId: `career-${i}`,
				observedAt,
				kpm180: i + 1,
				headshotRate: 0.2,
				maxKills15s: i + 1,
				featureVersion: STATISTICAL_MODEL_CONFIG.featureVersion,
				modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion
			}))
		);
		const caseId = crypto.randomUUID();
		await env.db.insert(integrityCases).values({
			id: caseId,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId,
			createdAt: at,
			confidence: 'B',
			trigger: 'ABNORMAL_INFANTRY_WINDOW',
			ruleVersion: 1,
			riskScore: 70,
			riskBreakdown: [],
			snapshot: {}
		});
		await env.db.insert(integrityCaseEvents).values({
			caseId,
			instanceId: 'boot',
			eventId: 'career-1',
			event: { killerSteamId: steamId, ts: at.toISOString() }
		});
		const career = await loadCleanCareerContext(env.db, world.org.id, steamId, new Date());
		expect(career?.sampleCount).toBe(2);
		expect(career?.orderedKpm).toEqual([1, 3]);
	});
});
