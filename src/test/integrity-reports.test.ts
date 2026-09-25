import { beforeAll, describe, expect, test } from 'bun:test';
import { randomInt } from 'node:crypto';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import {
	account,
	integrityReportEvents,
	integrityReports,
	integrityScores,
	kills,
	playerSessions,
	organizations,
	servers
} from '$lib/server/db/schema';
import { captureReportEvidence, submitReport } from '$lib/server/integrity/reports';
import { POST as reportPost } from '../routes/api/reports/+server';
import { hasTestDb, testEnv } from './db';
import { callApi } from './call';
import { seedWorld, type World } from './world';

describe.skipIf(!hasTestDb)('community report persistence', () => {
	let env: Env;
	let world: World;
	const reporter = '76561198000000101';
	const target = '76561198000000102';
	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
		// This case exercises a community report; the server must actually be public.
		await env.db.update(servers).set({ publicStatus: true }).where(eq(servers.id, world.server.id));
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

const steamId = () => String(76561198000000000n + BigInt(randomInt(100000000, 999999999)));

describe.skipIf(!hasTestDb)('community report authorization', () => {
	async function fixture(options: { publicStatus?: boolean; steam?: boolean } = {}) {
		const env = await testEnv();
		const world = await seedWorld(env);
		const reporter = steamId();
		const target = steamId();
		if (options.publicStatus)
			await env.db
				.update(servers)
				.set({ publicStatus: true })
				.where(eq(servers.id, world.server.id));
		if (options.steam !== false)
			await env.db.insert(account).values({
				id: `steam_${world.users.member!.id}`,
				accountId: reporter,
				providerId: 'steam',
				userId: world.users.member!.id
			});
		for (const serverId of [world.server.id, world.otherOrgServer.id])
			await env.db.insert(playerSessions).values({
				serverId,
				steamId: target,
				name: 'Report Target',
				joinedAt: new Date(Date.now() - 60_000),
				lastSeen: new Date()
			});
		return { env, world, reporter, target };
	}

	const request = (serverId: string) => ({
		method: 'POST',
		body: { serverId, target: 'Report Target', reason: 'Suspicious behavior' }
	});

	test('verified member may report on a public status server', async () => {
		const { env, world, target } = await fixture({ publicStatus: true });
		const result = await callApi(reportPost, world.users.member, request(world.server.id));
		expect(result.status).toBe(201);
		const rows = await env.db
			.select()
			.from(integrityReports)
			.where(eq(integrityReports.serverId, world.server.id));
		expect(rows).toHaveLength(1);
		expect(rows[0]).toMatchObject({
			orgId: world.org.id,
			serverId: world.server.id,
			targetSteamId: target
		});
	});

	test('known private server ID in another org grants no report access', async () => {
		const { env, world } = await fixture();
		const result = await callApi(reportPost, world.users.member, request(world.otherOrgServer.id));
		expect(result.status).toBe(404);
		expect(
			await env.db
				.select()
				.from(integrityReports)
				.where(eq(integrityReports.serverId, world.otherOrgServer.id))
		).toHaveLength(0);
	});

	test('member with integrity.view may report on a private server', async () => {
		const { env, world } = await fixture();
		await env.db.insert(account).values({
			id: `steam_${world.users.admin!.id}`,
			accountId: steamId(),
			providerId: 'steam',
			userId: world.users.admin!.id
		});
		const result = await callApi(reportPost, world.users.admin, request(world.server.id));
		expect(result.status).toBe(201);
		expect(
			await env.db
				.select()
				.from(integrityReports)
				.where(eq(integrityReports.serverId, world.server.id))
		).toHaveLength(1);
	});

	test('suspended org cannot accept reports even while public status switch is on', async () => {
		const { env, world } = await fixture({ publicStatus: true });
		await env.db
			.update(organizations)
			.set({ suspendedAt: new Date() })
			.where(eq(organizations.id, world.org.id));
		const result = await callApi(reportPost, world.users.member, request(world.server.id));
		expect(result.status).toBe(404);
		expect(
			await env.db
				.select()
				.from(integrityReports)
				.where(eq(integrityReports.serverId, world.server.id))
		).toHaveLength(0);
	});

	test('public server still requires a verified Steam account', async () => {
		const { env, world } = await fixture({ publicStatus: true, steam: false });
		const result = await callApi(reportPost, world.users.member, request(world.server.id));
		expect(result.status).toBe(403);
		expect(result.code).toBe('steam_link_required');
		expect(
			await env.db
				.select()
				.from(integrityReports)
				.where(eq(integrityReports.serverId, world.server.id))
		).toHaveLength(0);
	});
});
