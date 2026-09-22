// The cast the permission tests run against: two organisations that must never see each other, a
// suspended one, and one person per kind of access. Rows go in through Drizzle rather than the
// panel's own writers, so a bug in a writer cannot hide a bug in a check.
import { randomBytes } from 'node:crypto';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { keyUser, type SessionUser } from '$lib/server/access';
import { hashToken, mintToken, principalOf, tokenHint } from '$lib/server/apikeys-core';
import { encryptSecret } from '$lib/server/crypto';
import { ensureOrgRoles } from '$lib/server/roles';
import {
	apiKeys,
	orgMembers,
	orgRoles,
	organizations,
	serverGrants,
	servers,
	user
} from '$lib/server/db/schema';
import { CAPABILITIES, type Capability } from '$lib/capabilities';

export const PRINCIPALS = [
	'anon',
	'stranger',
	'outsider',
	'member',
	'viewer',
	'operator',
	'admin',
	'elsewhere',
	'orgBans',
	'orgSlots',
	'owner',
	'site',
	'keyView',
	'keyAll',
	'keyElsewhere',
	'keyBans'
] as const;
export type PrincipalName = (typeof PRINCIPALS)[number];

export interface World {
	/** the org under test, and the server in it that every server route is asked about */
	org: { id: string; name: string };
	server: { id: string };
	/** a second server in the same org; `elsewhere` and `keyElsewhere` reach only this one */
	otherServer: { id: string };
	/** another tenant: `outsider` owns it */
	otherOrg: { id: string };
	otherOrgServer: { id: string };
	/** the three built-ins, and the two custom roles that hold one org list each (with View) */
	roles: Record<'viewer' | 'operator' | 'admin' | 'orgBans' | 'orgSlots', string>;
	users: Record<PrincipalName, SessionUser | null>;
	/** bearer tokens of the keys, for tests that go through resolveBearer */
	tokens: Record<'keyView' | 'keyAll' | 'keyElsewhere' | 'keyBans', string>;
}

const tag = () => randomBytes(4).toString('hex');

async function addUser(env: Env, name: string, role: 'owner' | 'member'): Promise<SessionUser> {
	const id = `u_${name}`;
	await env.db.insert(user).values({
		id,
		name,
		email: `${name}@test.invalid`,
		username: name,
		displayUsername: name,
		role,
		authComplete: true
	});
	return {
		id,
		username: name,
		name,
		role,
		mustChangePassword: false,
		image: null,
		defaultOrgId: null,
		authComplete: true,
		authGraceStartedAt: null
	};
}

async function addOrg(env: Env, name: string, owner: SessionUser) {
	const id = `o_${name}`;
	await env.db.insert(organizations).values({ id, name, slug: name, createdBy: owner.id });
	await ensureOrgRoles(env.db, id);
	await env.db.insert(orgMembers).values({ orgId: id, userId: owner.id, role: 'owner' });
	return { id, name };
}

async function addServer(env: Env, orgId: string, name: string) {
	const id = `s_${name}`;
	await env.db.insert(servers).values({
		id,
		orgId,
		name,
		host: 'game.test.invalid',
		port: 7779,
		passwordEnc: encryptSecret(env, 'rcon-password'),
		notes: 'owner notes'
	});
	return { id };
}

/** A custom role of the org's, as an owner would make it in the role editor. */
async function addRole(env: Env, orgId: string, name: string, caps: Capability[]) {
	const id = `r_${name.toLowerCase().replace(/[^a-z0-9]+/g, '_')}`;
	await env.db.insert(orgRoles).values({ id, orgId, name, capabilities: caps, sortOrder: 3 });
	return id;
}

async function addKey(env: Env, orgId: string, label: string, caps: Capability[], only?: string) {
	const token = mintToken();
	const [row] = await env.db
		.insert(apiKeys)
		.values({
			id: `k_${label}`,
			orgId,
			label,
			keyHash: hashToken(token),
			hint: tokenHint(token),
			capabilities: caps,
			serverIds: only ? [only] : null
		})
		.returning();
	return { token, user: keyUser(principalOf(row)) };
}

/** A fresh, self-contained world; ids carry a random tag so many can live in one database. */
export async function seedWorld(env: Env): Promise<World> {
	const t = tag();
	const n = (s: string) => `${s}_${t}`;
	const [site, owner, outsider, stranger, member, viewer, operator, admin, elsewhere] = [
		await addUser(env, n('site'), 'owner'),
		await addUser(env, n('owner'), 'member'),
		await addUser(env, n('outsider'), 'member'),
		await addUser(env, n('stranger'), 'member'),
		await addUser(env, n('member'), 'member'),
		await addUser(env, n('viewer'), 'member'),
		await addUser(env, n('operator'), 'member'),
		await addUser(env, n('admin'), 'member'),
		await addUser(env, n('elsewhere'), 'member')
	];
	const [orgBans, orgSlots] = [
		await addUser(env, n('orgbans'), 'member'),
		await addUser(env, n('orgslots'), 'member')
	];
	const org = await addOrg(env, n('org'), owner);
	const otherOrg = await addOrg(env, n('other'), outsider);
	const server = await addServer(env, org.id, n('one'));
	const otherServer = await addServer(env, org.id, n('two'));
	const otherOrgServer = await addServer(env, otherOrg.id, n('theirs'));

	const roleRows = await env.db.select().from(orgRoles).where(eq(orgRoles.orgId, org.id));
	const roleId = (b: string) => roleRows.find((r) => r.builtin === b)!.id;
	const roles = {
		viewer: roleId('viewer'),
		operator: roleId('operator'),
		admin: roleId('admin'),
		// one org list each: what the split of 'Org lists' was for
		orgBans: await addRole(env, org.id, n('Org bans'), ['server.view', 'lists.ban']),
		orgSlots: await addRole(env, org.id, n('Org slots'), ['server.view', 'lists.reserve'])
	};

	const members = [member, viewer, operator, admin, elsewhere, orgBans, orgSlots];
	await env.db
		.insert(orgMembers)
		.values(members.map((u) => ({ orgId: org.id, userId: u.id, role: 'member' as const })));
	await env.db.insert(serverGrants).values([
		{ serverId: server.id, userId: viewer.id, roleId: roles.viewer },
		{ serverId: server.id, userId: operator.id, roleId: roles.operator },
		{ serverId: server.id, userId: admin.id, roleId: roles.admin },
		{ serverId: otherServer.id, userId: elsewhere.id, roleId: roles.admin },
		{ serverId: server.id, userId: orgBans.id, roleId: roles.orgBans },
		{ serverId: server.id, userId: orgSlots.id, roleId: roles.orgSlots }
	]);

	const keyView = await addKey(env, org.id, n('view'), ['server.view']);
	const keyAll = await addKey(env, org.id, n('all'), [...CAPABILITIES]);
	const keyElsewhere = await addKey(env, org.id, n('else'), [...CAPABILITIES], otherServer.id);
	const keyBans = await addKey(env, org.id, n('bans'), ['server.view', 'lists.ban']);

	return {
		org,
		server,
		otherServer,
		otherOrg,
		otherOrgServer,
		roles,
		users: {
			anon: null,
			stranger,
			outsider,
			member,
			viewer,
			operator,
			admin,
			elsewhere,
			orgBans,
			orgSlots,
			owner,
			site,
			keyView: keyView.user,
			keyAll: keyAll.user,
			keyElsewhere: keyElsewhere.user,
			keyBans: keyBans.user
		},
		tokens: {
			keyView: keyView.token,
			keyAll: keyAll.token,
			keyElsewhere: keyElsewhere.token,
			keyBans: keyBans.token
		}
	};
}

export async function suspend(env: Env, orgId: string): Promise<void> {
	await env.db
		.update(organizations)
		.set({ suspendedAt: new Date(), suspendedReason: 'test' })
		.where(eq(organizations.id, orgId));
}
