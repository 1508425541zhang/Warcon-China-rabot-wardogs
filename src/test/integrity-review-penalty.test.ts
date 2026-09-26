import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import {
	integrityCases,
	integrityActions,
	integrityLabels,
	listEntries,
	outbox
} from '$lib/server/db/schema';
import { integrityDeliverySkipReason } from '$lib/server/integrity/delivery';
import { serverListOf } from '$lib/server/lists';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callApi, callLoad, stubGateway } from './call';

describe.skipIf(!hasTestDb)('human review punishment', () => {
	async function fixture() {
		const env = await testEnv();
		stubGateway();
		const world = await seedWorld(env);
		const id = crypto.randomUUID(),
			steamId = '76561198000000988';
		await env.db.insert(integrityCases).values({
			id,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId,
			createdAt: new Date(),
			confidence: 'B',
			trigger: 'STATISTICAL_WINDOW',
			ruleVersion: 1,
			riskScore: 80,
			riskBreakdown: [],
			snapshot: {}
		});
		const { POST } =
			await import('../routes/api/orgs/[id]/integrity/cases/[caseId]/labels/+server');
		const save = (label = 'CONFIRMED_ABUSE', reason = '管理员已逐条核对击杀证据') =>
			callApi(POST, world.users.owner, {
				method: 'POST',
				params: { id: world.org.id, caseId: id },
				body: { label, reason }
			});
		return { env, world, id, steamId, save };
	}
	test('atomic 7-day ban, concurrent retry, correction and delivery cancellation', async () => {
		const { env, world, id, save } = await fixture();
		const before = Date.now();
		const results = await Promise.all([save(), save()]);
		expect(results.map((r) => r.status)).toEqual([200, 200]);
		const [action] = await env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.caseId, id));
		expect(action.source).toBe('REVIEW');
		expect(action.effectiveAt).not.toBeNull();
		expect(action.deliveryState).toBe('pending');
		expect(action.expiresAt!.getTime()).toBeGreaterThanOrEqual(before + 7 * 86400000);
		expect(action.expiresAt!.getTime()).toBeLessThan(Date.now() + 7 * 86400000 + 1000);
		const [ban] = await env.db
			.select()
			.from(listEntries)
			.where(eq(listEntries.id, action.listEntryId!));
		expect(ban.addedBy).toBe(world.users.owner!.id);
		const [job] = await env.db
			.select()
			.from(outbox)
			.where(eq(outbox.dedupeKey, `integrity:${action.id}:kick`));
		// Explicit human review works with automatic enforcement disabled.
		expect(await integrityDeliverySkipReason(env, job)).toBeNull();
		expect((await save('CONFIRMED_ABUSE', '补充人工审核理由，不应延长封禁')).status).toBe(200);
		expect(
			await env.db.select().from(integrityActions).where(eq(integrityActions.caseId, id))
		).toHaveLength(1);
		expect(
			(await env.db.select().from(listEntries).where(eq(listEntries.id, ban.id)))[0].expiresAt
		).toEqual(ban.expiresAt);
		expect(
			await env.db.select().from(outbox).where(eq(outbox.dedupeKey, job.dedupeKey!))
		).toHaveLength(1);
		const { load } = await import('../routes/(app)/server/[id]/integrity/+page.server');
		const page = await callLoad(load, world.users.owner, { params: { id: world.server.id } });
		expect(page.status).toBe(200);
		expect(
			(page.body as { reviewPenalties: { caseId: string; active: boolean }[] }).reviewPenalties
		).toContainEqual(expect.objectContaining({ caseId: id, active: true }));
		await save('INSUFFICIENT_EVIDENCE', '后续复核需要补充材料，封禁独立管理');
		expect(
			(await env.db.select().from(listEntries).where(eq(listEntries.id, ban.id)))[0].removedAt
		).toBeNull();
		await env.db
			.update(listEntries)
			.set({ removedAt: new Date() })
			.where(eq(listEntries.id, ban.id));
		expect(await integrityDeliverySkipReason(env, job)).not.toBeNull();
		await save(); // never recreate an explicitly revoked punishment by resaving the case
		expect(
			(await env.db.select().from(listEntries).where(eq(listEntries.id, ban.id)))[0].removedAt
		).not.toBeNull();
	});
	for (const expiry of [null, new Date(Date.now() + 30 * 86400000)])
		test(`preserves existing ${expiry ? 'longer' : 'permanent'} ban`, async () => {
			const { env, world, id, steamId, save } = await fixture();
			const list = await serverListOf(env, { ...world.server, orgId: world.org.id }, 'ban');
			const entryId = crypto.randomUUID();
			await env.db.insert(listEntries).values({
				id: entryId,
				listId: list.id,
				steamId,
				reason: 'existing ban',
				expiresAt: expiry
			});
			expect((await save()).status).toBe(200);
			expect(
				(await env.db.select().from(listEntries).where(eq(listEntries.id, entryId)))[0].expiresAt
			).toEqual(expiry);
			expect(
				(await env.db.select().from(integrityActions).where(eq(integrityActions.caseId, id)))[0]
					.listEntryId
			).toBe(entryId);
		});
	test('non-confirming labels never punish; invalid identity rolls the review back', async () => {
		const { env, id, save } = await fixture();
		for (const label of ['FALSE_POSITIVE', 'INSUFFICIENT_EVIDENCE', 'DATA_ERROR'])
			expect((await save(label)).status).toBe(200);
		expect(
			await env.db.select().from(integrityActions).where(eq(integrityActions.caseId, id))
		).toHaveLength(0);
		const prior = await env.db.select().from(integrityLabels).where(eq(integrityLabels.caseId, id));
		await env.db
			.update(integrityCases)
			.set({ steamId: 'invalid' })
			.where(eq(integrityCases.id, id));
		expect((await save()).status).toBe(409);
		expect(
			await env.db.select().from(integrityLabels).where(eq(integrityLabels.caseId, id))
		).toHaveLength(prior.length);
		expect(
			await env.db.select().from(integrityActions).where(eq(integrityActions.caseId, id))
		).toHaveLength(0);
	});
});
