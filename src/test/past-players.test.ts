// A server's past players: what someone who can open one server reads of who has played there,
// and that nothing of the org's other servers comes with it.
import { beforeAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import type { Env } from '$lib/server/env';
import { listEntries, playerSessions } from '$lib/server/db/schema';
import { listOf, serverListOf } from '$lib/server/lists';
import { newId } from '$lib/server/http';
import { hasTestDb, testEnv } from './db';
import { callApi } from './call';
import { seedWorld, type PrincipalName, type World } from './world';

const ROUTES = join(import.meta.dir, '..', 'routes', 'api');
const HERE = '76561198000000201';
const THERE = '76561198000000202';
const BOTH = '76561198000000203';

describe.skipIf(!hasTestDb)("a server's past players", () => {
	let env: Env;
	let w: World;

	const seen = async (who: PrincipalName, serverId: string, query = '') => {
		const { GET } = await import(join(ROUTES, 'servers/[id]/players/seen', '+server.ts'));
		return callApi(GET, w.users[who], { params: { id: serverId }, query });
	};
	const orgSeen = async (who: PrincipalName) => {
		const { GET } = await import(join(ROUTES, 'orgs/[id]/players', '+server.ts'));
		return callApi(GET, w.users[who], { params: { id: w.org.id } });
	};
	const ids = (o: { body: unknown }) =>
		(o.body as { players: { steamId: string }[] }).players.map((p) => p.steamId).sort();
	const player = (o: { body: unknown }, steamId: string) =>
		(o.body as { players: Record<string, unknown>[] }).players.find((p) => p.steamId === steamId);

	beforeAll(async () => {
		env = await testEnv();
		w = await seedWorld(env);
		const at = (h: number) => new Date(Date.now() - h * 3600_000);
		const session = (serverId: string, steamId: string, name: string, hoursAgo: number) => ({
			serverId,
			steamId,
			name,
			joinedAt: at(hoursAgo + 1),
			lastSeen: at(hoursAgo),
			leftAt: at(hoursAgo)
		});
		await env.db
			.insert(playerSessions)
			.values([
				session(w.server.id, HERE, 'Krieger', 2),
				session(w.server.id, BOTH, 'Vasquez', 3),
				session(w.otherServer.id, BOTH, 'Vasq-on-two', 1),
				session(w.otherServer.id, THERE, 'nightowl', 1),
				session(w.otherOrgServer.id, HERE, 'Krieger-elsewhere', 1)
			]);
		// HERE is banned on the OTHER server's own list only; THERE on the org's list.
		const otherOwn = await serverListOf(env, { id: w.otherServer.id, orgId: w.org.id }, 'ban');
		const orgBans = await listOf(env, w.org.id, 'ban');
		await env.db.insert(listEntries).values([
			{ id: newId(), listId: otherOwn.id, steamId: HERE, reason: 'two only', addedByName: 'x' },
			{ id: newId(), listId: orgBans.id, steamId: THERE, reason: 'org', addedByName: 'x' }
		]);
	});

	test('it lists who played on that server, and only what they did there', async () => {
		const answer = await seen('viewer', w.server.id);
		expect(answer.status).toBe(200);
		expect(ids(answer)).toEqual([BOTH, HERE].sort());
		expect(player(answer, BOTH)).toMatchObject({
			name: 'Vasquez',
			aliases: [],
			sessions: 1,
			servers: 1
		});
		expect(player(answer, HERE)).toMatchObject({ name: 'Krieger', aliases: [] });
		expect(JSON.stringify(answer.body)).not.toContain('on-two');
		expect(JSON.stringify(answer.body)).not.toContain('elsewhere');
	});

	test('seen within, a whole SteamID, the order and the paging', async () => {
		const old = '76561198000000204';
		const day = 86400_000;
		await env.db.insert(playerSessions).values([
			{
				serverId: w.server.id,
				steamId: old,
				name: 'Longago',
				joinedAt: new Date(Date.now() - 5 * day),
				lastSeen: new Date(Date.now() - 5 * day + 3600_000),
				leftAt: new Date(Date.now() - 5 * day + 3600_000)
			},
			{
				serverId: w.server.id,
				steamId: HERE,
				name: 'Krieger-then',
				joinedAt: new Date(Date.now() - 9 * day),
				lastSeen: new Date(Date.now() - 9 * day + 3600_000),
				leftAt: new Date(Date.now() - 9 * day + 3600_000)
			}
		]);
		// the last day leaves out who was last on five days ago, and still counts all of HERE's time
		const recent = await seen('viewer', w.server.id, 'since=1');
		expect(ids(recent)).toEqual([BOTH, HERE].sort());
		expect(player(recent, HERE)).toMatchObject({ sessions: 2, aliases: ['Krieger-then'] });
		expect((recent.body as { total: number }).total).toBe(2);
		expect(ids(await seen('viewer', w.server.id, 'since=7'))).toContain(old);
		// a whole SteamID finds that player and nobody else, whenever they played
		expect(ids(await seen('viewer', w.server.id, `q=${old}`))).toEqual([old]);
		expect(ids(await seen('viewer', w.server.id, `q=${old}&since=1`))).toEqual([]);
		// the server orders and pages
		const order = async (query: string) =>
			(
				(await seen('viewer', w.server.id, query)).body as { players: { steamId: string }[] }
			).players.map((p) => p.steamId);
		expect(await order('sort=lastSeen&dir=asc')).toEqual([old, BOTH, HERE]);
		expect(await order('sort=lastSeen&dir=desc&limit=1&offset=1')).toEqual([BOTH]);
	});

	test('no query reaches another server through it', async () => {
		const answer = await seen('viewer', w.server.id, `server=${w.otherServer.id}&q=nightowl`);
		expect(ids(answer)).toEqual([]);
		const bySteamId = await seen('viewer', w.server.id, `q=${THERE}`);
		expect(ids(bySteamId)).toEqual([]);
	});

	test('it opens to whoever can open the server, and to nobody else', async () => {
		for (const who of ['viewer', 'operator', 'admin', 'owner', 'site', 'keyView'] as const)
			expect({ who, status: (await seen(who, w.server.id)).status }).toEqual({ who, status: 200 });
		expect((await seen('anon', w.server.id)).status).toBe(401);
		for (const who of ['stranger', 'outsider', 'member', 'elsewhere', 'keyElsewhere'] as const)
			expect({ who, status: (await seen(who, w.server.id)).status }).toEqual({ who, status: 404 });
		expect((await seen('viewer', w.otherServer.id)).status).toBe(404);
		expect((await seen('viewer', w.otherOrgServer.id)).status).toBe(404);
	});

	test("a ban on another server's own list is not this server's, nor the org's", async () => {
		expect(player(await seen('viewer', w.server.id), HERE)).toMatchObject({ banned: null });
		// on the server that holds it, it shows, though the game has not placed it yet
		expect(player(await seen('elsewhere', w.otherServer.id), HERE)).toBeUndefined();
		expect(player(await seen('elsewhere', w.otherServer.id), THERE)).toMatchObject({
			banned: 'org'
		});
		// the org's page, for an owner who opens every server: a server's own list is 'server'
		const org = await orgSeen('owner');
		expect(player(org, HERE)).toMatchObject({ banned: 'server' });
		expect(player(org, THERE)).toMatchObject({ banned: 'org' });
	});

	test('it answers a burst of searches and then asks for a pause', async () => {
		const { GET } = await import(join(ROUTES, 'servers/[id]/players/seen', '+server.ts'));
		const { assertRate, resetRates } = await import('$lib/server/ratelimit');
		resetRates();
		for (let i = 0; i < 60; i++) assertRate(`seen:${w.users.viewer!.id}`, 60, 60_000);
		// callApi resets the limiter, so the handler is called as SvelteKit would call it
		const res = await GET({
			locals: { user: w.users.viewer, session: null, apiKey: null },
			params: { id: w.server.id },
			url: new URL('http://localhost/test'),
			request: new Request('http://localhost/test')
		} as never);
		expect(res.status).toBe(429);
	});
});
