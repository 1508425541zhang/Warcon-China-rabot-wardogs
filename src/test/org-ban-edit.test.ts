// Editing a ban on the organisation's list (the org Ban list page's Edit): which entry the PATCH
// reaches, not only who passes its gate (the API matrix sends no body, so it never gets that far).
import { beforeAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import { and, eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { listEntries } from '$lib/server/db/schema';
import { listOf, serverListOf } from '$lib/server/lists';
import { hasTestDb, testEnv } from './db';
import { callApi, stubGateway } from './call';
import { seedWorld, type PrincipalName, type World } from './world';

const ROUTES = join(import.meta.dir, '..', 'routes', 'api');
const ORG_BANNED = '76561198000000101';
const SERVER_ONLY = '76561198000000102';
const LIFTED = '76561198000000103';

describe.skipIf(!hasTestDb)('editing an org ban', () => {
	let env: Env;
	let w: World;

	const call = async (
		who: PrincipalName,
		method: 'POST' | 'PATCH' | 'DELETE',
		path: string,
		params: Record<string, string>,
		body: Record<string, unknown> = {}
	) => {
		const mod = await import(join(ROUTES, path, '+server.ts'));
		return callApi(mod[method], w.users[who], { method, params, body });
	};
	const patch = (who: PrincipalName, orgId: string, steamId: string, body: object) =>
		call(
			who,
			'PATCH',
			'orgs/[id]/lists/[kind]/entries/[steamId]',
			{
				id: orgId,
				kind: 'ban',
				steamId
			},
			body as Record<string, unknown>
		);
	const reasons = async (listId: string, steamId: string) =>
		(
			await env.db
				.select({ reason: listEntries.reason })
				.from(listEntries)
				.where(and(eq(listEntries.listId, listId), eq(listEntries.steamId, steamId)))
		).map((r) => r.reason);

	beforeAll(async () => {
		env = await testEnv();
		stubGateway();
		w = await seedWorld(env);
		const orgBan = (who: PrincipalName, id: string, steamId: string) =>
			call(
				who,
				'POST',
				'orgs/[id]/lists/[kind]/entries',
				{ id, kind: 'ban' },
				{
					steamId,
					reason: 'first'
				}
			);
		expect((await orgBan('owner', w.org.id, ORG_BANNED)).status).toBe(201);
		expect((await orgBan('outsider', w.otherOrg.id, ORG_BANNED)).status).toBe(201);
		expect((await orgBan('owner', w.org.id, LIFTED)).status).toBe(201);
		const lifted = await call('owner', 'DELETE', 'orgs/[id]/lists/[kind]/entries/[steamId]', {
			id: w.org.id,
			kind: 'ban',
			steamId: LIFTED
		});
		expect(lifted.status).toBe(200);
		const here = await call(
			'owner',
			'POST',
			'servers/[id]/lists/ban/entries',
			{ id: w.server.id },
			{ steamId: SERVER_ONLY, reason: 'first' }
		);
		expect(here.status).toBe(201);
	});

	test('reaches the entry on its own org list and nothing else', async () => {
		const ours = await listOf(env, w.org.id, 'ban');
		const theirs = await listOf(env, w.otherOrg.id, 'ban');
		const serverList = await serverListOf(env, { id: w.server.id, orgId: w.org.id }, 'ban');

		for (const who of ['orgBans', 'keyBans'] as const) {
			// another org's entry for the same player, through its own org id
			expect((await patch(who, w.otherOrg.id, ORG_BANNED, { reason: who })).status).toBe(404);
			// an entry on a server's own list, and one already lifted, are not on the org list
			expect((await patch(who, w.org.id, SERVER_ONLY, { reason: who })).status).toBe(404);
			expect((await patch(who, w.org.id, LIFTED, { reason: who })).status).toBe(404);
		}
		// the other org's owner cannot reach ours
		expect((await patch('outsider', w.org.id, ORG_BANNED, { reason: 'outsider' })).status).toBe(
			404
		);
		expect(await reasons(theirs.id, ORG_BANNED)).toEqual(['first']);
		expect(await reasons(serverList.id, SERVER_ONLY)).toEqual(['first']);
		expect(await reasons(ours.id, LIFTED)).toEqual(['first']);
		expect(await reasons(ours.id, ORG_BANNED)).toEqual(['first']);

		expect((await patch('orgBans', w.org.id, ORG_BANNED, { reason: 'edited' })).status).toBe(200);
		expect(await reasons(ours.id, ORG_BANNED)).toEqual(['edited']);
		expect(await reasons(theirs.id, ORG_BANNED)).toEqual(['first']);
	});

	test('a reason alone can be changed on a ban already past its expiry', async () => {
		const ours = await listOf(env, w.org.id, 'ban');
		const past = new Date(Date.now() - 60_000);
		await env.db
			.update(listEntries)
			.set({ expiresAt: past })
			.where(and(eq(listEntries.listId, ours.id), eq(listEntries.steamId, ORG_BANNED)));
		const answer = await patch('orgBans', w.org.id, ORG_BANNED, { reason: 'typo fixed' });
		expect(answer.status).toBe(200);
		const [row] = await env.db
			.select({ reason: listEntries.reason, expiresAt: listEntries.expiresAt })
			.from(listEntries)
			.where(and(eq(listEntries.listId, ours.id), eq(listEntries.steamId, ORG_BANNED)));
		expect(row).toEqual({ reason: 'typo fixed', expiresAt: past });
	});
});
