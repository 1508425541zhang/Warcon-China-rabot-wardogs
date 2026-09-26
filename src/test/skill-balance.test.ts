import { afterEach, describe, expect, spyOn, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import { ACTIONS } from '$lib/server/actions';
import { acquireOrRenew } from '$lib/server/leadership';
import { runSkillBalance, skillBalanceView } from '$lib/server/skill-balance';
import {
	skillBalanceRules,
	skillBalanceRuns,
	servers,
	matches,
	playerSessions,
	kills,
	factionMovePermits
} from '$lib/server/db/schema';
import type { Player, Status } from '$lib/types';
import { GameError, type WardogsClient } from '$lib/server/rcon';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callApi, stubGateway } from './call';

describe.skipIf(!hasTestDb)('skill balance durable execution', () => {
	const mocks: { mockRestore: () => void }[] = [];
	afterEach(() => {
		for (const m of mocks) m.mockRestore();
		mocks.length = 0;
	});
	async function setup() {
		const env = await testEnv(),
			world = await seedWorld(env);
		await acquireOrRenew(env, 'skill-balance-test');
		const [server] = await env.db.select().from(servers).where(eq(servers.id, world.server.id));
		const now = new Date(),
			old = new Date(now.getTime() - 600000);
		const [round] = await env.db
			.insert(matches)
			.values({ serverId: server.id, map: 'Europe', startedAt: old })
			.returning();
		const status = {
			map: 'Europe',
			scores: [
				{ name: 'A', score: 110 },
				{ name: 'B', score: 60 },
				{ name: 'C', score: 40 }
			],
			matchSeconds: 600,
			scoreCap: 200
		} as Status;
		const players: Player[] = Array.from({ length: 6 }, (_, i) => ({
			steamId: `7656119800000000${i + 1}`,
			name: `p${i}`,
			faction: i < 3 ? 'A' : 'C',
			kills: 0,
			deaths: 0,
			cash: 0,
			ping: 10
		}));
		await env.db.insert(playerSessions).values(
			players.map((p) => ({
				serverId: server.id,
				steamId: p.steamId,
				name: p.name,
				faction: p.faction,
				joinedAt: old,
				lastSeen: now
			}))
		);
		await env.db.insert(kills).values(
			Array.from({ length: 10 }, (_, i) => ({
				serverId: server.id,
				matchRow: round.id,
				ts: i === 0 ? old : now,
				eventId: crypto.randomUUID(),
				instanceId: 'test-instance',
				matchId: 'test-match',
				eventTime: i === 0 ? 1 : 599 - i,
				map: 'Europe',
				killerSteamId: players[i % 3].steamId,
				victimSteamId: players[3 + (i % 3)].steamId,
				victimName: 'victim',
				tags: []
			}))
		);
		const moves: { steamId: string; faction: string }[] = [];
		mocks.push(spyOn(ACTIONS.status, 'run').mockImplementation(async () => status));
		mocks.push(
			spyOn(ACTIONS.players, 'run').mockImplementation(async () => ({
				players: players.map((p) => ({ ...p }))
			}))
		);
		const moveSpy = spyOn(ACTIONS.changeTeam, 'run').mockImplementation(async (_c, p) => {
			moves.push({ steamId: String(p.steamId), faction: String(p.faction) });
			players.find((v) => v.steamId === p.steamId)!.faction = String(p.faction);
			return { respawned: true, message: '' };
		});
		mocks.push(moveSpy);
		const input = {
			players: players.map((p) => ({ ...p })),
			status,
			statusAt: now.getTime(),
			trusted: true,
			startupAt: old.getTime(),
			boundary: false,
			now
		};
		const run = () => runSkillBalance(env, server, {} as WardogsClient, input);
		const enable = () =>
			env.db.insert(skillBalanceRules).values({ serverId: server.id, enabled: true });
		return { env, world, server, players, moves, moveSpy, input, status, run, enable };
	}
	test('default off; enabling swaps three pairs once and issues faction-lock permits', async () => {
		const t = await setup();
		expect((await skillBalanceView(t.env, t.server.id)).rule.enabled).toBe(false);
		await t.run();
		expect(t.moves).toHaveLength(0);
		await t.enable();
		await t.run();
		expect(t.moves).toHaveLength(6);
		expect((await skillBalanceView(t.env, t.server.id)).runs[0].state).toBe('done');
		expect(
			await t.env.db
				.select()
				.from(factionMovePermits)
				.where(eq(factionMovePermits.serverId, t.server.id))
		).toHaveLength(6);
		await t.run();
		expect(t.moves).toHaveLength(6);
	});
	test('full target never kicks, retries or pretends a completed swap', async () => {
		const t = await setup();
		await t.enable();
		t.moveSpy.mockImplementation(async () => {
			throw new GameError(400, 'full', 'faction_full');
		});
		await t.run();
		expect(t.moveSpy).toHaveBeenCalledTimes(1);
		expect((await skillBalanceView(t.env, t.server.id)).runs[0].state).toBe('error');
		await t.run();
		expect(t.moveSpy).toHaveBeenCalledTimes(1);
	});
	test('failed second move compensates the first only after confirming actual factions', async () => {
		const t = await setup();
		await t.enable();
		let n = 0;
		t.moveSpy.mockImplementation(async (_c, p) => {
			n++;
			if (n === 2) throw new GameError(400, 'full', 'faction_full');
			t.players.find((v) => v.steamId === p.steamId)!.faction = String(p.faction);
			return { respawned: true, message: '' };
		});
		await t.run();
		expect(n).toBe(3);
		expect(t.players.map((p) => p.faction)).toEqual(['A', 'A', 'A', 'C', 'C', 'C']);
		expect((await skillBalanceView(t.env, t.server.id)).runs[0].state).toBe('partial');
	});
	test('mid-operation disable stops further pairs and compensates current pair', async () => {
		const t = await setup();
		await t.enable();
		let n = 0;
		t.moveSpy.mockImplementation(async (_c, p) => {
			n++;
			t.players.find((v) => v.steamId === p.steamId)!.faction = String(p.faction);
			await t.env.db
				.update(skillBalanceRules)
				.set({ enabled: false })
				.where(eq(skillBalanceRules.serverId, t.server.id));
			return { respawned: true, message: '' };
		});
		await t.run();
		expect(n).toBe(2);
		expect(t.players.map((p) => p.faction)).toEqual(['A', 'A', 'A', 'C', 'C', 'C']);
	});
	test('map transition and insufficient lead prevent any dispatch', async () => {
		const t = await setup();
		await t.enable();
		t.input.boundary = true;
		await t.run();
		t.input.boundary = false;
		t.status.scores[1].score = 70;
		await t.run();
		expect(t.moves).toHaveLength(0);
		expect(
			await t.env.db
				.select()
				.from(skillBalanceRuns)
				.where(eq(skillBalanceRuns.serverId, t.server.id))
		).toHaveLength(0);
	});
	test('settings endpoint requires permissions and validates threshold', async () => {
		const t = await setup();
		stubGateway();
		const { POST } = await import('../routes/api/servers/[id]/skill-balance/+server');
		const args = {
			method: 'POST',
			params: { id: t.server.id },
			body: { enabled: true, graceSeconds: 300, leadPoints: 40 }
		};
		expect((await callApi(POST, t.world.users.viewer, args)).status).toBe(403);
		expect(
			(
				await callApi(POST, t.world.users.owner, {
					...args,
					body: { ...args.body, leadPoints: 39 }
				})
			).status
		).toBe(400);
		expect((await callApi(POST, t.world.users.owner, args)).status).toBe(200);
		expect((await skillBalanceView(t.env, t.server.id)).rule.enabled).toBe(true);
	});
	test('missing feed and newly joined players do not become low-performance candidates', async () => {
		const t = await setup();
		await t.enable();
		await t.env.db
			.update(playerSessions)
			.set({ joinedAt: new Date() })
			.where(eq(playerSessions.serverId, t.server.id));
		await t.run();
		expect(t.moves).toHaveLength(0);
		await t.env.db
			.update(playerSessions)
			.set({ joinedAt: new Date(Date.now() - 600000) })
			.where(eq(playerSessions.serverId, t.server.id));
		await t.env.db.delete(kills).where(eq(kills.serverId, t.server.id));
		await t.run();
		expect(t.moves).toHaveLength(0);
	});
	test('concurrent observers cannot execute a round twice', async () => {
		const t = await setup();
		await t.enable();
		await Promise.all([t.run(), t.run()]);
		expect(t.moves).toHaveLength(6);
		expect(
			await t.env.db
				.select()
				.from(skillBalanceRuns)
				.where(eq(skillBalanceRuns.serverId, t.server.id))
		).toHaveLength(1);
	});
});
