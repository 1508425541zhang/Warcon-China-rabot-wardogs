import { callApi, stubGateway } from './call';
import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import { seedWorld } from './world';
import { groupControlRules, groupControlScans, serverLive } from '$lib/server/db/schema';
import { defaultGroupConfig } from '$lib/group-control-policy';
import { scanGroups, groupControlView } from '$lib/server/group-control';
describe.skipIf(!hasTestDb)('group control listing', () => {
	test('requires automation management and validates config', async () => {
		const env = await testEnv(),
			world = await seedWorld(env);
		stubGateway();
		const { POST } = await import('../routes/api/servers/[id]/group-control/+server');
		const args = {
			method: 'POST',
			params: { id: world.server.id },
			body: { operation: 'save', config: defaultGroupConfig }
		};
		expect((await callApi(POST, world.users.viewer, args)).status).toBe(403);
		expect(
			(
				await callApi(POST, world.users.owner, {
					...args,
					body: { ...args.body, config: { ...defaultGroupConfig, minPlayers: 1 } }
				})
			).status
		).toBe(400);
		expect((await callApi(POST, world.users.owner, args)).status).toBe(200);
	});
	test('off/manual/auto, persisted results and stale roster protection', async () => {
		const env = await testEnv(),
			world = await seedWorld(env);
		const server = { id: world.server.id, orgId: world.org.id };
		const config = { ...defaultGroupConfig, mode: 'manual' as const, minPlayers: 2 };
		await scanGroups(env, server);
		expect((await groupControlView(env, server.id)).scan).toBeNull();
		await env.db.insert(groupControlRules).values({ serverId: server.id, config });
		await scanGroups(env, server);
		expect((await groupControlView(env, server.id)).scan).toBeNull();
		await expect(scanGroups(env, server, true)).rejects.toThrow('过期');
		await env.db
			.insert(serverLive)
			.values({
				serverId: server.id,
				ok: true,
				playersAt: new Date(),
				statusAt: new Date(),
				status: { scores: [{ name: 'A' }] },
				players: [1, 2].map((n) => ({
					name: `[ABCD] ${n}`,
					steamId: `7656119800000000${n}`,
					faction: 'A'
				}))
			})
			.onConflictDoUpdate({
				target: serverLive.serverId,
				set: {
					ok: true,
					playersAt: new Date(),
					statusAt: new Date(),
					status: { scores: [{ name: 'A' }] },
					players: [1, 2].map((n) => ({
						name: `[ABCD] ${n}`,
						steamId: `7656119800000000${n}`,
						faction: 'A'
					}))
				}
			});
		await scanGroups(env, server, true);
		expect((await groupControlView(env, server.id)).scan?.groups[0].members).toHaveLength(2);
		await expect(scanGroups(env, server, true)).rejects.toThrow('65');
		await env.db
			.update(groupControlRules)
			.set({ config: { ...config, mode: 'auto' } })
			.where(eq(groupControlRules.serverId, server.id));
		await env.db
			.update(groupControlScans)
			.set({ scannedAt: new Date(Date.now() - 70000) })
			.where(eq(groupControlScans.serverId, server.id));
		await scanGroups(env, server);
		expect((await groupControlView(env, server.id)).scan?.config.mode).toBe('auto');
	});
});
