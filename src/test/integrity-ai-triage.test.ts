import { describe, test, expect } from 'bun:test';
import { eq } from 'drizzle-orm';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { saveAiSettings, aiSettings, aiBundle } from '$lib/server/integrity/ai';
import { finishAiReview, triageDecision } from '$lib/server/integrity/ai-triage';
import {
	integrityCases,
	integrityCaseEvents,
	integrityAiSettings,
	integrityAiJobs,
	integrityAiReviews,
	integrityActions
} from '$lib/server/db/schema';
import { discoverAiJobs } from '$lib/server/integrity/ai-queue';
import { acquireOrRenew } from '$lib/server/leadership';
import { callLoad, stubGateway } from './call';
import { load } from '../routes/(app)/server/[id]/integrity/cases/+page.server';

const review = {
	verdict: '建议通过',
	suspicionPercent: 20,
	evidenceQuality: '中',
	summary: '数据可核对，未见可靠违规证据。',
	reasons: [{ text: '6次步兵击杀，KPM为2。', evidence: 'case.snapshot.infantryKills' }],
	alternatives: [],
	contradictions: [],
	missingEvidence: []
};
test('balanced triage boundaries, missing data and conflicts do not become low risk', () => {
	for (const n of [0, 20, 25])
		expect(triageDecision({ ...review, suspicionPercent: n }, true)).toBe('AI_CLEARED');
	for (const n of [30, 45, 60])
		expect(triageDecision({ ...review, verdict: '建议复核', suspicionPercent: n }, true)).toBe(
			'AI_ARCHIVED'
		);
	for (const n of [65, 90, 100])
		expect(triageDecision({ ...review, suspicionPercent: n }, true)).toBe('ADMIN_REVIEW');
	for (const extra of [
		{ suspicionPercent: null },
		{ evidenceQuality: '低' },
		{ contradictions: ['计数不符'] },
		{ reasons: [] }
	])
		expect(triageDecision({ ...review, ...extra }, true)).toBe('AI_ARCHIVED_UNRESOLVED');
	expect(triageDecision({ ...review, missingEvidence: ['关键证据'] }, true)).toBe('AI_ARCHIVED');
	expect(triageDecision(review, false)).toBe('AI_ARCHIVED');
});

describe.skipIf(!hasTestDb)('AI automatic case lifecycle', () => {
	async function fixture() {
		const env = await testEnv(),
			w = await seedWorld(env);
		await saveAiSettings(env, w.org.id, {
			baseUrl: 'https://example.com/v1',
			apiKey: 'test-key',
			model: 'test'
		});
		const config = (await aiSettings(env, w.org.id))!;
		const id = crypto.randomUUID(),
			token = crypto.randomUUID();
		await env.db.insert(integrityCases).values({
			id,
			orgId: w.org.id,
			serverId: w.server.id,
			steamId: '76561198000000001',
			createdAt: new Date(Date.now() - 10 * 86400000),
			status: 'OPEN',
			confidence: 'B',
			trigger: 'AI_SINGLE_SIGNAL',
			ruleVersion: 1,
			riskScore: 35,
			riskBreakdown: [],
			statistical: { level: 'WATCH', committee: { votes: [] } },
			snapshot: {
				roundId: 'round',
				eventIds: ['event'],
				infantryKills: 6,
				kpm180: 2,
				rules: { large: 'payload' }
			}
		});
		await env.db.insert(integrityCaseEvents).values({
			caseId: id,
			instanceId: 'instance',
			eventId: 'event',
			event: {
				killerSteamId: '76561198000000001',
				ts: new Date().toISOString(),
				victimName: 'large evidence',
				cause: 'rifle'
			}
		});
		await env.db.insert(integrityAiJobs).values({
			caseId: id,
			state: 'running',
			claimToken: token,
			leaseUntil: new Date(Date.now() + 60000)
		});
		await env.db.insert(integrityAiReviews).values({ caseId: id, fingerprint: id, result: review });
		const finish = (input: unknown = review, claim: string = token) =>
			env.db.transaction((tx) => finishAiReview(tx, id, claim, input, config.updatedAt));
		const read = async () =>
			(await env.db.select().from(integrityCases).where(eq(integrityCases.id, id)))[0];
		return { env, w, id, token, config, finish, read };
	}
	test('low risk clears evidence copies, retains dedup receipt and exclusion keys, prevents incomplete re-review', async () => {
		const f = await fixture();
		await f.finish();
		const c = await f.read();
		expect(c.status).toBe('AI_CLEARED');
		expect(c.reviewedAt).not.toBeNull();
		expect(c.reviewedBy).toBeNull();
		expect(c.snapshot).toEqual({ roundId: 'round', eventIds: ['event'] });
		expect(c.statistical).toEqual({ level: 'WATCH' });
		const events = await f.env.db
			.select()
			.from(integrityCaseEvents)
			.where(eq(integrityCaseEvents.caseId, f.id));
		expect(events).toHaveLength(1);
		expect(events[0].event).toHaveProperty('killerSteamId');
		expect(events[0].event).not.toHaveProperty('cause');
		expect(
			await f.env.db.select().from(integrityAiReviews).where(eq(integrityAiReviews.caseId, f.id))
		).toHaveLength(0);
		expect(
			await f.env.db.select().from(integrityActions).where(eq(integrityActions.caseId, f.id))
		).toHaveLength(0);
		await expect(aiBundle(f.env, f.w.org.id, f.w.server.id, f.id)).rejects.toThrow('已清理');
		stubGateway();
		const page = async (view: string) =>
			(
				await callLoad(load, f.w.users.owner, {
					params: { id: f.w.server.id },
					query: `view=${view}`
				})
			).body as any;
		expect((await page('pending')).total).toBe(0);
		expect((await page('all')).total).toBe(0);
		expect((await page('cleared')).cases[0].id).toBe(f.id);
		await f.finish(); // idempotent after state becomes done
		expect((await f.read()).status).toBe('AI_CLEARED');
	});
	test('medium and insufficient evidence close without penalty; high stays open', async () => {
		for (const [n, status] of [
			[45, 'AI_ARCHIVED'],
			[80, 'OPEN'],
			[null, 'AI_ARCHIVED']
		] as const) {
			const f = await fixture();
			await f.finish({
				...review,
				suspicionPercent: n,
				verdict: n === null ? '证据不足' : '建议复核'
			});
			expect((await f.read()).status).toBe(status);
			expect(
				await f.env.db.select().from(integrityActions).where(eq(integrityActions.caseId, f.id))
			).toHaveLength(0);
		}
	});
	test('human review, stale claim, changed settings, and existing actions cannot be overwritten', async () => {
		for (const guard of [
			'human',
			'claim',
			'settings',
			'action',
			'arithmetic',
			'disabled'
		] as const) {
			const f = await fixture();
			if (guard === 'human')
				await f.env.db
					.update(integrityCases)
					.set({ status: 'REVIEWED', reviewedAt: new Date(), reviewedBy: f.w.users.owner!.id })
					.where(eq(integrityCases.id, f.id));
			if (guard === 'settings')
				await f.env.db
					.update(integrityAiSettings)
					.set({ updatedAt: new Date(f.config.updatedAt.getTime() + 1000) })
					.where(eq(integrityAiSettings.orgId, f.w.org.id));
			if (guard === 'disabled')
				await f.env.db
					.update(integrityAiSettings)
					.set({ autoCloseEnabled: false })
					.where(eq(integrityAiSettings.orgId, f.w.org.id));
			if (guard === 'action')
				await f.env.db.insert(integrityActions).values({
					id: crypto.randomUUID(),
					caseId: f.id,
					orgId: f.w.org.id,
					serverId: f.w.server.id,
					steamId: '76561198000000001',
					action: 'KICK',
					source: 'RULE'
				});
			if (guard === 'arithmetic')
				await f.env.db
					.update(integrityCases)
					.set({ snapshot: { infantryKills: 6, kpm180: 9 } })
					.where(eq(integrityCases.id, f.id));
			await f.finish(review, guard === 'claim' ? 'wrong-claim' : f.token);
			expect((await f.read()).status).toBe(
				guard === 'human' ? 'REVIEWED' : guard === 'arithmetic' ? 'AI_ARCHIVED' : 'OPEN'
			);
		}
	});
	test('old unreviewed cases enter discovery', async () => {
		const f = await fixture();
		await f.env.db.delete(integrityAiJobs).where(eq(integrityAiJobs.caseId, f.id));
		await acquireOrRenew(f.env, 'triage-tests');
		await discoverAiJobs(f.env);
		expect(
			(await f.env.db.select().from(integrityAiJobs).where(eq(integrityAiJobs.caseId, f.id)))[0]
				.state
		).toBe('pending');
	});
});
