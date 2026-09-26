import { describe, test, expect } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { acquireOrRenew } from '$lib/server/leadership';
import { aiCall, saveAiSettings } from '$lib/server/integrity/ai';
import { discoverAiJobs, processNextAiJob } from '$lib/server/integrity/ai-queue';
import { reviewOutput } from '$lib/server/integrity/ai-protocol';
import {
	integrityAiSettings,
	integrityAiJobs,
	integrityCases,
	integrityActions,
	organizations
} from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';

const result = {
	verdict: '建议复核',
	suspicionPercent: 45,
	evidenceQuality: '中',
	summary: '核对步兵窗口。',
	reasons: [{ text: '6次击杀对应2 KPM', evidence: 'case.snapshot.infantryKills' }],
	alternatives: ['连续交战可能解释'],
	contradictions: [],
	missingEvidence: []
};
test('insufficient evidence and low quality cannot produce confident percentages', () => {
	expect(reviewOutput.safeParse(result).success).toBe(true);
	expect(reviewOutput.safeParse({ ...result, verdict: '证据不足' }).success).toBe(false);
	expect(
		reviewOutput.safeParse({ ...result, verdict: '证据不足', suspicionPercent: null }).success
	).toBe(true);
	expect(
		reviewOutput.safeParse({ ...result, evidenceQuality: '低', suspicionPercent: 90 }).success
	).toBe(false);
});
describe.skipIf(!hasTestDb)('automatic AI initial review', () => {
	test('persistent automatic queue respects budget, pause, closed cases; never reviews or punishes for a human', async () => {
		const env = await testEnv(),
			w = await seedWorld(env);
		await acquireOrRenew(env, 'ai-queue-tests');
		await saveAiSettings(env, w.org.id, {
			baseUrl: 'https://example.com/v1',
			apiKey: 'test-secret',
			model: 'test-model',
			dailyLimit: 1
		});
		const add = async (extra: Record<string, unknown> = {}) => {
			const id = crypto.randomUUID();
			await env.db.insert(integrityCases).values({
				id,
				orgId: w.org.id,
				serverId: w.server.id,
				steamId: '76561198000777777',
				createdAt: new Date(),
				confidence: 'HIGH',
				trigger: 'test',
				ruleVersion: 1,
				riskScore: 40,
				riskBreakdown: [],
				snapshot: { infantryKills: 6, kpm180: 2 },
				...extra
			});
			return id;
		};
		const first = await add();
		await add({ reviewedAt: new Date() });
		await add({ createdAt: new Date(Date.now() - 8 * 86400000) });
		await add({ status: 'CLOSED' });
		await discoverAiJobs(env);
		await discoverAiJobs(env);
		let calls = 0;
		const reviewer: typeof aiCall = (env, org, op, bundle, automatic) =>
			aiCall(env, org, op, bundle, automatic, async (_base, _key, _path, body) => {
				calls++;
				const payload = JSON.stringify(body);
				expect(payload).toContain('numericChecks');
				expect(payload).not.toContain('test-secret');
				expect(payload).not.toContain('image_url');
				return {
					choices: [{ message: { content: JSON.stringify(result) }, finish_reason: 'stop' }]
				};
			});
		expect(
			(await env.db.select().from(integrityAiJobs)).filter((j) => j.caseId === first)
		).toHaveLength(1);
		expect(await processNextAiJob(env, reviewer)).toBe(true);
		const [done] = await env.db
			.select()
			.from(integrityAiJobs)
			.where(eq(integrityAiJobs.caseId, first));
		expect(done.state).toBe('done');
		expect(done.result).toMatchObject(result);
		const [unchanged] = await env.db
			.select()
			.from(integrityCases)
			.where(eq(integrityCases.id, first));
		expect(unchanged.reviewedAt).toBeNull();
		expect(unchanged.status).toBe('OPEN');
		expect(unchanged.riskScore).toBe(40);
		expect(
			await env.db.select().from(integrityActions).where(eq(integrityActions.caseId, first))
		).toHaveLength(0);
		const second = await add();
		await discoverAiJobs(env);
		await env.db
			.update(integrityAiSettings)
			.set({ lastRequestAt: null })
			.where(eq(integrityAiSettings.orgId, w.org.id));
		expect(await processNextAiJob(env, reviewer)).toBe(false);
		expect(calls).toBe(1);
		await env.db
			.update(integrityAiSettings)
			.set({ budgetDay: 'yesterday', autoEnabled: false })
			.where(eq(integrityAiSettings.orgId, w.org.id));
		expect(await processNextAiJob(env, reviewer)).toBe(false);
		await env.db
			.update(integrityAiSettings)
			.set({ autoEnabled: true })
			.where(eq(integrityAiSettings.orgId, w.org.id));
		await env.db
			.update(organizations)
			.set({ suspendedAt: new Date() })
			.where(eq(organizations.id, w.org.id));
		expect(await processNextAiJob(env, reviewer)).toBe(false);
		await env.db
			.update(organizations)
			.set({ suspendedAt: null })
			.where(eq(organizations.id, w.org.id));
		expect(await processNextAiJob(env, reviewer)).toBe(true);
		expect(calls).toBe(2);
		const [settings] = await env.db
			.select()
			.from(integrityAiSettings)
			.where(eq(integrityAiSettings.orgId, w.org.id));
		expect(settings.dailyRequests).toBe(1);
		await env.db
			.update(integrityAiJobs)
			.set({ result: { ...result, promptVersion: 'integrity-triage-v2' } })
			.where(eq(integrityAiJobs.caseId, first));
		await discoverAiJobs(env);
		expect(
			(await env.db.select().from(integrityAiJobs).where(eq(integrityAiJobs.caseId, first)))[0]
				.state
		).toBe('pending');
		const third = await add();
		await discoverAiJobs(env);
		await env.db
			.update(integrityCases)
			.set({ reviewedAt: new Date() })
			.where(eq(integrityCases.id, third));
		await discoverAiJobs(env);
		expect(
			(await env.db.select().from(integrityAiJobs).where(eq(integrityAiJobs.caseId, third)))[0]
				.state
		).toBe('skipped');
		expect(
			(await env.db.select().from(integrityAiJobs).where(eq(integrityAiJobs.caseId, second)))[0]
				.state
		).toBe('done');
	});
	test('invalid provider output has bounded retries without writing a successful review', async () => {
		const env = await testEnv(),
			w = await seedWorld(env);
		await acquireOrRenew(env, 'ai-queue-tests');
		await saveAiSettings(env, w.org.id, {
			baseUrl: 'https://example.com/v1',
			apiKey: 'test-secret',
			model: 'test-model'
		});
		const id = crypto.randomUUID();
		await env.db.insert(integrityCases).values({
			id,
			orgId: w.org.id,
			serverId: w.server.id,
			steamId: '76561198000777778',
			createdAt: new Date(),
			confidence: 'HIGH',
			trigger: 'test',
			ruleVersion: 1,
			riskScore: 0,
			riskBreakdown: [],
			snapshot: {}
		});
		await discoverAiJobs(env);
		const reviewer: typeof aiCall = (env, org, op, bundle, automatic) =>
			aiCall(env, org, op, bundle, automatic, async () => ({
				choices: [{ message: { content: 'invalid JSON' } }]
			}));
		for (let i = 0; i < 3; i++) {
			await env.db
				.update(integrityAiSettings)
				.set({ lastRequestAt: null })
				.where(eq(integrityAiSettings.orgId, w.org.id));
			await env.db
				.update(integrityAiJobs)
				.set({ nextAt: new Date(0) })
				.where(eq(integrityAiJobs.caseId, id));
			expect(await processNextAiJob(env, reviewer)).toBe(true);
		}
		const [job] = await env.db.select().from(integrityAiJobs).where(eq(integrityAiJobs.caseId, id));
		expect(job.state).toBe('error');
		expect(job.attempts).toBe(3);
		expect(job.result).toBeNull();
		expect(await processNextAiJob(env, reviewer)).toBe(false);
	});
});
