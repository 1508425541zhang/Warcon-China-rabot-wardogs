import { beforeAll, afterAll, describe, expect, spyOn, test } from 'bun:test';
import { eq, sql } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import { seedWorld, type World } from './world';
import { callApi } from './call';
import type { Env } from '$lib/server/env';
import { GET, PUT } from '../routes/api/admin/qq/vips/+server';
import { GET as communityGet } from '../routes/api/servers/[id]/community/+server';
import {
	vipSettings,
	saveVips,
	vipFor,
	vipAllowsMetric,
	vipRiskKickExempt
} from '$lib/server/qq/vip';
import { desiredFor, kickBanned } from '$lib/server/lists-sync';
import { serverListOf } from '$lib/server/lists';
import { getServer, getOrg } from '$lib/server/access';
import { listEntries, integrityCases, integrityActions } from '$lib/server/db/schema';
import { integrityDeliverySkipReason } from '$lib/server/integrity/delivery';
import { ACTIONS } from '$lib/server/actions';
import type { WardogsClient } from '$lib/server/rcon';

describe.skipIf(!hasTestDb)('VIP privileges', () => {
	let env: Env, world: World;
	const steam = '76561198000000001';
	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
	}, 120000);
	afterAll(async () => {
		await env.db.execute(sql`DELETE FROM site_settings WHERE key='qqVip'`);
	});
	const entry = () => ({
		serverId: world.server.id,
		steamId: steam,
		enabled: true,
		reserve: true,
		allowOverkill: false,
		whitelist: false,
		note: 'private note'
	});
	const save = async (entries: ReturnType<typeof entry>[]) =>
		saveVips(env, { revision: (await vipSettings(env)).revision, entries }, world.users.site!.id);
	test('only site owners manage VIP; saves without any QQ settings and rejects duplicate, bad IDs and stale edits', async () => {
		for (const role of ['anon', 'owner', 'member', 'keyAll'] as const) {
			expect([401, 403]).toContain((await callApi(GET, world.users[role])).status);
			expect([401, 403]).toContain(
				(await callApi(PUT, world.users[role], { method: 'PUT', body: {} })).status
			);
		}
		const before = await vipSettings(env);
		const result = await callApi(PUT, world.users.site, {
			method: 'PUT',
			body: { revision: before.revision, entries: [entry()] }
		});
		expect(result.status).toBe(200);
		expect((await vipFor(env, world.server.id, steam))?.reserve).toBe(true);
		await expect(
			saveVips(env, { revision: before.revision, entries: [] }, world.users.site!.id)
		).rejects.toThrow();
		for (const entries of [
			[entry(), entry()],
			[{ ...entry(), steamId: '123' }],
			[{ ...entry(), serverId: 'missing' }]
		])
			await expect(save(entries)).rejects.toThrow();
	});
	test('query is server-scoped and never discloses administrator notes', async () => {
		const args = { params: { id: world.server.id }, query: `view=vip&steamId=${steam}` };
		expect((await callApi(communityGet, world.users.outsider, args)).status).toBe(404);
		const result = await callApi(communityGet, world.users.keyView, args);
		expect(result.status).toBe(200);
		expect(JSON.stringify(result.body)).not.toContain('private note');
		expect(await vipFor(env, world.otherServer.id, steam)).toBeUndefined();
	});
	test('reservation withdrawal preserves a separately managed reservation', async () => {
		const server = (await getServer(env, world.server.id))!,
			org = (await getOrg(env, world.org.id))!;
		expect((await desiredFor(env, server, org)).reserved.map((r) => r.steamId)).toContain(steam);
		const list = await serverListOf(env, server, 'reserve');
		const id = crypto.randomUUID();
		await env.db
			.insert(listEntries)
			.values({ id, listId: list.id, steamId: steam, addedBy: world.users.site!.id });
		await save([]);
		expect((await desiredFor(env, server, org)).reserved.map((r) => r.steamId)).toContain(steam);
		await env.db.delete(listEntries).where(eq(listEntries.id, id));
		expect((await desiredFor(env, server, org)).reserved.map((r) => r.steamId)).not.toContain(
			steam
		);
	});
	test('overkill only exempts KD/KPM; whitelist suppresses automatic risk kick, not other actions', async () => {
		const vip = { ...entry(), reserve: false, allowOverkill: true };
		await save([vip]);
		expect(vipAllowsMetric(vip, 'kd')).toBe(false);
		expect(vipAllowsMetric(vip, 'kpm')).toBe(false);
		expect(vipAllowsMetric(vip, 'cash')).toBe(true);
		const row = {
			serverId: world.server.id,
			steamId: steam,
			triggerKind: 'risk_kick',
			action: 'kick'
		};
		expect(await vipRiskKickExempt(env, row)).toBe(false);
		await save([{ ...vip, whitelist: true }]);
		expect(await vipRiskKickExempt(env, row)).toBe(true);
		expect(await vipRiskKickExempt(env, { ...row, triggerKind: 'team_kill' })).toBe(false);
		expect(vipAllowsMetric({ ...vip, whitelist: true }, 'cash')).toBe(false);
	});
	test('whitelist allows re-entry past automatic quarantine and pending automatic kicks, preserving manual bans', async () => {
		const server = (await getServer(env, world.server.id))!,
			org = (await getOrg(env, world.org.id))!;
		const list = await serverListOf(env, server, 'ban');
		const caseId = crypto.randomUUID(),
			actionId = crypto.randomUUID(),
			entryId = crypto.randomUUID();
		await env.db.insert(integrityCases).values({
			id: caseId,
			createdAt: new Date(),
			orgId: world.org.id,
			serverId: server.id,
			steamId: steam,
			confidence: 'B',
			trigger: 'ABNORMAL_INFANTRY_WINDOW',
			ruleVersion: 1,
			riskScore: 85,
			riskBreakdown: [],
			snapshot: {}
		});
		await env.db.insert(listEntries).values({
			id: entryId,
			listId: list.id,
			steamId: steam,
			addedByName: 'Community Integrity rule'
		});
		await env.db.insert(integrityActions).values({
			id: actionId,
			caseId,
			orgId: world.org.id,
			serverId: server.id,
			steamId: steam,
			action: 'QUARANTINE_7D',
			source: 'RULE',
			listEntryId: entryId
		});
		const row = {
			serverId: server.id,
			steamId: steam,
			triggerKind: 'integrity',
			action: 'kick',
			detail: { caseId, actionId }
		};
		expect(await integrityDeliverySkipReason(env, row)).toContain('VIP');
		expect((await desiredFor(env, server, org)).bans).toHaveLength(0);
		const kicks = spyOn(ACTIONS.kick, 'run').mockResolvedValue({} as never);
		try {
			await kickBanned(
				env,
				server,
				org,
				{} as WardogsClient,
				[steam],
				new Map([[steam, { steamId: steam, listId: list.id }]])
			);
			expect(kicks).not.toHaveBeenCalled();
			await env.db
				.update(listEntries)
				.set({ addedBy: world.users.site!.id, addedByName: 'administrator' })
				.where(eq(listEntries.id, entryId));
			expect((await desiredFor(env, server, org)).bans).toHaveLength(1);
			await kickBanned(
				env,
				server,
				org,
				{} as WardogsClient,
				[steam],
				new Map([[steam, { steamId: steam, listId: list.id }]])
			);
			expect(kicks).toHaveBeenCalledTimes(1);
			await env.db
				.update(integrityActions)
				.set({ source: 'REVIEW' })
				.where(eq(integrityActions.id, actionId));
			await env.db
				.update(integrityCases)
				.set({ reviewedAt: new Date() })
				.where(eq(integrityCases.id, caseId));
			expect(await integrityDeliverySkipReason(env, row)).toBeNull();
		} finally {
			kicks.mockRestore();
		}
	});
});
