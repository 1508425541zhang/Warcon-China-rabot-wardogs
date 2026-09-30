import { describe, test, expect } from 'bun:test';
import { eq, sql } from 'drizzle-orm';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callApi, stubGateway } from './call';
import {
	integrityCases,
	integrityActions,
	siteSettings,
	matches,
	lists,
	listEntries,
	outbox,
	qqIntegrityNotifications
} from '$lib/server/db/schema';
import { integrityActionPage } from '$lib/server/integrity/action-history';
import { historyPolicy, pruneIntegrityHistory } from '$lib/server/integrity/history-retention';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import { PUT } from '../routes/api/server/[id]/integrity/history-retention/+server';

describe.skipIf(!hasTestDb)('Punishment history pagination and bounded retention', () => {
	test('server-scoped automatic/manual pagination is stable with arrivals, handles invalid pages, and has no hidden truncation', async () => {
		const env = await testEnv(),
			w = await seedWorld(env),
			at = new Date(Date.now() - 60000);
		await env.db.insert(integrityCases).values({
			id: w.server.id,
			orgId: w.org.id,
			serverId: w.server.id,
			steamId: '76561198000000001',
			createdAt: at,
			confidence: 'B',
			trigger: 'test',
			ruleVersion: 1,
			riskScore: 55,
			riskBreakdown: [],
			snapshot: {}
		});
		await env.db.insert(integrityActions).values(
			Array.from({ length: 125 }, (_, i) => ({
				id: w.server.id + '-' + String(i).padStart(3, '0'),
				caseId: w.server.id,
				orgId: w.org.id,
				serverId: w.server.id,
				steamId: '76561198000000001',
				createdAt: at,
				action: 'KICK',
				source: i % 2 ? 'RULE' : 'REVIEW',
				deliveryState: 'delivered'
			}))
		);
		const first = await integrityActionPage(env, w.server.id, { pageSize: 20 });
		expect(first.total).toBe(125);
		expect(first.pages).toBe(7);
		expect(first.rows).toHaveLength(20);
		await env.db.insert(integrityActions).values({
			id: crypto.randomUUID(),
			caseId: w.server.id,
			orgId: w.org.id,
			serverId: w.server.id,
			steamId: '76561198000000001',
			action: 'KICK',
			source: 'RULE',
			createdAt: new Date()
		});
		const ids = first.rows.map((r) => r.id);
		for (let page = 2; page <= 7; page++)
			ids.push(
				...(
					await integrityActionPage(env, w.server.id, {
						page,
						pageSize: 20,
						before: new Date(first.before)
					})
				).rows.map((r) => r.id)
			);
		expect(ids).toHaveLength(125);
		expect(new Set(ids).size).toBe(125);
		expect((await integrityActionPage(env, w.server.id, { filter: 'manual' })).total).toBe(63);
		expect((await integrityActionPage(env, w.server.id, { filter: 'automatic' })).total).toBe(63);
		expect(
			(await integrityActionPage(env, w.server.id, { page: Infinity, filter: 'bad' })).page
		).toBe(1);
		expect((await integrityActionPage(env, w.server.id, { page: 999, pageSize: 20 })).page).toBe(7);
		expect((await integrityActionPage(env, w.otherOrgServer.id)).total).toBe(0);
	});
	test('configuration is owner-only, validated, revision checked and read-only until enabled', async () => {
		const env = await testEnv(),
			w = await seedWorld(env);
		stubGateway();
		const policy = { autoDeleteEnabled: true, pageSize: 20, maxRecords: 100, maxAgeDays: 90 };
		expect((await historyPolicy(env, w.server.id)).policy.autoDeleteEnabled).toBe(false);
		const save = (user: any, body: any, id = w.server.id) =>
			callApi(PUT, user, { method: 'PUT', params: { id }, body });
		expect((await save(w.users.member, { policy, revision: '' })).status).toBe(404);
		expect((await save(w.users.owner, { policy, revision: '' }, w.otherOrgServer.id)).status).toBe(
			404
		);
		expect(
			(await save(w.users.owner, { policy: { ...policy, maxAgeDays: 1 }, revision: '' })).status
		).toBe(400);
		expect((await save(w.users.owner, { policy, revision: '' })).status).toBe(200);
		expect((await save(w.users.owner, { policy, revision: '' })).status).toBe(409);
		expect((await historyPolicy(env, w.server.id)).policy).toEqual(policy);
	});
	test('prunes real expired history and finished model payloads, preserves active bans, current-round receipts, unknown delivery and queued notifications', async () => {
		const env = await testEnv(),
			w = await seedWorld(env),
			old = new Date(Date.now() - 100 * 86400000),
			recent = new Date(Date.now() - 86400000),
			steam = '76561198000000001';
		const [match] = await env.db
			.insert(matches)
			.values({ serverId: w.server.id, startedAt: old })
			.returning();
		const [list] = await env.db
			.insert(lists)
			.values({ id: crypto.randomUUID(), orgId: w.org.id, kind: 'ban', name: 'test' })
			.returning();
		const entry = crypto.randomUUID();
		await env.db
			.insert(listEntries)
			.values({ id: entry, listId: list.id, steamId: steam, reason: 'manual permanent ban' });
		const actions = ['expired', 'active', 'unknown', 'pending', 'notification', 'recent'];
		for (const name of actions) {
			await env.db.insert(integrityCases).values({
				id: w.server.id + name,
				orgId: w.org.id,
				serverId: w.server.id,
				steamId: steam,
				createdAt: old,
				status: 'CLOSED',
				confidence: 'B',
				trigger: 'test',
				ruleVersion: 1,
				riskScore: 55,
				riskBreakdown: [],
				snapshot: {},
				reviewedAt: old
			});
			await env.db.insert(integrityActions).values({
				id: w.server.id + name,
				caseId: w.server.id + name,
				orgId: w.org.id,
				serverId: w.server.id,
				steamId: steam,
				createdAt: name === 'recent' ? recent : old,
				action: 'KICK',
				source: name === 'active' ? 'REVIEW' : 'RULE',
				deliveryState:
					name === 'unknown' ? 'unknown' : name === 'pending' ? 'pending' : 'delivered',
				effectiveAt: old,
				listEntryId: name === 'active' ? entry : null
			});
		}
		const [notice] = await env.db
			.insert(outbox)
			.values({
				serverId: w.server.id,
				triggerKind: 'integrity',
				action: 'kick',
				detail: { actionId: w.server.id + 'notification' },
				state: 'delivered',
				dedupeKey: crypto.randomUUID(),
				createdAt: old
			})
			.returning();
		await env.db.insert(qqIntegrityNotifications).values({
			outboxId: notice.id,
			groupId: '123456',
			serverId: w.server.id,
			selfId: '12345',
			content: 'test',
			state: 'pending'
		});
		for (const name of ['old', 'current', 'unknown'])
			await env.db.insert(siteSettings).values({
				key: 'shortRisk:' + w.server.id + ':' + name,
				updatedAt: old,
				value: {
					serverId: w.server.id,
					steamId: steam,
					score: 0.7,
					state: name === 'unknown' ? 'unknown' : 'warning',
					scope: ['s', 'boot', name === 'current' ? String(match.id) : '999999', 'map']
				}
			});
		for (const name of ['normal', 'long', 'uncertain', 'queued', 'active'])
			await env.db.execute(
				sql`INSERT INTO integrity_model_runs(id,org_id,server_id,steam_id,match_id,slot,config_revision,threshold,state,action,action_state,created_at,punished_at,list_entry_id) VALUES(${w.server.id + name},${w.org.id},${w.server.id},${steam},${match.id},${actions.length + ['normal', 'long', 'uncertain', 'queued', 'active'].indexOf(name)},'test',.03,${name === 'queued' ? 'pending' : 'READY'},${['long', 'uncertain', 'active'].includes(name) ? 'KICK' : null},${name === 'uncertain' ? 'unknown' : name === 'normal' ? 'skipped' : 'delivered'},${old},${['long', 'uncertain', 'active'].includes(name) ? old : null},${name === 'active' ? entry : null})`
			);
		await env.db.insert(siteSettings).values({
			key: 'integrityRetention:' + w.server.id,
			value: { autoDeleteEnabled: true, pageSize: 20, maxRecords: 100, maxAgeDays: 90 }
		});
		await releaseOwnership(env);
		expect(await pruneIntegrityHistory(env, w.server.id)).toBeNull();
		expect(await acquireOrRenew(env, 'retention-test')).toBe(true);
		try {
			const result = await pruneIntegrityHistory(env, w.server.id);
			expect(result).toEqual({ actions: 1, modelRuns: 2, short: 1, cases: 1 });
			expect(
				(
					await env.db
						.select()
						.from(integrityActions)
						.where(eq(integrityActions.serverId, w.server.id))
				)
					.map((a) => a.id)
					.sort()
			).toEqual(
				actions
					.filter((n) => n !== 'expired')
					.map((n) => w.server.id + n)
					.sort()
			);
			expect(await env.db.select().from(listEntries).where(eq(listEntries.id, entry))).toHaveLength(
				1
			);
			expect(
				await env.db
					.select()
					.from(qqIntegrityNotifications)
					.where(eq(qqIntegrityNotifications.outboxId, notice.id))
			).toHaveLength(1);
			expect(await pruneIntegrityHistory(env, w.server.id)).toEqual({
				actions: 0,
				modelRuns: 0,
				short: 0,
				cases: 0
			});
		} finally {
			await env.db.execute(sql`DELETE FROM integrity_model_runs WHERE server_id=${w.server.id}`);
			await releaseOwnership(env);
		}
	});
	test('overflow retains the newest 100 rows rather than merely hiding or deleting recent evidence', async () => {
		const env = await testEnv(),
			w = await seedWorld(env),
			old = new Date(Date.now() - 10 * 86400000);
		await env.db.insert(siteSettings).values(
			Array.from({ length: 620 }, (_, i) => ({
				key: 'shortRisk:' + w.server.id + ':' + i,
				updatedAt: new Date(old.getTime() + i * 1000),
				value: {
					serverId: w.server.id,
					steamId: '76561198000000001',
					score: 0.7,
					state: 'warning',
					scope: ['s', 'boot', '99999', 'map']
				}
			}))
		);
		await env.db.insert(siteSettings).values({
			key: 'integrityRetention:' + w.server.id,
			value: { autoDeleteEnabled: true, pageSize: 20, maxRecords: 100, maxAgeDays: 90 }
		});
		await acquireOrRenew(env, 'retention-overflow');
		try {
			expect((await pruneIntegrityHistory(env, w.server.id))?.short).toBe(500);
			expect((await pruneIntegrityHistory(env, w.server.id))?.short).toBe(20);
			expect((await integrityActionPage(env, w.server.id)).total).toBe(100);
		} finally {
			await releaseOwnership(env);
		}
	});
});
