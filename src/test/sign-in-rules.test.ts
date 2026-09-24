// The sign-in rules and the login lockout against a real database: what a second sign-in, a
// promotion and a burst of wrong passwords leave behind.
import { beforeAll, describe, expect, test } from 'bun:test';
import { eq, sql } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { loginLockSeconds, noteLoginFailure } from '$lib/server/access';
import type { Auth } from '$lib/server/auth';
import { loginAttempts, passkey, user } from '$lib/server/db/schema';
import { refreshAuthComplete, startGrace } from '$lib/server/enrolment';
import { refuseMemberBeforeOwner, updateUser } from '$lib/server/users';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('sign-in rules', () => {
	let env: Env;

	beforeAll(async () => {
		env = await testEnv();
	});

	const graceOf = async (id: string) =>
		(
			await env.db
				.select({ at: user.authGraceStartedAt })
				.from(user)
				.where(eq(user.id, id))
				.limit(1)
		)[0].at;

	test('the grace period counts from the first sign-in, and a later one does not restart it', async () => {
		const w = await seedWorld(env);
		const id = w.users.member!.id;
		await startGrace(env, id);
		expect(await graceOf(id)).toBeInstanceOf(Date);

		const longAgo = new Date(Date.now() - 30 * 86_400_000);
		await env.db.update(user).set({ authGraceStartedAt: longAgo }).where(eq(user.id, id));
		await startGrace(env, id);
		expect((await graceOf(id))!.getTime()).toBe(longAgo.getTime());
	});

	test('a promotion to site owner is judged by the owner rules at once', async () => {
		const w = await seedWorld(env);
		const id = w.users.member!.id;
		await env.db.insert(passkey).values(
			['a', 'b'].map((n) => ({
				id: `${id}_${n}`,
				publicKey: 'key',
				userId: id,
				credentialID: `${id}_${n}`,
				counter: 0,
				deviceType: 'singleDevice',
				backedUp: false
			}))
		);
		// Two passkeys are enough for a member; an owner also needs a provider or a recovery key.
		expect((await refreshAuthComplete(env, id)).enrolment.complete).toBe(true);
		const complete = async () =>
			(await env.db.select({ c: user.authComplete }).from(user).where(eq(user.id, id)))[0].c;
		const req = new Request('http://localhost/api/users');
		await updateUser({} as Auth, env, req, w.users.site!, id, { role: 'owner' });
		expect(await complete()).toBe(false);
		await updateUser({} as Auth, env, req, w.users.site!, id, { role: 'member' });
		expect(await complete()).toBe(true);
	});

	test('before the site owner exists no other account can be made', async () => {
		await seedWorld(env);
		await refuseMemberBeforeOwner(env, 'member');
		// An empty panel, for the length of a transaction that is rolled back.
		const empty = new Error('roll back');
		await env.db
			.transaction(async (tx) => {
				await tx.execute(sql`TRUNCATE "user" CASCADE`);
				await refuseMemberBeforeOwner({ db: tx }, 'owner');
				for (const role of ['member', undefined])
					await expect(refuseMemberBeforeOwner({ db: tx }, role)).rejects.toMatchObject({
						status: 409,
						code: 'not_set_up'
					});
				throw empty;
			})
			.catch((err) => {
				if (err !== empty) throw err;
			});
	});

	test('wrong passwords sent at once are each counted, and lock the name at the limit', async () => {
		const key = `u:burst_${Date.now()}`;
		await Promise.all(Array.from({ length: 12 }, () => noteLoginFailure(env, [key])));
		const [row] = await env.db.select().from(loginAttempts).where(eq(loginAttempts.key, key));
		expect(row.count).toBe(12);
		expect(await loginLockSeconds(env, [key])).toBeGreaterThan(0);
	});

	test('failures older than the window start the count again', async () => {
		const key = `u:stale_${Date.now()}`;
		await env.db
			.insert(loginAttempts)
			.values({ key, count: 7, firstAt: new Date(Date.now() - 31 * 60_000), lockedUntil: null });
		await noteLoginFailure(env, [key]);
		const [row] = await env.db.select().from(loginAttempts).where(eq(loginAttempts.key, key));
		expect(row.count).toBe(1);
		expect(row.lockedUntil).toBeNull();
	});
});
