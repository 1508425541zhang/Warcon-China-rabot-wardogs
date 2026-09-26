import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import { integrityCases, integrityLabels } from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callApi, callLoad, stubGateway } from './call';

describe.skipIf(!hasTestDb)('append-only Integrity review labels', () => {
	test('owner correction records versioned labels without rewriting case evidence or score', async () => {
		const env = await testEnv();
		stubGateway();
		const world = await seedWorld(env);
		const caseId = `CASE-LABEL-${world.server.id}`;
		await env.db.insert(integrityCases).values({
			id: caseId,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId: '76561198000000901',
			createdAt: new Date(),
			confidence: 'B',
			trigger: 'STATISTICAL_WINDOW',
			ruleVersion: 7,
			riskScore: 42,
			riskBreakdown: [],
			statistical: { modelVersion: 'ensemble-shadow-v1' },
			snapshot: { eventIds: ['event-1'], roundId: 'round-1' }
		});
		const { POST } =
			await import('../routes/api/orgs/[id]/integrity/cases/[caseId]/labels/+server');
		const input = {
			method: 'POST',
			params: { id: world.org.id, caseId },
			body: { label: 'FALSE_POSITIVE', reason: '击杀记录由管理员复核为误报' }
		};
		expect((await callApi(POST, world.users.operator, input)).status).toBe(403);
		expect(
			(
				await callApi(POST, world.users.owner, {
					...input,
					body: { label: 'FALSE_POSITIVE', reason: 'x' }
				})
			).status
		).toBe(400);
		expect((await callApi(POST, world.users.owner, input)).status).toBe(200);
		const [saved] = await env.db.select().from(integrityCases).where(eq(integrityCases.id, caseId));
		expect(saved.status).toBe('REVIEWED');
		expect(saved.reviewedBy).toBe(world.users.owner!.id);
		expect(saved.reviewedAt).not.toBeNull();
		expect((await callApi(POST, world.users.owner, input)).status).toBe(200);
		expect(saved.riskScore).toBe(42);
		expect(saved.snapshot).toEqual({ eventIds: ['event-1'], roundId: 'round-1' });
		const rows = await env.db
			.select()
			.from(integrityLabels)
			.where(eq(integrityLabels.caseId, caseId));
		expect(rows).toHaveLength(1);
		expect(rows[0].modelVersion).toBe('ensemble-shadow-v1');
		const { load } = await import('../routes/(app)/server/[id]/integrity/+page.server');
		const page = await callLoad(load, world.users.owner, { params: { id: world.server.id } });
		expect(page.status).toBe(200);
		const data = page.body as {
			cases: { id: string; status: string }[];
			labels: { caseId: string; label: string }[];
		};
		expect(data.cases.find((c) => c.id === caseId)?.status).toBe('REVIEWED');
		expect(data.labels.find((l) => l.caseId === caseId)?.label).toBe('FALSE_POSITIVE');
		expect(
			(
				await callApi(POST, world.users.owner, {
					...input,
					body: { label: 'INSUFFICIENT_EVIDENCE', reason: '重新审核后需要补充证据材料' }
				})
			).status
		).toBe(200);
		expect(
			await env.db.select().from(integrityLabels).where(eq(integrityLabels.caseId, caseId))
		).toHaveLength(2);
	});
});
