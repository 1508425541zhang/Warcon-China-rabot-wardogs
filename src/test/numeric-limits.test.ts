import { afterEach, describe, expect, spyOn, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import { seedWorld } from './world';
import { callApi, stubGateway } from './call';
import { ACTIONS } from '$lib/server/actions';
import { acquireOrRenew } from '$lib/server/leadership';
import { runNumericLimits } from '$lib/server/numeric-limits';
import {
	numericLimitRules,
	numericLimitEvents,
	servers,
	matches,
	playerSessions,
	playerProgressSamples
} from '$lib/server/db/schema';
import { defaultNumericLimits } from '$lib/numeric-limit-policy';
import type { Player, Status } from '$lib/types';
import type { WardogsClient } from '$lib/server/rcon';
describe.skipIf(!hasTestDb)('numeric hard limits', () => {
	const mocks: { mockRestore: () => void }[] = [];
	afterEach(() => {
		mocks.splice(0).forEach((m) => m.mockRestore());
	});
	async function setup() {
		const env = await testEnv(),
			world = await seedWorld(env);
		await acquireOrRenew(env, 'numeric-test');
		const [server] = await env.db.select().from(servers).where(eq(servers.id, world.server.id));
		const now = new Date(),
			old = new Date(now.getTime() - 300000);
		const [round] = await env.db
			.insert(matches)
			.values({ serverId: server.id, map: 'Europe', startedAt: old })
			.returning();
		const player: Player = {
			steamId: '76561198000000001',
			name: 'test',
			faction: 'A',
			kills: 20,
			deaths: 2,
			cash: 1000,
			ping: 10
		};
		await env.db
			.insert(playerSessions)
			.values({
				serverId: server.id,
				steamId: player.steamId,
				name: player.name,
				joinedAt: old,
				lastSeen: now
			});
		await env.db
			.insert(playerProgressSamples)
			.values(
				[90, 60, 30].map((s, i) => ({
					serverId: server.id,
					matchId: round.id,
					bucket: Math.floor(now.getTime() / 30000) - 3 + i,
					observedAt: new Date(now.getTime() - s * 1000),
					players: [{ ...player, kills: 10 + i * 3, cash: 100 + i * 200 }]
				}))
			);
		const config = { ...defaultNumericLimits, enabled: true, kpm: 3, windowSeconds: 60 };
		const enable = () =>
			env.db.insert(numericLimitRules).values({ serverId: server.id, config, updatedAt: old });
		const warning = spyOn(ACTIONS.whisper, 'run').mockResolvedValue({} as never),
			kick = spyOn(ACTIONS.kick, 'run').mockResolvedValue({} as never);
		mocks.push(warning, kick);
		const run = () =>
			runNumericLimits(env, server, {} as WardogsClient, {
				players: [player],
				status: { map: 'Europe', scores: [], scoreCap: null } as unknown as Status,
				statusAt: Date.now(),
				trusted: true,
				boundary: false,
				now
			});
		return { env, world, server, round, player, now, enable, warning, kick, run };
	}
	test('default off; first warning, no repeat on same window; fresh post-warning window kicks once', async () => {
		const t = await setup();
		await t.run();
		expect(t.warning).not.toHaveBeenCalled();
		await t.enable();
		await t.run();
		await t.run();
		expect(t.warning).toHaveBeenCalledTimes(1);
		expect(t.kick).not.toHaveBeenCalled();
		await t.env.db
			.update(numericLimitEvents)
			.set({ updatedAt: new Date(t.now.getTime() - 61000) })
			.where(eq(numericLimitEvents.serverId, t.server.id));
		await t.run();
		await t.run();
		expect(t.kick).toHaveBeenCalledTimes(1);
	});
	test('failed warning never qualifies for kick', async () => {
		const t = await setup();
		await t.enable();
		t.warning.mockRejectedValue(new Error('timeout'));
		await t.run();
		await t.run();
		expect(t.warning).toHaveBeenCalledTimes(1);
		expect(t.kick).not.toHaveBeenCalled();
	});
	test('permissions and limits validated by endpoint', async () => {
		const t = await setup();
		stubGateway();
		const { POST } = await import('../routes/api/servers/[id]/numeric-limits/+server');
		const args = {
			method: 'POST',
			params: { id: t.server.id },
			body: { ...defaultNumericLimits, enabled: true, kpm: 4 }
		};
		expect((await callApi(POST, t.world.users.viewer, args)).status).toBe(403);
		expect(
			(await callApi(POST, t.world.users.owner, { ...args, body: { ...args.body, kpm: -1 } }))
				.status
		).toBe(400);
		expect((await callApi(POST, t.world.users.owner, args)).status).toBe(200);
	});
});
