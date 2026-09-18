// What the matrices cannot show: access that changes (a suspension, a removal, an edited role),
// the walls between organisations, and keys and invites from the token in. Each test seeds a
// world of its own, so the order they run in does not matter.
import { beforeAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';
import { and, eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import {
	accessibleServers,
	auditVisibility,
	keyUser,
	serverAccessFor,
	userOrgs
} from '$lib/server/access';
import { resolveBearer } from '$lib/server/apikeys';
import { queryAudit } from '$lib/server/audit';
import {
	apiKeys,
	auditLog,
	orgInvites,
	orgMembers,
	orgRoles,
	serverGrants,
	user
} from '$lib/server/db/schema';
import { beforeSelfDelete } from '$lib/server/erasure';
import { findInvite, joinOrg } from '$lib/server/orgs';
import type { PrincipalName } from './world';
import { hasTestDb, testEnv } from './db';
import { callApi, callLoad, stubGateway, type CallInput, type Outcome } from './call';
import { seedWorld, suspend, type World } from './world';

const ROUTES = join(import.meta.dir, '..', 'routes');

describe.skipIf(!hasTestDb)('access', () => {
	let env: Env;

	beforeAll(async () => {
		env = await testEnv();
		stubGateway();
	});

	/** `api(w, 'owner', 'PUT api/servers/[id]/grants', { params, body })` */
	async function api(
		w: World,
		who: PrincipalName,
		route: string,
		input: Omit<CallInput, 'method'> = {}
	): Promise<Outcome> {
		const [method, path] = route.split(' ');
		const mod = await import(join(ROUTES, path, '+server.ts'));
		return callApi(mod[method], w.users[who], { ...input, method });
	}

	const run = (w: World, who: PrincipalName, action: string, serverId = w.server.id) =>
		api(w, who, 'POST api/servers/[id]/rcon/[action]', { params: { id: serverId, action } });

	describe('what each person can open', () => {
		test('the server list is exactly the servers they hold, never another org', async () => {
			const w = await seedWorld(env);
			const seen = async (who: PrincipalName) =>
				(await accessibleServers(env, w.users[who]!))
					.map((s) => s.id)
					.filter((id) => [w.server.id, w.otherServer.id, w.otherOrgServer.id].includes(id))
					.sort();
			const both = [w.server.id, w.otherServer.id].sort();
			expect(await seen('stranger')).toEqual([]);
			expect(await seen('member')).toEqual([]);
			expect(await seen('viewer')).toEqual([w.server.id]);
			expect(await seen('elsewhere')).toEqual([w.otherServer.id]);
			expect(await seen('owner')).toEqual(both);
			expect(await seen('outsider')).toEqual([w.otherOrgServer.id]);
			expect(await seen('keyView')).toEqual(both);
			expect(await seen('keyElsewhere')).toEqual([w.otherServer.id]);
			expect(await seen('site')).toEqual([...both, w.otherOrgServer.id].sort());
		});

		test('the capabilities that arrive are the granted role and nothing more', async () => {
			const w = await seedWorld(env);
			const caps = async (who: PrincipalName) => [
				...((await serverAccessFor(env, w.users[who]!, w.server.id))?.caps ?? [])
			];
			expect(await caps('viewer')).toEqual(['server.view']);
			expect(await caps('operator')).not.toContain('bans.manage');
			expect(await caps('admin')).toContain('rcon.raw');
			expect((await serverAccessFor(env, w.users.owner!, w.server.id))?.manager).toBe(true);
			expect((await serverAccessFor(env, w.users.admin!, w.server.id))?.manager).toBe(false);
		});

		// The containment is SQL over jsonb; a string bound as jsonb is encoded twice and never matches.
		test('Audit trail shows what others did, not the browser they did it from', async () => {
			const w = await seedWorld(env);
			const row = (actor: PrincipalName) => ({
				actorId: w.users[actor]!.id,
				actorName: actor,
				serverId: w.server.id,
				orgId: w.org.id,
				category: 'rcon',
				action: 'rcon.kick',
				outcome: 'ok' as const,
				userAgent: `${actor}-browser`
			});
			await env.db.insert(auditLog).values([row('owner'), row('admin')]);
			const seenBy = async (who: PrincipalName) =>
				(
					await queryAudit(env, {
						serverId: w.server.id,
						visibleTo: await auditVisibility(env, w.users[who]!)
					})
				).entries.map((e) => [e.actorName, e.userAgent]);
			expect(await seenBy('admin')).toEqual([
				['admin', 'admin-browser'],
				['owner', '']
			]);
			expect(await seenBy('owner')).toEqual([
				['admin', 'admin-browser'],
				['owner', 'owner-browser']
			]);
		});

		test('lists.edit and audit.read are found in the stored role', async () => {
			const w = await seedWorld(env);
			const lists = async (who: PrincipalName) =>
				(await userOrgs(env, w.users[who]!)).find((o) => o.id === w.org.id)?.lists;
			expect(await lists('admin')).toBe(true);
			expect(await lists('elsewhere')).toBe(true);
			expect(await lists('viewer')).toBe(false);
			expect(await lists('operator')).toBe(false);
			expect((await auditVisibility(env, w.users.admin!))?.adminServerIds).toEqual([w.server.id]);
			expect((await auditVisibility(env, w.users.viewer!))?.adminServerIds).toEqual([]);
			expect(await auditVisibility(env, w.users.site!)).toBeNull();
		});
	});

	describe('between organisations', () => {
		test("an owner cannot reach into another org's server, by any id they supply", async () => {
			const w = await seedWorld(env);
			const theirs = w.otherOrgServer.id;
			expect((await run(w, 'owner', 'status', theirs)).status).toBe(404);
			expect((await run(w, 'keyAll', 'status', theirs)).status).toBe(404);
			const patch = await api(w, 'owner', 'PATCH api/servers/[id]', {
				params: { id: theirs },
				body: { name: 'mine now' }
			});
			expect(patch.status).toBe(404);
		});

		test("a grant cannot name another org's role or another org's server", async () => {
			const w = await seedWorld(env);
			const [foreign] = await env.db
				.select()
				.from(orgRoles)
				.where(and(eq(orgRoles.orgId, w.otherOrg.id), eq(orgRoles.builtin, 'admin')));
			await api(w, 'owner', 'PUT api/orgs/[id]/members/[userId]/grants', {
				params: { id: w.org.id, userId: w.users.member!.id },
				body: {
					grants: [
						{ serverId: w.server.id, roleId: foreign.id },
						{ serverId: w.otherOrgServer.id, roleId: w.roles.admin }
					]
				}
			});
			const held = await env.db
				.select()
				.from(serverGrants)
				.where(eq(serverGrants.userId, w.users.member!.id));
			expect(held).toEqual([]);
		});

		test('granting a server role does not enrol someone who never joined the org', async () => {
			const w = await seedWorld(env);
			const put = await api(w, 'owner', 'PUT api/servers/[id]/grants', {
				params: { id: w.server.id },
				body: {
					grants: [
						{ userId: w.users.stranger!.id, roleId: w.roles.admin },
						{ userId: w.users.member!.id, roleId: w.roles.viewer }
					]
				}
			});
			expect(put.status).toBe(200);
			expect(await userOrgs(env, w.users.stranger!)).toEqual([]);
			expect((await run(w, 'stranger', 'status')).status).toBe(404);
			expect((await run(w, 'member', 'status')).status).toBe(200);
		});

		test('a key cannot be scoped to, or an invite given a role from, another org', async () => {
			const w = await seedWorld(env);
			const key = await api(w, 'owner', 'POST api/orgs/[id]/keys', {
				params: { id: w.org.id },
				body: { label: 'reach', capabilities: ['server.view'], serverIds: [w.otherOrgServer.id] }
			});
			expect(key.status).toBe(400);
			const [foreign] = await env.db
				.select()
				.from(orgRoles)
				.where(and(eq(orgRoles.orgId, w.otherOrg.id), eq(orgRoles.builtin, 'admin')));
			const invite = await api(w, 'owner', 'POST api/orgs/[id]/invites', {
				params: { id: w.org.id },
				body: { serverRoleId: foreign.id }
			});
			expect(invite.status).toBe(404);
		});

		test("another org's role, invite and key ids are not found through one's own org", async () => {
			const w = await seedWorld(env);
			const [foreign] = await env.db
				.select()
				.from(orgRoles)
				.where(and(eq(orgRoles.orgId, w.org.id), eq(orgRoles.builtin, 'viewer')));
			const edit = await api(w, 'outsider', 'PATCH api/orgs/[id]/roles/[roleId]', {
				params: { id: w.otherOrg.id, roleId: foreign.id },
				body: { capabilities: ['server.view', 'rcon.raw'] }
			});
			expect(edit.status).toBe(404);
			const [key] = await env.db.select().from(apiKeys).where(eq(apiKeys.orgId, w.org.id));
			const revoke = await api(w, 'outsider', 'DELETE api/orgs/[id]/keys/[keyId]', {
				params: { id: w.otherOrg.id, keyId: key.id }
			});
			expect(revoke.status).toBe(404);
			const [still] = await env.db.select().from(apiKeys).where(eq(apiKeys.id, key.id));
			expect(still.revokedAt).toBeNull();

			await api(w, 'owner', 'POST api/orgs/[id]/invites', { params: { id: w.org.id }, body: {} });
			const [link] = await env.db.select().from(orgInvites).where(eq(orgInvites.orgId, w.org.id));
			const unlink = await api(w, 'outsider', 'DELETE api/orgs/[id]/invites/[inviteId]', {
				params: { id: w.otherOrg.id, inviteId: link.id }
			});
			expect(unlink.status).toBe(404);
			const [live] = await env.db.select().from(orgInvites).where(eq(orgInvites.id, link.id));
			expect(live.revokedAt).toBeNull();
		});
	});

	describe('a suspended organisation', () => {
		test('closes to its owner, members and keys, and stays open to the site owner', async () => {
			const w = await seedWorld(env);
			await suspend(env, w.org.id);
			for (const who of ['owner', 'admin', 'viewer', 'keyAll'] as const)
				expect({ who, status: (await run(w, who, 'status')).status }).toEqual({ who, status: 404 });
			expect((await run(w, 'site', 'status')).status).toBe(200);
			expect(await accessibleServers(env, w.users.owner!, w.org.id)).toEqual([]);
			const members = await api(w, 'owner', 'GET api/orgs/[id]/members', {
				params: { id: w.org.id }
			});
			expect([members.status, members.code]).toEqual([403, 'suspended']);
			const lists = await api(w, 'admin', 'GET api/orgs/[id]/lists', { params: { id: w.org.id } });
			expect([lists.status, lists.code]).toEqual([403, 'suspended']);
			await expect(resolveBearer(env, w.tokens.keyAll)).rejects.toMatchObject({ status: 403 });
		});

		test('its audit trail closes too: members and owners keep only their own rows', async () => {
			const w = await seedWorld(env);
			expect((await auditVisibility(env, w.users.owner!))?.ownedOrgIds).toEqual([w.org.id]);
			await suspend(env, w.org.id);
			for (const who of ['owner', 'admin'] as const)
				expect({ who, ...(await auditVisibility(env, w.users[who]!)) }).toEqual({
					who,
					userId: w.users[who]!.id,
					adminServerIds: [],
					ownedOrgIds: []
				});
			expect(await auditVisibility(env, w.users.keyAll!)).toMatchObject({ adminServerIds: [] });
			expect(await auditVisibility(env, w.users.site!)).toBeNull();
		});

		test("its owner no longer reads the server's Discord channels from the settings page", async () => {
			const w = await seedWorld(env);
			const { load } = await import(join(ROUTES, '(app)/server/[id]/settings/+page.server.ts'));
			const open = await callLoad(load, w.users.owner, { params: { id: w.server.id } });
			expect(open.body).toMatchObject({ owner: true });
			await suspend(env, w.org.id);
			const shut = await callLoad(load, w.users.owner, { params: { id: w.server.id } });
			expect(shut.status).toBe(404);
		});
	});

	describe('access that is taken away', () => {
		test('a removed member loses every grant in the org at once', async () => {
			const w = await seedWorld(env);
			expect((await run(w, 'admin', 'status')).status).toBe(200);
			const removed = await api(w, 'owner', 'DELETE api/orgs/[id]/members/[userId]', {
				params: { id: w.org.id, userId: w.users.admin!.id }
			});
			expect(removed.status).toBe(200);
			expect((await run(w, 'admin', 'status')).status).toBe(404);
			expect(await accessibleServers(env, w.users.admin!)).toEqual([]);
			const lists = await api(w, 'admin', 'GET api/orgs/[id]/lists', { params: { id: w.org.id } });
			expect(lists.status).toBe(404);
		});

		test('an owner made a member keeps nothing they were not granted', async () => {
			const w = await seedWorld(env);
			// A second owner, so the first can be demoted.
			await env.db
				.update(orgMembers)
				.set({ role: 'owner' })
				.where(and(eq(orgMembers.orgId, w.org.id), eq(orgMembers.userId, w.users.member!.id)));
			const demoted = await api(w, 'member', 'PATCH api/orgs/[id]/members/[userId]', {
				params: { id: w.org.id, userId: w.users.owner!.id },
				body: { role: 'member' }
			});
			expect(demoted.status).toBe(200);
			expect((await run(w, 'owner', 'status')).status).toBe(404);
			const members = await api(w, 'owner', 'GET api/orgs/[id]/members', {
				params: { id: w.org.id }
			});
			expect(members.status).toBe(403);
		});

		test('the links and keys an owner minted end with their ownership', async () => {
			for (const leave of ['remove', 'demote'] as const) {
				const w = await seedWorld(env);
				const params = { id: w.org.id };
				// `member` becomes a second owner, mints an owner link and a key, and is then let go.
				await env.db
					.update(orgMembers)
					.set({ role: 'owner' })
					.where(and(eq(orgMembers.orgId, w.org.id), eq(orgMembers.userId, w.users.member!.id)));
				await api(w, 'member', 'POST api/orgs/[id]/invites', {
					params,
					body: { orgRole: 'owner' }
				});
				const minted = await api(w, 'member', 'POST api/orgs/[id]/keys', {
					params,
					body: { label: 'mine', capabilities: ['server.view', 'rcon.raw'] }
				});
				const token = (minted.body as { token: string }).token;
				const [link] = await env.db
					.select()
					.from(orgInvites)
					.where(eq(orgInvites.createdBy, w.users.member!.id));
				expect((await resolveBearer(env, token)).orgId).toBe(w.org.id);

				const path = 'api/orgs/[id]/members/[userId]';
				const gone = await api(
					w,
					'owner',
					leave === 'remove' ? `DELETE ${path}` : `PATCH ${path}`,
					{
						params: { ...params, userId: w.users.member!.id },
						body: { role: 'member' }
					}
				);
				expect(gone.status).toBe(200);

				await expect(resolveBearer(env, token)).rejects.toMatchObject({ status: 401 });
				const found = (await findInvite(env, link.token))!;
				await expect(
					joinOrg(env, new Request('http://localhost/'), w.users.stranger!, found.invite, found.org)
				).rejects.toMatchObject({ status: 410 });
				// What the remaining owner made is untouched.
				expect((await resolveBearer(env, w.tokens.keyAll)).orgId).toBe(w.org.id);
			}
		});

		test('they end when their maker is disabled, stops being a site owner, or deletes their own account', async () => {
			for (const leave of ['disable', 'demote', 'self-delete'] as const) {
				const w = await seedWorld(env);
				const params = { id: w.org.id };
				await env.db
					.update(orgMembers)
					.set({ role: 'owner' })
					.where(and(eq(orgMembers.orgId, w.org.id), eq(orgMembers.userId, w.users.member!.id)));
				await api(w, 'member', 'POST api/orgs/[id]/invites', {
					params,
					body: { orgRole: 'owner' }
				});
				const minted = await api(w, 'member', 'POST api/orgs/[id]/keys', {
					params,
					body: { label: 'mine', capabilities: ['server.view', 'rcon.raw'] }
				});
				const token = (minted.body as { token: string }).token;
				const [link] = await env.db
					.select()
					.from(orgInvites)
					.where(eq(orgInvites.createdBy, w.users.member!.id));
				expect((await resolveBearer(env, token)).orgId).toBe(w.org.id);

				if (leave === 'self-delete')
					await beforeSelfDelete(env, { id: w.users.member!.id, role: 'member' });
				else {
					if (leave === 'demote')
						await env.db.update(user).set({ role: 'owner' }).where(eq(user.id, w.users.member!.id));
					const changed = await api(w, 'site', 'PATCH api/users/[id]', {
						params: { id: w.users.member!.id },
						body: leave === 'demote' ? { role: 'member' } : { disabled: true }
					});
					expect(changed.status).toBe(200);
				}

				await expect(resolveBearer(env, token)).rejects.toMatchObject({ status: 401 });
				const found = (await findInvite(env, link.token))!;
				await expect(
					joinOrg(env, new Request('http://localhost/'), w.users.stranger!, found.invite, found.org)
				).rejects.toMatchObject({ status: 410 });
				expect((await resolveBearer(env, w.tokens.keyAll)).orgId).toBe(w.org.id);
			}
		});

		test('the last owner of an org cannot be removed or demoted', async () => {
			const w = await seedWorld(env);
			const params = { id: w.org.id, userId: w.users.owner!.id };
			const path = 'api/orgs/[id]/members/[userId]';
			expect((await api(w, 'owner', `DELETE ${path}`, { params })).status).toBe(400);
			const demote = await api(w, 'owner', `PATCH ${path}`, { params, body: { role: 'member' } });
			expect(demote.status).toBe(400);
		});

		test('a revoked or expired key stops working', async () => {
			const w = await seedWorld(env);
			expect((await resolveBearer(env, w.tokens.keyView)).orgId).toBe(w.org.id);
			await env.db
				.update(apiKeys)
				.set({ expiresAt: new Date(Date.now() - 1000) })
				.where(eq(apiKeys.id, (await resolveBearer(env, w.tokens.keyView)).id));
			await expect(resolveBearer(env, w.tokens.keyView)).rejects.toMatchObject({ status: 401 });
			const all = await resolveBearer(env, w.tokens.keyAll);
			const revoked = await api(w, 'owner', 'DELETE api/orgs/[id]/keys/[keyId]', {
				params: { id: w.org.id, keyId: all.id }
			});
			expect(revoked.status).toBe(200);
			await expect(resolveBearer(env, w.tokens.keyAll)).rejects.toMatchObject({ status: 401 });
			await expect(resolveBearer(env, 'wck_' + 'x'.repeat(43))).rejects.toMatchObject({
				status: 401
			});
		});
	});

	describe('roles', () => {
		const rolePath = 'api/orgs/[id]/roles/[roleId]';

		test('editing a role changes what its holders may do on their next request', async () => {
			const w = await seedWorld(env);
			expect((await run(w, 'viewer', 'broadcast')).status).toBe(403);
			const edit = await api(w, 'owner', `PATCH ${rolePath}`, {
				params: { id: w.org.id, roleId: w.roles.viewer },
				body: { capabilities: ['server.view', 'chat.send'] }
			});
			expect(edit.status).toBe(200);
			expect((await run(w, 'viewer', 'broadcast')).status).toBe(200);
			expect((await run(w, 'viewer', 'kick')).status).toBe(403);
		});

		test('a role always carries View, and an unknown capability is refused', async () => {
			const w = await seedWorld(env);
			const params = { id: w.org.id };
			const blind = await api(w, 'owner', 'POST api/orgs/[id]/roles', {
				params,
				body: { name: 'blind', capabilities: ['chat.send'] }
			});
			const typo = await api(w, 'owner', 'POST api/orgs/[id]/roles', {
				params,
				body: { name: 'typo', capabilities: ['server.view', 'server.everything'] }
			});
			expect(typo.status).toBe(400);
			const stored = await env.db.select().from(orgRoles).where(eq(orgRoles.orgId, w.org.id));
			for (const role of stored) expect(role.capabilities).toContain('server.view');
			expect(stored.some((r) => r.name === 'typo')).toBe(false);
			// Either answer is sound: refuse the role, or add View to it.
			expect([200, 201, 400]).toContain(blind.status);
		});

		test('a built-in role cannot be deleted, nor a role that someone still holds', async () => {
			const w = await seedWorld(env);
			const builtin = await api(w, 'owner', `DELETE ${rolePath}`, {
				params: { id: w.org.id, roleId: w.roles.operator }
			});
			expect(builtin.status).toBe(409);
			const made = await api(w, 'owner', 'POST api/orgs/[id]/roles', {
				params: { id: w.org.id },
				body: { name: 'helper', capabilities: ['server.view', 'chat.send'] }
			});
			const roleId = (made.body as { role: { id: string } }).role.id;
			await api(w, 'owner', 'PUT api/orgs/[id]/members/[userId]/grants', {
				params: { id: w.org.id, userId: w.users.member!.id },
				body: { grants: [{ serverId: w.server.id, roleId }] }
			});
			expect((await run(w, 'member', 'broadcast')).status).toBe(200);
			const held = await api(w, 'owner', `DELETE ${rolePath}`, {
				params: { id: w.org.id, roleId }
			});
			expect(held.status).toBe(409);
			expect((await run(w, 'member', 'broadcast')).status).toBe(200);
		});

		test('nobody but an owner can change a role, their own included', async () => {
			const w = await seedWorld(env);
			for (const who of ['admin', 'keyAll', 'member'] as const) {
				const edit = await api(w, who, `PATCH ${rolePath}`, {
					params: { id: w.org.id, roleId: w.roles.viewer },
					body: { capabilities: ['server.view', 'rcon.raw'] }
				});
				expect({ who, status: edit.status }).toEqual({ who, status: 403 });
			}
			const self = await api(w, 'admin', 'PUT api/orgs/[id]/members/[userId]/grants', {
				params: { id: w.org.id, userId: w.users.admin!.id },
				body: { grants: [{ serverId: w.otherServer.id, roleId: w.roles.admin }] }
			});
			expect(self.status).toBe(403);
			expect((await run(w, 'admin', 'status', w.otherServer.id)).status).toBe(404);
		});
	});

	describe('invite links', () => {
		async function invite(w: World, body: Record<string, unknown>) {
			const made = await api(w, 'owner', 'POST api/orgs/[id]/invites', {
				params: { id: w.org.id },
				body
			});
			expect(made.status).toBeLessThan(300);
			const [row] = await env.db
				.select()
				.from(orgInvites)
				.where(eq(orgInvites.orgId, w.org.id))
				.orderBy(orgInvites.createdAt);
			return (await findInvite(env, row.token))!;
		}
		const req = () => new Request('http://localhost:5173/join/x', { method: 'POST' });

		test("joining gives the link's role on the org's servers and no more", async () => {
			const w = await seedWorld(env);
			const found = await invite(w, { serverRoleId: w.roles.viewer });
			await joinOrg(env, req(), w.users.stranger!, found.invite, found.org);
			expect((await run(w, 'stranger', 'status')).status).toBe(200);
			expect((await run(w, 'stranger', 'status', w.otherServer.id)).status).toBe(200);
			expect((await run(w, 'stranger', 'broadcast')).status).toBe(403);
			const members = await api(w, 'stranger', 'GET api/orgs/[id]/members', {
				params: { id: w.org.id }
			});
			expect(members.status).toBe(403);
		});

		test('a link without a server role opens no server', async () => {
			const w = await seedWorld(env);
			const found = await invite(w, {});
			await joinOrg(env, req(), w.users.stranger!, found.invite, found.org);
			expect(await accessibleServers(env, w.users.stranger!)).toEqual([]);
		});

		test('a revoked, used-up or expired link, or a suspended org, lets nobody in', async () => {
			const w = await seedWorld(env);
			const found = await invite(w, { serverRoleId: w.roles.admin, maxUses: 1 });
			await joinOrg(env, req(), w.users.stranger!, found.invite, found.org);
			const again = (await findInvite(env, found.invite.token))!;
			await expect(
				joinOrg(env, req(), w.users.outsider!, again.invite, again.org)
			).rejects.toMatchObject({ status: 410 });

			const w2 = await seedWorld(env);
			const second = await invite(w2, { serverRoleId: w2.roles.admin });
			await suspend(env, w2.org.id);
			const held = (await findInvite(env, second.invite.token))!;
			await expect(
				joinOrg(env, req(), w2.users.stranger!, held.invite, held.org)
			).rejects.toMatchObject({ status: 410 });
			expect(await accessibleServers(env, w2.users.stranger!)).toEqual([]);
		});
	});

	describe('keys', () => {
		test('a key is what its capabilities say, whoever minted it', async () => {
			const w = await seedWorld(env);
			const principal = await resolveBearer(env, w.tokens.keyView);
			const key = keyUser(principal);
			expect(key.role).toBe('member');
			const viaKey = { ...w, users: { ...w.users, keyView: key } };
			expect((await run(viaKey, 'keyView', 'status')).status).toBe(200);
			expect((await run(viaKey, 'keyView', 'broadcast')).status).toBe(403);
		});

		test('a key limited to some servers cannot be given the org lists', async () => {
			const w = await seedWorld(env);
			const body = { label: 'one server', capabilities: ['server.view', 'lists.edit'] };
			const limited = await api(w, 'owner', 'POST api/orgs/[id]/keys', {
				params: { id: w.org.id },
				body: { ...body, serverIds: [w.server.id] }
			});
			expect(limited.status).toBe(400);
			const whole = await api(w, 'owner', 'POST api/orgs/[id]/keys', {
				params: { id: w.org.id },
				body
			});
			expect(whole.status).toBe(201);
		});

		test('an empty or malformed server selection is refused, not read as every server', async () => {
			const w = await seedWorld(env);
			const params = { id: w.org.id };
			for (const serverIds of [[], [''], 'all', [w.server.id, 'no-such-server'], [42]]) {
				const key = await api(w, 'owner', 'POST api/orgs/[id]/keys', {
					params,
					body: { label: 'scoped', capabilities: ['server.view'], serverIds }
				});
				expect({ serverIds, code: key.code }).toEqual({ serverIds, code: 'bad_scope' });
				const hook = await api(w, 'owner', 'POST api/orgs/[id]/webhooks', {
					params,
					body: {
						url: 'https://discord.com/api/webhooks/123456789012345678/' + 'a'.repeat(60),
						events: ['bans'],
						serverIds
					}
				});
				expect({ serverIds, code: hook.code }).toEqual({ serverIds, code: 'bad_scope' });
			}
			expect(await env.db.select().from(apiKeys).where(eq(apiKeys.label, 'scoped'))).toEqual([]);
		});

		test('a key holds no seat in the org: no members, roles, keys, invites or servers', async () => {
			const w = await seedWorld(env);
			const params = { id: w.org.id };
			for (const route of [
				'GET api/orgs/[id]/members',
				'POST api/orgs/[id]/keys',
				'POST api/orgs/[id]/invites',
				'POST api/orgs/[id]/roles',
				'DELETE api/orgs/[id]'
			]) {
				const answer = await api(w, 'keyAll', route, { params });
				expect({ route, code: answer.code }).toEqual({ route, code: 'api_key_forbidden' });
			}
		});
	});
});
