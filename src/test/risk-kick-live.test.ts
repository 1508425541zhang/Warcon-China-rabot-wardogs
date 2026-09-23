// The worker's own look at a server, end to end against the database and the mock game: a player
// the risk kick rule matches is kicked however they come to be on the server, including a
// reconnect inside the leave grace, which keeps the session and is no join; a reserved slot is
// spared even on the first look after a start, before the reserved list has been read; and a
// kick that does not land is not asked for again at every sweep.
import { afterAll, beforeAll, describe, expect, spyOn, test } from 'bun:test';
import { and, eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { organizations, outbox, playerMarks, servers, triggers } from '$lib/server/db/schema';
import { newId } from '$lib/server/http';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import { forgetMemory, memoryFor, observeServer } from '$lib/server/observe';
import { WardogsClient } from '$lib/server/rcon';
import { validateConfig } from '$lib/server/trigger-rules';
import { invalidateTriggers, RISK_REKICK_MS } from '$lib/server/triggers';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

const GOOD = '76561198000000601';
const BAD = '76561198000000602';
const LATE = '76561198000000603';
// on the mock game's reserved list from the start
const RESERVED = '76561198100000201';
const on = (...ids: string[]) =>
	ids.map((steamId) => ({ name: `p${steamId.slice(-3)}`, steamId, faction: null, kills: 0 }));

describe.skipIf(!hasTestDb)('the risk kick rule on a live look', () => {
	let env: Env;
	let w: World;
	const lists = new Map<string, ReturnType<typeof on>>();
	let spy: ReturnType<typeof spyOn>;

	beforeAll(async () => {
		env = { ...(await testEnv()), STEAM_API_KEY: '' };
		w = await seedWorld(env);
		expect(await acquireOrRenew(env, 'risk-kick-live')).toBe(true);
		// The mock game answers everything but the player list, which the test scripts per server.
		spy = spyOn(WardogsClient, 'forServer').mockImplementation(async (_env, server) => {
			const client = new WardogsClient(
				env,
				{ host: 'demo', port: 1, scheme: 'http' },
				'demo',
				`risk-kick-${server.id}`
			);
			const raw = client.raw.bind(client);
			client.raw = async (method, path, body, headers) =>
				method === 'GET' && path === '/v1/players'
					? {
							status: 200,
							statusText: 'OK',
							headers: { 'content-type': 'application/json' },
							text: JSON.stringify({ players: lists.get(server.id) ?? [] })
						}
					: raw(method, path, body, headers);
			return client;
		});
		await env.db.insert(playerMarks).values(
			[BAD, LATE, RESERVED].map((steamId) => ({
				orgId: w.org.id,
				steamId,
				watched: true,
				reason: 'note'
			}))
		);
	});

	afterAll(async () => {
		spy.mockRestore();
		forgetMemory(w.server.id);
		forgetMemory(w.otherServer.id);
		await releaseOwnership(env);
	});

	const rule = async (serverId: string) => {
		await env.db.insert(triggers).values({
			id: newId(),
			serverId,
			orgId: w.org.id,
			kind: 'risk_kick',
			name: 'Kick on connect risk',
			enabled: true,
			config: validateConfig('risk_kick', { watchlist: true, reason: 'no' })
		});
		invalidateTriggers(serverId);
	};
	const watch = async (serverId: string) => {
		const [server] = await env.db.select().from(servers).where(eq(servers.id, serverId));
		const [org] = await env.db.select().from(organizations).where(eq(organizations.id, w.org.id));
		const m = memoryFor(server, org);
		const look = async (...ids: string[]) => {
			lists.set(serverId, on(...ids));
			m.playersIntervalMs = 1000;
			await observeServer(env, m, { status: true, players: true });
		};
		const kicksOf = async (steamId: string) =>
			(
				await env.db
					.select()
					.from(outbox)
					.where(and(eq(outbox.serverId, serverId), eq(outbox.steamId, steamId)))
			).filter((r) => r.action === 'kick');
		return { m, look, kicksOf };
	};

	test('kicks at the join, again at a reconnect inside the grace, and whoever was on first', async () => {
		const { m, look, kicksOf } = await watch(w.server.id);
		// LATE is on before the rule exists
		await look(GOOD, LATE);
		await rule(w.server.id);

		// the rule's first look judges everyone on: LATE, on before it, is kicked
		await look(GOOD, LATE);
		expect(await kicksOf(LATE)).toHaveLength(1);
		expect(await kicksOf(GOOD)).toHaveLength(0);

		await look(GOOD, BAD);
		expect(await kicksOf(BAD)).toHaveLength(1);
		// kicked: off the list for a look, then straight back, well inside the leave grace
		await look(GOOD);
		await look(GOOD, BAD);
		expect(await kicksOf(BAD)).toHaveLength(2);
		// still on at the next look (the kick not landed yet): not kicked every look
		await look(GOOD, BAD);
		expect(await kicksOf(BAD)).toHaveLength(2);

		// a kick that did not land: the next sweep leaves BAD alone for a while, then asks again
		m.riskSweptAt = 0;
		await look(GOOD, BAD);
		expect(await kicksOf(BAD)).toHaveLength(2);
		m.riskKickedAt.set(BAD, Date.now() - RISK_REKICK_MS - 1);
		m.riskSweptAt = 0;
		await look(GOOD, BAD);
		expect(await kicksOf(BAD)).toHaveLength(3);
		expect(await kicksOf(GOOD)).toHaveLength(0);
	});

	test('the first look after a start spares a reserved slot the list has not been read for yet', async () => {
		await rule(w.otherServer.id);
		const { look, kicksOf } = await watch(w.otherServer.id);
		// a fresh worker: the rule exists, the reserved player and a watched one are on
		await look(RESERVED, LATE);
		await look(RESERVED, LATE);
		await look(RESERVED, LATE);
		expect(await kicksOf(RESERVED)).toHaveLength(0);
		expect(await kicksOf(LATE)).toHaveLength(1);
	});
});
