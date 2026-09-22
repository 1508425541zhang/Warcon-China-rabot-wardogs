// Each org list has a capability of its own: 'Org ban list' (lists.ban) and 'Org reserved slots'
// (lists.reserve), split from 'Org lists' (lists.edit) so an owner can hand out org-wide bans
// without org-wide slots. What one list's editor can change, and what migration 0033 made of the
// roles and keys that held the old capability.
import { beforeAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import { and, eq, isNull, sql } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { keyUser, listsRoleFor, userOrgs } from '$lib/server/access';
import { resolveBearer } from '$lib/server/apikeys';
import { apiKeys, listEntries, orgRoles, serverGrants } from '$lib/server/db/schema';
import { listOf } from '$lib/server/lists';
import { BUILTIN_CAPABILITIES } from '$lib/capabilities';
import { principalOf } from '$lib/server/apikeys-core';
import { hasTestDb, testEnv } from './db';
import { callApi, stubGateway } from './call';
import { seedWorld, type PrincipalName, type World } from './world';

const ROUTES = join(import.meta.dir, '..', 'routes', 'api');
const PLAYER = '76561198000000091';

async function runMigration(env: Env) {
	const text = await Bun.file('drizzle/0033_org_list_capabilities.sql').text();
	for (const stmt of text.split('--> statement-breakpoint')) await env.db.execute(sql.raw(stmt));
}

describe.skipIf(!hasTestDb)('one org list without the other', () => {
	let env: Env;
	let w: World;

	const entries = async (
		who: PrincipalName,
		method: 'POST' | 'DELETE',
		kind: 'ban' | 'reserve'
	) => {
		const path =
			method === 'POST'
				? 'orgs/[id]/lists/[kind]/entries'
				: 'orgs/[id]/lists/[kind]/entries/[steamId]';
		const mod = await import(join(ROUTES, path, '+server.ts'));
		return callApi(mod[method], w.users[who], {
			method,
			params: { id: w.org.id, kind, steamId: PLAYER },
			body: { steamId: PLAYER, reason: `${who} ${kind}` }
		});
	};
	const active = async (kind: 'ban' | 'reserve') => {
		const list = await listOf(env, w.org.id, kind);
		return env.db
			.select({ reason: listEntries.reason })
			.from(listEntries)
			.where(
				and(
					eq(listEntries.listId, list.id),
					eq(listEntries.steamId, PLAYER),
					isNull(listEntries.removedAt)
				)
			);
	};

	beforeAll(async () => {
		env = await testEnv();
		stubGateway();
		w = await seedWorld(env);
	});

	test('the ban list alone bans and unbans across the org, and reserves nothing', async () => {
		for (const who of ['orgBans', 'keyBans'] as const) {
			expect((await entries(who, 'POST', 'ban')).status).toBe(201);
			expect(await active('ban')).toEqual([{ reason: `${who} ban` }]);
			const refused = await entries(who, 'POST', 'reserve');
			expect({ who, status: refused.status, code: refused.code }).toEqual({
				who,
				status: 403,
				code: 'forbidden'
			});
			expect(await active('reserve')).toEqual([]);
			expect((await entries(who, 'DELETE', 'ban')).status).toBe(200);
			expect(await active('ban')).toEqual([]);
		}
	});

	test('the reserved-slot list alone reserves and withdraws across the org, and bans nobody', async () => {
		expect((await entries('orgSlots', 'POST', 'reserve')).status).toBe(201);
		expect(await active('reserve')).toEqual([{ reason: 'orgSlots reserve' }]);
		expect((await entries('orgSlots', 'POST', 'ban')).status).toBe(403);
		expect(await active('ban')).toEqual([]);
		// nor can it lift a ban someone else placed
		expect((await entries('owner', 'POST', 'ban')).status).toBe(201);
		expect((await entries('orgSlots', 'DELETE', 'ban')).status).toBe(403);
		expect(await active('ban')).toEqual([{ reason: 'owner ban' }]);
		expect((await entries('orgSlots', 'DELETE', 'reserve')).status).toBe(200);
		expect(await active('reserve')).toEqual([]);
		expect((await entries('owner', 'DELETE', 'ban')).status).toBe(200);
	});

	test("a key over the whole org carrying one list's capability opens that list alone", async () => {
		const mod = await import(join(ROUTES, 'orgs/[id]/lists', '+server.ts'));
		const answer = await callApi(mod.GET, w.users.keyBans, { params: { id: w.org.id } });
		expect(answer.status).toBe(200);
		const view = answer.body as { kinds: string[]; lists: { kind: string }[] };
		expect(view.kinds).toEqual(['ban']);
		expect(view.lists.map((l) => l.kind)).toEqual(['ban']);
	});

	test('one list held on one server and the other on another add up to both', async () => {
		const u = await seedWorld(env);
		const person = u.users.orgBans!;
		await env.db
			.insert(serverGrants)
			.values({ serverId: u.otherServer.id, userId: person.id, roleId: u.roles.orgSlots });
		expect(await listsRoleFor(env, person, u.org.id)).toEqual({
			owner: false,
			kinds: ['ban', 'reserve']
		});
		expect((await userOrgs(env, person)).find((o) => o.id === u.org.id)?.listKinds).toEqual([
			'ban',
			'reserve'
		]);
	});

	test('a key over the whole org with the reserved-slot list alone reserves and bans nobody', async () => {
		const keys = await import(join(ROUTES, 'orgs/[id]/keys', '+server.ts'));
		const minted = await callApi(keys.POST, w.users.owner, {
			method: 'POST',
			params: { id: w.org.id },
			body: { label: 'slots bot', capabilities: ['server.view', 'lists.reserve'] }
		});
		expect(minted.status).toBe(201);
		const bot = keyUser(await resolveBearer(env, (minted.body as { token: string }).token));
		const post = async (kind: 'ban' | 'reserve') => {
			const mod = await import(join(ROUTES, 'orgs/[id]/lists/[kind]/entries', '+server.ts'));
			return callApi(mod.POST, bot, {
				method: 'POST',
				params: { id: w.org.id, kind },
				body: { steamId: PLAYER, reason: `bot ${kind}` }
			});
		};
		expect((await post('ban')).status).toBe(403);
		expect(await active('ban')).toEqual([]);
		expect((await post('reserve')).status).toBe(201);
		expect(await active('reserve')).toEqual([{ reason: 'bot reserve' }]);
		expect((await entries('owner', 'DELETE', 'reserve')).status).toBe(200);
	});

	test('migration 0033 gives both lists to every role and org-wide key that held Org lists', async () => {
		const m = await seedWorld(env);
		// Rows as they stood before the split: the built-in admin as migration 0012 seeded it, a
		// custom role, org-wide keys (no server list, or a JSON null one), a key limited to one
		// server, and one whose server list is a JSON string (written before migration 0008's
		// repair), which the panel reads as a list of servers.
		const adminBefore = [
			'server.view',
			'chat.send',
			'players.moderate',
			'match.control',
			'rotation.edit',
			'players.notes',
			'rotation.save',
			'players.notes.manage',
			'bans.manage',
			'slots.manage',
			'lists.edit',
			'config.apply',
			'automation.manage',
			'audit.read',
			'rcon.raw'
		];
		await env.db
			.update(orgRoles)
			.set({ capabilities: adminBefore })
			.where(eq(orgRoles.id, m.roles.admin));
		await env.db
			.update(orgRoles)
			.set({ capabilities: ['server.view', 'lists.edit'] })
			.where(eq(orgRoles.id, m.roles.orgBans));
		await env.db
			.update(apiKeys)
			.set({ capabilities: ['server.view', 'lists.edit'] })
			.where(eq(apiKeys.id, m.users.keyBans!.apiKey!.id));
		await env.db
			.update(apiKeys)
			.set({ capabilities: ['server.view', 'lists.edit', 'chat.send'] })
			.where(eq(apiKeys.id, m.users.keyElsewhere!.apiKey!.id));
		await env.db
			.update(apiKeys)
			.set({ capabilities: ['server.view', 'lists.edit'], serverIds: sql`'null'::jsonb` })
			.where(eq(apiKeys.id, m.users.keyView!.apiKey!.id));
		await env.db
			.update(apiKeys)
			.set({
				capabilities: ['server.view', 'lists.edit'],
				serverIds: sql`to_jsonb(${JSON.stringify([m.server.id])}::text)`
			})
			.where(eq(apiKeys.id, m.users.keyAll!.apiKey!.id));

		await runMigration(env);

		const role = async (id: string) =>
			(await env.db.select().from(orgRoles).where(eq(orgRoles.id, id)))[0].capabilities as string[];
		const key = async (id: string) =>
			(await env.db.select().from(apiKeys).where(eq(apiKeys.id, id)))[0].capabilities as string[];
		// the built-in admin comes out exactly as a new org's admin
		expect((await role(m.roles.admin)).sort()).toEqual([...BUILTIN_CAPABILITIES.admin].sort());
		expect((await role(m.roles.orgBans)).sort()).toEqual([
			'lists.ban',
			'lists.reserve',
			'server.view'
		]);
		// a role that never held it is left alone
		expect(await role(m.roles.orgSlots)).toEqual(['server.view', 'lists.reserve']);
		expect((await key(m.users.keyBans!.apiKey!.id)).sort()).toEqual([
			'lists.ban',
			'lists.reserve',
			'server.view'
		]);
		expect((await key(m.users.keyView!.apiKey!.id)).sort()).toEqual([
			'lists.ban',
			'lists.reserve',
			'server.view'
		]);
		// a key held to some servers was never let into the org lists, so it gets neither
		expect((await key(m.users.keyElsewhere!.apiKey!.id)).sort()).toEqual([
			'chat.send',
			'server.view'
		]);
		// nor does one whose scope is not plainly the whole org: the panel reads this one as held
		// to one server, and the migration must not read it as the whole org
		const [stringScoped] = await env.db
			.select()
			.from(apiKeys)
			.where(eq(apiKeys.id, m.users.keyAll!.apiKey!.id));
		expect(principalOf(stringScoped).serverIds).toEqual([m.server.id]);
		expect(await key(m.users.keyAll!.apiKey!.id)).toEqual(['server.view']);
		// and nothing that says lists.edit is left anywhere
		const [left] = await env.db.execute<{ n: number }>(sql`
			SELECT (SELECT count(*) FROM org_roles WHERE capabilities @> '["lists.edit"]'::jsonb)
			     + (SELECT count(*) FROM api_keys WHERE capabilities @> '["lists.edit"]'::jsonb) AS n`);
		expect(Number(left.n)).toBe(0);
	});
});
