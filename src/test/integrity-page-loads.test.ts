import { beforeAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import type { Env } from '$lib/server/env';
import {
	integrityCases,
	integrityReports,
	integrityScores,
	integrityWindows
} from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { callLoad, stubGateway } from './call';
import { seedWorld, type World } from './world';

const ROUTES = join(import.meta.dir, '..', 'routes', '(app)', 'server', '[id]');
const PLAYER = '76561198000000421';

describe.skipIf(!hasTestDb)('Integrity and Player Dossier page loads', () => {
	let env: Env;
	let world: World;
	const loadBoth = async () => {
		const { load: integrity } = await import(join(ROUTES, 'integrity', '+page.server.ts'));
		const { load: player } = await import(join(ROUTES, 'players', '[steamId]', '+page.server.ts'));
		const params = { id: world.server.id, steamId: PLAYER };
		const first = await callLoad(integrity, world.users.owner, { params });
		const second = await callLoad(player, world.users.owner, { params });
		expect(first.status).toBe(200);
		expect(second.status).toBe(200);
		return {
			integrity: first.body as Record<string, any>,
			player: second.body as Record<string, any>
		};
	};
	beforeAll(async () => {
		env = await testEnv();
		stubGateway();
		world = await seedWorld(env);
	});
	test('both pages load without a score or optional Integrity records', async () => {
		const result = await loadBoth();
		expect(result.integrity.scores).toEqual([]);
		expect(result.player.integrity.riskScore).toBeNull();
	});
	test('both pages load with a window, score, report, and case; malformed historic JSON is ignored', async () => {
		const now = new Date();
		const [window] = await env.db
			.insert(integrityWindows)
			.values({
				orgId: world.org.id,
				serverId: world.server.id,
				steamId: PLAYER,
				instanceId: 'page-test',
				map: 'Kavkazi',
				clockFrom: 0,
				clockTo: 180,
				observedAt: now,
				infantryKills: 12,
				kpm180: 4,
				uniqueVictims: 12,
				headshots: 8,
				penetrations: 2,
				burstPoints: 7,
				behaviorReasons: ['kpm', 'burst'],
				eventIds: ['one']
			})
			.returning();
		await env.db.insert(integrityScores).values({
			windowId: window.id,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId: PLAYER,
			scoredAt: now,
			ruleVersion: 1,
			score: 55,
			level: 'AUTO_KO',
			breakdown: 'legacy-json-string',
			currentBehaviorAnomaly: true
		});
		await env.db.insert(integrityReports).values({
			orgId: world.org.id,
			serverId: world.server.id,
			targetSteamId: PLAYER,
			reporterSteamId: '76561198000000422',
			reason: 'suspicious behavior',
			source: 'web',
			createdAt: now,
			evidenceFrom: now,
			evidenceUntil: now
		});
		await env.db.insert(integrityCases).values({
			id: 'CASE-PAGE-TEST',
			orgId: world.org.id,
			serverId: world.server.id,
			steamId: PLAYER,
			createdAt: now,
			confidence: 'B',
			trigger: 'ABNORMAL_INFANTRY_WINDOW',
			ruleVersion: 1,
			riskScore: 55,
			riskBreakdown: [],
			snapshot: { behaviorReasons: ['kpm'] }
		});
		const result = await loadBoth();
		expect(result.integrity.scores).toHaveLength(1);
		expect(result.integrity.cases).toHaveLength(1);
		expect(result.integrity.reports).toHaveLength(1);
		expect(result.player.integrity.latestWindow.kpm180).toBe(4);
	});
});
