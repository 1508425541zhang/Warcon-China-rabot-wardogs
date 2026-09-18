// What View is: what is happening on the server. These tests read the answers a viewer gets and
// check that what belongs to another capability is not in them, however the page would have
// drawn it: data sent to the browser is read, shown or not.
import { beforeAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import type { Env } from '$lib/server/env';
import { listOf } from '$lib/server/lists';
import { listEntries, playerMarks, playerNotes, serverBans } from '$lib/server/db/schema';
import { newId } from '$lib/server/http';
import { hasTestDb, testEnv } from './db';
import { callApi, callLoad, stubGateway } from './call';
import { seedWorld, type PrincipalName, type World } from './world';

const ROUTES = join(import.meta.dir, '..', 'routes');
const PLAYER = '76561198000000042';

describe.skipIf(!hasTestDb)('what View shows', () => {
	let env: Env;
	let w: World;

	const get = async (who: PrincipalName, path: string, query = '') => {
		const { GET } = await import(join(ROUTES, path, '+server.ts'));
		const answer = await callApi(GET, w.users[who], {
			params: { id: w.server.id, steamId: PLAYER },
			query
		});
		expect({ who, path, status: answer.status }).toEqual({ who, path, status: 200 });
		return answer.body as Record<string, any>;
	};

	beforeAll(async () => {
		env = await testEnv();
		w = await seedWorld(env);
		await env.db.insert(playerNotes).values({
			orgId: w.org.id,
			steamId: PLAYER,
			authorId: w.users.admin!.id,
			authorName: 'admin',
			body: 'suspected alt of a banned player'
		});
		await env.db.insert(playerMarks).values({
			orgId: w.org.id,
			steamId: PLAYER,
			watched: true,
			reason: 'watch for team kills',
			updatedByName: 'admin'
		});
		await env.db.insert(serverBans).values({
			serverId: w.otherServer.id,
			steamId: PLAYER,
			reason: 'cheating on the other server',
			bannedBy: 'admin'
		});
		const bans = await listOf(env, w.org.id, 'ban');
		await env.db.insert(listEntries).values({
			id: newId(),
			listId: bans.id,
			steamId: PLAYER,
			reason: 'org-wide ban reason',
			addedByName: 'admin'
		});
		const slots = await listOf(env, w.org.id, 'reserve');
		await env.db.insert(listEntries).values({
			id: newId(),
			listId: slots.id,
			steamId: PLAYER,
			reason: 'sponsor, paid until March',
			addedByName: 'admin'
		});
	});

	test('who holds a reserved slot is View; the note on it needs Reserved slots or Org lists', async () => {
		const slot = async (who: PrincipalName) =>
			(await get(who, 'api/servers/[id]/lists/state')).reserved[PLAYER];
		expect(await slot('viewer')).toMatchObject({ state: 'pending', note: '' });
		expect(await slot('operator')).toMatchObject({ note: '' });
		expect(await slot('admin')).toMatchObject({ note: 'sponsor, paid until March' });
		expect(await slot('owner')).toMatchObject({ note: 'sponsor, paid until March' });
	});

	test('where the server listens and what its owners noted are for its owners', async () => {
		const mine = async (who: PrincipalName) =>
			(await get(who, 'api/servers')).servers.find((s: { id: string }) => s.id === w.server.id);
		for (const who of ['viewer', 'operator', 'admin', 'keyAll'] as const)
			expect({ who, ...pick(await mine(who)) }).toEqual({ who, host: '', port: 0, notes: '' });
		expect(pick(await mine('owner'))).toEqual({
			host: 'game.test.invalid',
			port: 7779,
			notes: 'owner notes'
		});

		stubGateway();
		const { load } = await import(join(ROUTES, '(app)/server/[id]/+layout.server.ts'));
		const page = await callLoad(load, w.users.viewer, { params: { id: w.server.id } });
		expect(pick((page.body as { server: Record<string, unknown> }).server)).toEqual({
			host: '',
			port: 0,
			notes: ''
		});
	});

	test('staff notes and the watch reason need Notes; the org list entry needs Org lists', async () => {
		const dossier = async (who: PrincipalName) =>
			(await get(who, 'api/servers/[id]/players/[steamId]')).dossier;

		const viewer = await dossier('viewer');
		expect(viewer.notes).toEqual([]);
		expect(viewer.watch).toMatchObject({ watched: true, reason: '', updatedByName: '' });
		expect(viewer.orgLists).toEqual({ ban: null, reserve: null, canEdit: false });
		expect(JSON.stringify(viewer)).not.toContain('suspected alt');
		expect(JSON.stringify(viewer)).not.toContain('org-wide ban reason');

		const operator = await dossier('operator');
		expect(operator.notes.map((n: { body: string }) => n.body)).toEqual([
			'suspected alt of a banned player'
		]);
		expect(operator.watch.reason).toBe('watch for team kills');
		expect(operator.orgLists.ban).toBeNull();

		const admin = await dossier('admin');
		expect(admin.orgLists.ban).toMatchObject({ reason: 'org-wide ban reason' });
		expect(admin.orgLists.canEdit).toBe(true);
	});

	test("the players table's marks name bans only on servers the reader can open", async () => {
		const marks = async (who: PrincipalName) =>
			JSON.stringify(
				(await get(who, 'api/servers/[id]/players/marks', `ids=${PLAYER}&names=someone`)).marks
			);
		expect(await marks('viewer')).not.toContain('cheating on the other server');
		expect(await marks('viewer')).not.toContain('watch for team kills');
		expect(await marks('operator')).toContain('watch for team kills');
		expect(await marks('owner')).toContain('cheating on the other server');
	});

	test("raw status and the game's raw capabilities need Config & settings", async () => {
		const seen: Record<string, unknown>[] = [];
		const { setGateway } = await import('$lib/server/gateway');
		const base = stubGateway();
		void base;
		const { gateway } = await import('$lib/server/gateway');
		const stub = gateway();
		setGateway({
			...stub,
			run: async (_env, _server, action, params) => {
				seen.push({ action, ...params });
				return action === 'capabilities' ? { routes: [], features: {}, raw: { secret: 1 } } : {};
			}
		});
		const { POST } = await import(join(ROUTES, 'api/servers/[id]/rcon/[action]/+server.ts'));
		const ask = (who: PrincipalName, action: string, body: unknown) =>
			callApi(POST, w.users[who], {
				method: 'POST',
				params: { id: w.server.id, action },
				body
			});
		await ask('viewer', 'status', { raw: true });
		await ask('admin', 'status', { raw: true });
		expect(seen).toEqual([{ action: 'status' }, { action: 'status', raw: true }]);
		const viewer = await ask('viewer', 'capabilities', {});
		const admin = await ask('admin', 'capabilities', {});
		expect((viewer.body as any).result.raw).toBeUndefined();
		expect((admin.body as any).result.raw).toEqual({ secret: 1 });
	});
});

const pick = (s: Record<string, unknown>) => ({ host: s.host, port: s.port, notes: s.notes });
