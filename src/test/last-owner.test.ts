// An organisation and the panel always keep an owner, also when the requests that would take the
// last two away arrive together.
import { beforeAll, describe, expect, test } from 'bun:test';
import { and, eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import type { Auth } from '$lib/server/auth';
import { orgMembers, user } from '$lib/server/db/schema';
import { removeMember, setMemberRole } from '$lib/server/orgs';
import { updateUser } from '$lib/server/users';
import type { OrgRow } from '$lib/server/db/schema';
import { organizations } from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

describe.skipIf(!hasTestDb)('the last owner', () => {
	let env: Env;
	const req = new Request('http://localhost/api');

	beforeAll(async () => {
		env = await testEnv();
	});

	/** The world's org with a second owner, `admin`. */
	async function twoOwners(): Promise<{ w: World; org: OrgRow; ids: [string, string] }> {
		const w = await seedWorld(env);
		const ids: [string, string] = [w.users.owner!.id, w.users.admin!.id];
		await env.db
			.update(orgMembers)
			.set({ role: 'owner' })
			.where(and(eq(orgMembers.orgId, w.org.id), eq(orgMembers.userId, ids[1])));
		const [org] = await env.db.select().from(organizations).where(eq(organizations.id, w.org.id));
		return { w, org, ids };
	}

	const ownersOf = async (orgId: string) =>
		(
			await env.db
				.select({ id: orgMembers.userId })
				.from(orgMembers)
				.where(and(eq(orgMembers.orgId, orgId), eq(orgMembers.role, 'owner')))
		).length;

	test('two owners demoted at once: one request is refused', async () => {
		for (let round = 0; round < 5; round++) {
			const { w, org, ids } = await twoOwners();
			const done = await Promise.allSettled(
				ids.map((id) => setMemberRole(env, req, w.users.site!, org, id, 'member'))
			);
			expect(done.filter((d) => d.status === 'rejected')).toHaveLength(1);
			expect(await ownersOf(org.id)).toBe(1);
		}
	});

	test('two owners removed at once: one request is refused', async () => {
		for (let round = 0; round < 5; round++) {
			const { w, org, ids } = await twoOwners();
			const done = await Promise.allSettled(
				ids.map((id) => removeMember(env, req, w.users.site!, org, id))
			);
			expect(done.filter((d) => d.status === 'rejected')).toHaveLength(1);
			expect(await ownersOf(org.id)).toBe(1);
		}
	});

	test('the only owner cannot be demoted or removed', async () => {
		const w = await seedWorld(env);
		const [org] = await env.db.select().from(organizations).where(eq(organizations.id, w.org.id));
		const id = w.users.owner!.id;
		await expect(setMemberRole(env, req, w.users.site!, org, id, 'member')).rejects.toThrow(
			/at least one owner/
		);
		await expect(removeMember(env, req, w.users.site!, org, id)).rejects.toThrow(
			/at least one owner/
		);
		expect(await ownersOf(org.id)).toBe(1);
	});

	test('site owners demoted at once: the panel keeps one', async () => {
		// Every world adds a site owner, so first leave exactly two standing.
		const w = await seedWorld(env);
		const other = (await seedWorld(env)).users.site!;
		const pair = [w.users.site!, other];
		const kept = pair.map((u) => u.id);
		const before = await env.db.select({ id: user.id, role: user.role }).from(user);
		const others = before.filter((u) => u.role === 'owner' && !kept.includes(u.id));
		for (const u of others)
			await env.db.update(user).set({ role: 'member' }).where(eq(user.id, u.id));
		try {
			const done = await Promise.allSettled([
				updateUser({} as Auth, env, req, pair[0], pair[1].id, { role: 'member' }),
				updateUser({} as Auth, env, req, pair[1], pair[0].id, { role: 'member' })
			]);
			expect(done.filter((d) => d.status === 'rejected')).toHaveLength(1);
			const left = await env.db.select({ id: user.id }).from(user).where(eq(user.role, 'owner'));
			expect(left).toHaveLength(1);
		} finally {
			for (const u of [...others, ...pair])
				await env.db.update(user).set({ role: 'owner' }).where(eq(user.id, u.id));
		}
	});
});
