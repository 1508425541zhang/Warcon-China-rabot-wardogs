import { describe, test, expect } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callLoad, stubGateway } from './call';
import { integrityCases } from '$lib/server/db/schema';
import { load } from '../routes/(app)/server/[id]/integrity/cases/+page.server';
describe.skipIf(!hasTestDb)('case archive pagination', () => {
	test('all history is reachable, stable with new arrivals, bounded and tenant scoped', async () => {
		const env = await testEnv(),
			world = await seedWorld(env);
		stubGateway();
		const prefix = crypto.randomUUID(),
			createdAt = new Date(Date.now() - 10000);
		const row = (id: string) => ({
			id,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId: '76561198000000001',
			createdAt,
			confidence: 'B',
			trigger: 'ABNORMAL_INFANTRY_WINDOW',
			ruleVersion: 1,
			riskScore: 55,
			riskBreakdown: [],
			snapshot: {}
		});
		await env.db
			.insert(integrityCases)
			.values(Array.from({ length: 45 }, (_, i) => row(`${prefix}-${String(i).padStart(3, '0')}`)));
		await env.db
			.insert(integrityCases)
			.values({
				...row(`${prefix}-foreign`),
				orgId: world.otherOrg.id,
				serverId: world.otherOrgServer.id
			});
		const params = { id: world.server.id };
		const first = (await callLoad(load, world.users.owner, { params })).body as any;
		expect(first.total).toBe(45);
		expect(first.cases).toHaveLength(20);
		expect(first.pages).toBe(3);
		await env.db
			.insert(integrityCases)
			.values({ ...row(`${prefix}-new`), createdAt: new Date(Date.parse(first.before) + 1) });
		const query = `before=${encodeURIComponent(first.before)}`;
		const second = (await callLoad(load, world.users.owner, { params, query: `page=2&${query}` }))
			.body as any;
		const third = (await callLoad(load, world.users.owner, { params, query: `page=999&${query}` }))
			.body as any;
		expect(third.page).toBe(3);
		expect(third.cases).toHaveLength(5);
		expect(new Set([...first.cases, ...second.cases, ...third.cases].map((c) => c.id)).size).toBe(
			45
		);
		expect((await callLoad(load, world.users.outsider, { params })).status).not.toBe(200);
		const overview = await import('../routes/(app)/server/[id]/integrity/+page.server');
		expect(
			((await callLoad(overview.load, world.users.owner, { params })).body as any).cases
		).toHaveLength(5);
	});
	test('empty archive and malformed page remain usable', async () => {
		const env = await testEnv(),
			world = await seedWorld(env);
		const result = (
			await callLoad(load, world.users.owner, {
				params: { id: world.server.id },
				query: 'page=NaN&before=bad'
			})
		).body as any;
		expect(result.page).toBe(1);
		expect(result.pages).toBe(1);
		expect(result.cases).toEqual([]);
	});
});
