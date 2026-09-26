import { beforeAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import {
	integrityCases,
	integrityReports,
	integrityScores,
	integrityWindows,
	kills,
	serverLive,
	servers
} from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { callLoad, stubGateway } from './call';
import { seedWorld, type World } from './world';
import { loadPlayerIntegrity } from '$lib/server/integrity/player-view';

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
	test('historical window count does not count an upgraded score twice', async () => {
		const [window] = await env.db
			.select()
			.from(integrityWindows)
			.where(eq(integrityWindows.steamId, PLAYER));
		await env.db.insert(integrityScores).values({
			windowId: window.id,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId: PLAYER,
			scoredAt: new Date(),
			ruleVersion: 1,
			score: 65,
			level: 'AUTO_QUARANTINE_ELIGIBLE',
			breakdown: [],
			currentBehaviorAnomaly: true
		});
		const result = await loadBoth();
		expect(result.integrity.scores).toHaveLength(2);
		expect(
			result.integrity.dryRun.every((period: { windows: number }) => period.windows === 1)
		).toBe(true);
	});
	test('player with no kills uses the server feed; another player advances the rolling clock', async () => {
		const now = new Date();
		const [server] = await env.db
			.update(servers)
			.set({ feedTokenHash: 'test-feed' })
			.where(eq(servers.id, world.server.id))
			.returning();
		await env.db
			.insert(serverLive)
			.values({
				serverId: server.id,
				feedAt: now,
				status: { map: 'Ozeti' },
				players: [{ steamId: PLAYER, kills: 0, deaths: 0 }]
			});
		const event = {
			serverId: server.id,
			ts: now,
			instanceId: 'clock',
			matchId: 'clock',
			map: 'Europe',
			victimSteamId: '76561198000000422',
			victimName: 'Target',
			killerFaction: 'A',
			victimFaction: 'B',
			factionObservedAt: now,
			factionBracketed: true,
			cause: 'Id.Item.AK74M',
			tags: []
		};
		await env.db
			.insert(kills)
			.values({
				...event,
				eventId: 'other-kill',
				eventTime: 400,
				killerSteamId: '76561198000000423'
			});
		const empty = await loadPlayerIntegrity(env, server, PLAYER);
		expect(empty.metricsAvailable).toBe(true);
		expect(empty.metrics).toBeNull();
		await env.db
			.insert(kills)
			.values({
				...event,
				ts: new Date(now.getTime() - 240_000),
				eventId: 'old-own-kill',
				eventTime: 160,
				killerSteamId: PLAYER
			});
		const old = await loadPlayerIntegrity(env, server, PLAYER);
		expect(old.metricsAvailable).toBe(true);
		expect(old.metrics?.kpm180).toBe(0);
		expect(old.metrics?.peakKpm180).toBeCloseTo(1 / 3);
	});
});
