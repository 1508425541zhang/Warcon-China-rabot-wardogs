import { describe, test, expect } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import {
	servers,
	factionLockRules,
	factionLockEvents,
	factionMovePermits,
	matches
} from '$lib/server/db/schema';
import { runFactionLock } from '$lib/server/faction-lock';
import { acquireOrRenew } from '$lib/server/leadership';
import { eq } from 'drizzle-orm';
import { GameError, WardogsClient } from '$lib/server/rcon';
import type { Player, Status } from '$lib/types';
describe.skipIf(!hasTestDb)('faction lock real state machine', () => {
	async function setup(capacities: Record<string, number> = {}) {
		const env = await testEnv(),
			w = await seedWorld(env);
		await acquireOrRenew(env, 'faction-test');
		const [server] = await env.db.select().from(servers).where(eq(servers.id, w.server.id));
		const now = new Date(),
			old = new Date(now.getTime() - 600000);
		const [round] = await env.db
			.insert(matches)
			.values({ serverId: w.server.id, map: 'Europe', startedAt: old })
			.returning();
		await env.db
			.insert(factionLockRules)
			.values({ serverId: w.server.id, enabled: true, graceSeconds: 120, capacities });
		const p: Player = {
			steamId: '76561198000777777',
			name: 'player',
			faction: 'B',
			kills: 0,
			deaths: 0,
			cash: 0,
			ping: 20
		};
		const calls: string[] = [];
		const client = {
			json: async (method: string, path: string) => {
				calls.push(method + ' ' + path);
				return {};
			}
		} as unknown as WardogsClient;
		const status = {
			map: 'Europe',
			scores: [
				{ name: 'A', score: 10 },
				{ name: 'B', score: 9 },
				{ name: 'C', score: 8 }
			],
			matchSeconds: 600,
			scoreCap: null
		} as Status;
		const input = {
			players: [p, { ...p, steamId: '76561198000777778', faction: 'A' }],
			status,
			statusAt: now.getTime(),
			trusted: true,
			changes: [],
			startupAt: old.getTime(),
			boundary: false,
			now
		};
		const [e] = await env.db
			.insert(factionLockEvents)
			.values({
				serverId: w.server.id,
				matchId: round.id,
				steamId: p.steamId,
				fromFaction: 'A',
				toFaction: 'B',
				state: 'pending',
				reason: 'test',
				createdAt: new Date(now.getTime() - 20000),
				updatedAt: new Date(now.getTime() - 20000)
			})
			.returning();
		const run = () => runFactionLock(env, server, client, input);
		const state = async () =>
			(await env.db.select().from(factionLockEvents).where(eq(factionLockEvents.id, e.id)))[0]
				.state;
		return { env, w, server, p, e, input, client, calls, run, state };
	}
	test('free/unknown seats restore via PATCH and respawn, never kick', async () => {
		const t = await setup();
		await t.run();
		expect(await t.state()).toBe('restored');
		expect(t.calls.some((c) => c.includes('/kick'))).toBe(false);
		expect(t.calls[0]).toContain('PATCH');
	});
	test('full leading original faction warns before kicking on later fresh evaluation', async () => {
		const t = await setup({ A: 1 });
		await t.run();
		expect(await t.state()).toBe('warned');
		expect(t.calls).toHaveLength(1);
		expect(t.calls[0]).toContain('/message');
		t.input.now = new Date(t.input.now.getTime() + 9000);
		t.input.statusAt = t.input.now.getTime();
		await t.run();
		expect(await t.state()).toBe('kicked');
		expect(t.calls[1]).toContain('/kick');
		await t.run();
		expect(t.calls).toHaveLength(2);
	});
	test('full non-leading faction keeps player without warning or kicking', async () => {
		const t = await setup({ A: 1 });
		t.input.status.scores[0].score = 1;
		await t.run();
		expect(await t.state()).toBe('skipped');
		expect(t.calls).toHaveLength(0);
	});
	test('lead disappearing after warning cancels kick', async () => {
		const t = await setup({ A: 1 });
		await t.run();
		t.input.now = new Date(t.input.now.getTime() + 9000);
		t.input.statusAt = t.input.now.getTime();
		t.input.status.scores[0].score = 0;
		await t.run();
		expect(await t.state()).toBe('skipped');
		expect(t.calls).toHaveLength(1);
	});
	test('administrator or balancer permit exempts observed move', async () => {
		const t = await setup();
		await t.env.db.insert(factionMovePermits).values({
			serverId: t.w.server.id,
			steamId: t.p.steamId,
			faction: 'B',
			expiresAt: new Date(Date.now() + 120000)
		});
		await t.run();
		expect(await t.state()).toBe('skipped');
		expect(t.calls).toHaveLength(0);
	});
	test('round transition cancels pending enforcement', async () => {
		const t = await setup();
		t.input.boundary = true;
		await t.run();
		expect(await t.state()).toBe('skipped');
		expect(t.calls).toHaveLength(0);
	});
	test('network failure is not evidence of a full team', async () => {
		const t = await setup();
		t.client.json = async () => {
			throw new GameError(502, 'network', 'unreachable');
		};
		await t.run();
		expect(await t.state()).toBe('error');
		expect(t.calls).toHaveLength(0);
	});
	test('Warcon raw PATCH writes an exemption before transport, covering all action paths', async () => {
		const t = await setup();
		const client = new WardogsClient(t.env, t.server, 'test', 'faction-test-demo');
		await client.raw('PATCH', `/v1/players/${t.p.steamId}`, JSON.stringify({ faction: 'C' }));
		const rows = await t.env.db
			.select()
			.from(factionMovePermits)
			.where(eq(factionMovePermits.serverId, t.w.server.id));
		expect(rows[0].faction).toBe('C');
	});
});
