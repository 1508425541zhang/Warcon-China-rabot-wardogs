import { beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import {
	account,
	integrityReportEvents,
	integrityReports,
	integrityScores,
	kills,
	playerSessions
} from '$lib/server/db/schema';
import { captureReportEvidence, submitReport } from '$lib/server/integrity/reports';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

describe.skipIf(!hasTestDb)('community report persistence', () => {
	let env: Env;
	let world: World;
	const reporter = '76561198000000101';
	const target = '76561198000000102';
	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
		await env.db.insert(account).values({
			id: `steam_${world.users.member!.id}`,
			accountId: reporter,
			providerId: 'steam',
			userId: world.users.member!.id
		});
		await env.db.insert(playerSessions).values({
			serverId: world.server.id,
			steamId: target,
			name: 'Suspect',
			joinedAt: new Date(Date.now() - 60_000),
			lastSeen: new Date()
		});
	});

	test('freezes past and future events, scores distinct reporters and enforces cooldown', async () => {
		const before = new Date(Date.now() - 30_000);
		await env.db.insert(kills).values({
			ts: before,
			serverId: world.server.id,
			eventId: 'pre',
			instanceId: 'i1',
			matchId: 'm1',
			eventTime: 10,
			map: 'test',
			killerSteamId: target,
			killerName: 'Suspect',
			victimSteamId: '76561198000000103',
			victimName: 'Victim',
			tags: []
		});
		const input = { serverId: world.server.id, target: 'Suspect', reason: 'Suspicious kills' };
		const request = new Request('http://localhost:5173/api/reports', { method: 'POST' });
		const created = await submitReport(env, request, world.users.member!, input);
		const [report] = await env.db
			.select()
			.from(integrityReports)
			.where(eq(integrityReports.id, created.id));
		expect(report.targetSteamId).toBe(target);
		const [score] = await env.db
			.select()
			.from(integrityScores)
			.where(eq(integrityScores.reportId, created.id));
		expect(score.score).toBe(1);
		expect(score.currentBehaviorAnomaly).toBe(false);
		expect(
			await env.db
				.select()
				.from(integrityReportEvents)
				.where(eq(integrityReportEvents.reportId, created.id))
		).toHaveLength(1);
		await env.db.insert(kills).values({
			ts: new Date(),
			serverId: world.server.id,
			eventId: 'post',
			instanceId: 'i1',
			matchId: 'm1',
			eventTime: 40,
			map: 'test',
			killerSteamId: target,
			killerName: 'Suspect',
			victimSteamId: '76561198000000104',
			victimName: 'Victim 2',
			tags: []
		});
		await captureReportEvidence(env, world.server.id, [{ eventId: 'post' }]);
		expect(
			await env.db
				.select()
				.from(integrityReportEvents)
				.where(eq(integrityReportEvents.reportId, created.id))
		).toHaveLength(2);
		await expect(submitReport(env, request, world.users.member!, input)).rejects.toMatchObject({
			status: 429
		});
	});
});
