import { describe, expect, test } from 'bun:test';
import { and, eq } from 'drizzle-orm';
import { pluginManifestSchema } from '$lib/plugins/sdk';
import { readPluginJson } from '$lib/server/personal-plugins';
import { personalPlugins, serverLive, serverGrants } from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callApi } from './call';

const manifest = {
	apiVersion: 1,
	id: 'my-plugin',
	name: '我的插件',
	widgets: [{ type: 'metric', title: '在线', metric: 'online' }]
};
test('plugin JSON rejects script/CSS injection, unknown fields and incompatible API versions', () => {
	expect(pluginManifestSchema.safeParse(manifest).success).toBe(true);
	for (const bad of [
		{ ...manifest, apiVersion: 2 },
		{ ...manifest, script: 'alert(1)' },
		{ ...manifest, style: { accent: 'red;position:fixed' } },
		{ ...manifest, widgets: [{ type: 'html', html: '<script/>' }] },
		{ ...manifest, id: '../escape' }
	])
		expect(pluginManifestSchema.safeParse(bad).success).toBe(false);
});
test('upload parser bounds real streamed body size and rejects malformed JSON', async () => {
	await expect(
		readPluginJson(
			new Request('http://test', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: 'x'.repeat(65_537)
			})
		)
	).rejects.toMatchObject({ status: 413 });
	await expect(
		readPluginJson(
			new Request('http://test', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: '{'
			})
		)
	).rejects.toMatchObject({ status: 400 });
});

describe.skipIf(!hasTestDb)('personal plugin permissions and persistence', () => {
	test('account isolation, CRUD, disabled data and scoped API-key snapshot', async () => {
		const env = await testEnv();
		const w = await seedWorld(env);
		const collection = await import('../routes/api/plugins/+server');
		const item = await import('../routes/api/plugins/[pluginId]/+server');
		const data = await import('../routes/api/plugins/[pluginId]/data/+server');
		const snapshot = await import('../routes/api/servers/[id]/plugin-snapshot/+server');
		const body = { manifest, serverId: w.server.id, enabled: true };
		const params = { pluginId: 'my-plugin' };
		expect((await callApi(collection.GET, null)).status).toBe(401);
		expect((await callApi(collection.POST, w.users.owner, { method: 'POST', body })).status).toBe(
			201
		);
		expect((await callApi(collection.POST, w.users.owner, { method: 'POST', body })).status).toBe(
			409
		);
		expect((await callApi(item.GET, w.users.viewer, { params })).status).toBe(404);
		expect((await callApi(item.DELETE, w.users.viewer, { method: 'DELETE', params })).status).toBe(
			404
		);
		expect(
			(
				await callApi(collection.POST, w.users.viewer, {
					method: 'POST',
					body: { ...body, serverId: w.otherOrgServer.id }
				})
			).status
		).toBe(404);
		expect((await callApi(collection.GET, w.users.keyView)).status).toBe(403);
		expect(
			(
				await callApi(collection.POST, w.users.owner, {
					method: 'POST',
					body: {
						...body,
						manifest: { ...manifest, id: 'unknown-component', renderer: 'not-installed' }
					}
				})
			).code
		).toBe('unknown_renderer');
		const at = new Date();
		await env.db
			.insert(serverLive)
			.values({
				serverId: w.server.id,
				playersAt: at,
				statusAt: at,
				status: { map: 'Kavkazi', secret: 'MUST_NOT_LEAK' },
				players: [
					{
						name: '玩家甲',
						steamId: '76561198000000001',
						faction: 'Blue',
						kills: 3,
						deaths: 1,
						cash: 100,
						ping: 30,
						secret: 'MUST_NOT_LEAK'
					}
				]
			})
			.onConflictDoUpdate({
				target: serverLive.serverId,
				set: {
					playersAt: at,
					statusAt: at,
					status: { map: 'Kavkazi', secret: 'MUST_NOT_LEAK' },
					players: [
						{
							name: '玩家甲',
							steamId: '76561198000000001',
							faction: 'Blue',
							kills: 3,
							deaths: 1,
							cash: 100,
							ping: 30,
							secret: 'MUST_NOT_LEAK'
						}
					]
				}
			});
		const result = await callApi(data.GET, w.users.owner, { params });
		expect(result.status).toBe(200);
		expect(JSON.stringify(result.body)).not.toContain('MUST_NOT_LEAK');
		expect(
			(result.body as { snapshot: { metrics: { kills: number } } }).snapshot.metrics.kills
		).toBe(3);
		expect(
			(await callApi(snapshot.GET, w.users.keyView, { params: { id: w.server.id } })).status
		).toBe(200);
		expect(
			(await callApi(snapshot.GET, w.users.keyView, { params: { id: w.otherOrgServer.id } })).status
		).toBe(404);
		expect(
			(
				await callApi(item.PUT, w.users.owner, {
					method: 'PUT',
					params,
					body: { ...body, enabled: false }
				})
			).status
		).toBe(200);
		expect((await callApi(data.GET, w.users.owner, { params })).code).toBe('plugin_disabled');
		expect((await callApi(item.DELETE, w.users.owner, { method: 'DELETE', params })).status).toBe(
			200
		);
		expect((await callApi(item.GET, w.users.owner, { params })).status).toBe(404);
	});
	test('revoking a viewer grant stops previously installed plugin data', async () => {
		const env = await testEnv();
		const w = await seedWorld(env);
		const { POST } = await import('../routes/api/plugins/+server');
		const { GET } = await import('../routes/api/plugins/[pluginId]/data/+server');
		expect(
			(
				await callApi(POST, w.users.viewer, {
					method: 'POST',
					body: { manifest, serverId: w.server.id }
				})
			).status
		).toBe(201);
		await env.db
			.delete(serverGrants)
			.where(
				and(eq(serverGrants.userId, w.users.viewer!.id), eq(serverGrants.serverId, w.server.id))
			);
		expect((await callApi(GET, w.users.viewer, { params: { pluginId: manifest.id } })).status).toBe(
			404
		);
	});
	test('concurrent installs cannot exceed the per-user limit', async () => {
		const env = await testEnv();
		const w = await seedWorld(env);
		await env.db.insert(personalPlugins).values(
			Array.from({ length: 19 }, (_, i) => ({
				userId: w.users.owner!.id,
				pluginId: `existing-${i}`,
				manifest: { ...manifest, id: `existing-${i}` }
			}))
		);
		const { POST } = await import('../routes/api/plugins/+server');
		const results = await Promise.all(
			['one', 'two', 'three'].map((id) =>
				callApi(POST, w.users.owner, {
					method: 'POST',
					body: { manifest: { ...manifest, id: `extra-${id}` } }
				})
			)
		);
		expect(results.filter((r) => r.status === 201)).toHaveLength(1);
		expect(results.filter((r) => r.code === 'plugin_limit')).toHaveLength(2);
	});
});
