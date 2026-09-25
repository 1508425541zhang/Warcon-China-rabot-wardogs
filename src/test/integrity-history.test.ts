import { beforeAll, describe, expect, test } from 'bun:test';
import type { Env } from '$lib/server/env';
import { integrityScores, integrityWindows } from '$lib/server/db/schema';
import { hadRecentHighRiskWindow } from '$lib/server/integrity/history';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

describe.skipIf(!hasTestDb)('recorded high-risk window history', () => {
	let env: Env;
	let world: World;
	const steamId = '76561198000000666';
	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
	});

	test('uses a prior saved high-risk level within the review period and excludes the current window', async () => {
		const now = new Date();
		const [old, current] = await env.db
			.insert(integrityWindows)
			.values(
				[1, 2].map((n) => ({
					orgId: world.org.id,
					serverId: world.server.id,
					steamId,
					instanceId: 'boot',
					map: 'map',
					clockFrom: n,
					clockTo: n,
					observedAt: now,
					infantryKills: 12,
					kpm180: 4,
					uniqueVictims: 12,
					eventIds: [`window-${n}`]
				}))
			)
			.returning({ id: integrityWindows.id });
		await env.db.insert(integrityScores).values({
			windowId: old.id,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId,
			scoredAt: new Date(now.getTime() - 2 * 3600_000),
			ruleVersion: 1,
			score: 54,
			level: 'AUTO_KO',
			breakdown: [],
			currentBehaviorAnomaly: true
		});
		expect(
			await hadRecentHighRiskWindow(env.db, world.org.id, steamId, now, 24, current.id, ['window-2'])
		).toBe(true);
		expect(
			await hadRecentHighRiskWindow(env.db, world.org.id, steamId, now, 1, current.id, ['window-2'])
		).toBe(false);
		expect(
			await hadRecentHighRiskWindow(env.db, world.org.id, steamId, now, 24, old.id, ['window-1'])
		).toBe(false);
		expect(
			await hadRecentHighRiskWindow(env.db, world.org.id, steamId, now, 24, current.id, [
				'window-1',
				'fresh'
			])
		).toBe(false);
	});
});
