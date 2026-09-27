import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import { loadDistributionDashboard } from '$lib/server/integrity/distribution-dashboard';
import { integrityBaselines, integrityModelState } from '$lib/server/db/schema';
import { STATISTICAL_MODEL_CONFIG } from '$lib/server/integrity/statistical-config';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('live distribution dashboard', () => {
	test('follows baseline generations without needing player scores, isolates scope and keeps CDF private', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const now = new Date();
		await env.db
			.insert(integrityModelState)
			.values({
				orgId: world.org.id,
				activeBaselineGeneration: 'first',
				weaponMapVersion: 1,
				baselineStatus: 'READY',
				lastRefreshAt: now
			})
			.onConflictDoUpdate({
				target: integrityModelState.orgId,
				set: {
					activeBaselineGeneration: 'first',
					weaponMapVersion: 1,
					baselineStatus: 'READY',
					lastRefreshAt: now
				}
			});
		const baseline = {
			orgId: world.org.id,
			metric: 'kpm180',
			level: 3,
			weaponCategory: 'INFANTRY',
			sampleCount: 100,
			uniquePlayers: 20,
			uniquePlayerDays: 20,
			modelVersion: STATISTICAL_MODEL_CONFIG.modelVersion,
			featureVersion: STATISTICAL_MODEL_CONFIG.featureVersion,
			weaponMapVersion: 1,
			generation: 'first',
			median: 1,
			mad: 1,
			p90: 2,
			p95: 3,
			p99: 4,
			p995: 4,
			p999: 5,
			p9995: 5,
			histogram: [{ from: 0, to: 5, count: 20 }],
			cdf: [
				[1, 10],
				[5, 10]
			],
			calculatedAt: now
		};
		await env.db.insert(integrityBaselines).values([
			{ ...baseline, id: 'dashboard-current' },
			{ ...baseline, id: 'dashboard-next', generation: 'second', sampleCount: 150 },
			{ ...baseline, id: 'dashboard-other-org', orgId: world.otherOrg.id },
			{
				...baseline,
				id: 'dashboard-other-server',
				serverId: world.otherServer.id,
				metric: 'headshotRate'
			},
			{ ...baseline, id: 'dashboard-old-model', modelVersion: 'retired' },
			{ ...baseline, id: 'dashboard-old-weapon-map', weaponMapVersion: 0 },
			{ ...baseline, id: 'dashboard-subcohort', level: 2, populationBucket: '81+' },
			{
				...baseline,
				id: 'dashboard-small',
				metric: 'maxKillDistanceWeapon',
				weaponCategory: 'Id.Item.AK74M',
				sampleCount: 34
			}
		]);
		const first = await loadDistributionDashboard(env, world.org.id, world.server.id);
		expect(first.metrics.map((m) => m.baselineId)).toEqual([
			'dashboard-current',
			'dashboard-small'
		]);
		expect(first.metrics[0].sampleCount).toBe(100);
		expect(first.metrics[0].value).toBeNull();
		expect(first.metrics[0].extremenessPercentile).toBeNull();
		expect(JSON.stringify(first)).not.toContain('"cdf"');
		expect(first.status).toBe('READY');
		expect(first.dataBefore).toBe(new Date(now.getTime() - 600_000).toISOString());
		await env.db
			.update(integrityModelState)
			.set({ activeBaselineGeneration: 'second' })
			.where(eq(integrityModelState.orgId, world.org.id));
		const second = await loadDistributionDashboard(env, world.org.id, world.server.id);
		expect(second.metrics).toHaveLength(1);
		expect(second.metrics[0].sampleCount).toBe(150);
		expect(first.metrics[0].sampleCount).toBe(100);
		await env.db
			.update(integrityModelState)
			.set({ lastRefreshAt: new Date(now.getTime() - 49 * 3_600_000) })
			.where(eq(integrityModelState.orgId, world.org.id));
		expect((await loadDistributionDashboard(env, world.org.id, world.server.id)).status).toBe(
			'STALE'
		);
	});
});
