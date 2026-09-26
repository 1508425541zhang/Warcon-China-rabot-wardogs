import { describe, test, expect } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callApi, callLoad } from './call';
import {
	servers,
	matches,
	kills,
	weaponRestrictionRules,
	weaponRestrictionEvents
} from '$lib/server/db/schema';
import { runWeaponRestrictions } from '$lib/server/weapon-restrictions';
import { acquireOrRenew } from '$lib/server/leadership';
import { eq } from 'drizzle-orm';
import type { WardogsClient } from '$lib/server/rcon';
import type { Player, Status } from '$lib/types';
describe.skipIf(!hasTestDb)('weapon restriction persisted delivery', () => {
	async function setup() {
		const env = await testEnv(),
			w = await seedWorld(env);
		await acquireOrRenew(env, 'weapon-tests');
		const [server] = await env.db.select().from(servers).where(eq(servers.id, w.server.id));
		const now = new Date(),
			old = new Date(now.getTime() - 600000);
		const [round] = await env.db
			.insert(matches)
			.values({ serverId: server.id, map: 'Europe', startedAt: old })
			.returning();
		await env.db
			.insert(weaponRestrictionRules)
			.values({
				serverId: server.id,
				enabled: true,
				causes: ['Id.Item.M67Grenade'],
				groups: [],
				updatedAt: old
			});
		const player = {
			steamId: '76561198000777777',
			name: '玩家',
			faction: 'A',
			kills: 0,
			deaths: 0,
			cash: 0,
			ping: 20
		} as Player;
		const input = {
			players: [player],
			status: { map: 'Europe', matchSeconds: 600, scoreCap: null, scores: [] } as unknown as Status,
			statusAt: now.getTime(),
			boundary: false,
			now
		};
		const calls: string[] = [];
		let fail = false;
		const client = {
			json: async (_method: string, path: string) => {
				calls.push(path);
				if (fail) throw new Error('failed');
				return {};
			}
		} as unknown as WardogsClient;
		const add = async (eventId: string, eventTime: number, extra: Record<string, unknown> = {}) => {
			await env.db
				.insert(kills)
				.values({
					serverId: server.id,
					ts: new Date(Date.now() + 10),
					eventId,
					instanceId: 'i',
					matchId: 'm',
					matchRow: round.id,
					eventTime,
					map: 'Europe',
					killerSteamId: player.steamId,
					killerName: player.name,
					victimSteamId: '76561198000777778',
					victimName: 'victim',
					cause: 'Id.Item.M67Grenade',
					tags: [],
					...extra
				});
		};
		return {
			env,
			w,
			server,
			round,
			input,
			calls,
			add,
			run: () => runWeaponRestrictions(env, server, client, input),
			fail: () => {
				fail = true;
			}
		};
	}
	test('warn once for multi-kill, kick on new post-warning kill, replay cannot kick twice', async () => {
		const s = await setup();
		await s.add('one', 590);
		await s.add('same-explosion', 591);
		await s.run();
		expect(s.calls).toHaveLength(1);
		expect(s.calls[0]).toEndWith('/message');
		await s.run();
		expect(s.calls).toHaveLength(1);
		s.input.status.matchSeconds = 612;
		await s.add('two', 611);
		await s.run();
		expect(s.calls).toHaveLength(2);
		expect(s.calls[1]).toEndWith('/kick');
		await s.run();
		expect(s.calls).toHaveLength(2);
		const events = await s.env.db
			.select()
			.from(weaponRestrictionEvents)
			.where(eq(weaponRestrictionEvents.serverId, s.server.id));
		expect(events.map((e) => e.state)).toEqual(['delivered', 'delivered']);
	});
	test('failed warning is never kick eligibility', async () => {
		const s = await setup();
		s.fail();
		await s.add('failed', 590);
		await s.run();
		s.input.status.matchSeconds = 612;
		await s.add('later', 611);
		await s.run();
		expect(s.calls.every((p) => p.endsWith('/message'))).toBe(true);
	});
	test('offline, stale, map transition and unrelated source do not act', async () => {
		const s = await setup();
		await s.add('allowed', 599, { cause: 'Id.Item.M4' });
		await s.run();
		expect(s.calls).toHaveLength(0);
		await s.add('restricted', 599);
		s.input.boundary = true;
		await s.run();
		s.input.boundary = false;
		s.input.statusAt = Date.now() - 31000;
		await s.run();
		s.input.statusAt = Date.now();
		s.input.players = [];
		await s.run();
		expect(s.calls).toHaveLength(0);
	});
	test('automation contains both settings and API denies viewers or empty enabled list', async () => {
		const s = await setup();
		const { load } = await import('../routes/(app)/server/[id]/automation/+page.server');
		const loaded = await callLoad(load, s.w.users.owner, { params: { id: s.server.id } });
		expect(loaded.status).toBe(200);
		expect(loaded.body).toHaveProperty('weaponRestriction');
		expect(loaded.body).toHaveProperty('factionLock');
		const { POST } = await import('../routes/api/servers/[id]/weapon-restrictions/+server');
		const request = {
			method: 'POST',
			params: { id: s.server.id },
			body: { enabled: true, causes: [], groups: [] }
		};
		expect((await callApi(POST, s.w.users.viewer, request)).status).toBe(403);
		expect((await callApi(POST, s.w.users.owner, request)).status).toBe(400);
	});
});
