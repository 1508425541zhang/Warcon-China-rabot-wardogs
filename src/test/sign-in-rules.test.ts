// The sign-in rules and the login lockout against a real database: what a second sign-in and a
// burst of wrong passwords leave behind.
import { beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { loginLockSeconds, noteLoginFailure } from '$lib/server/access';
import { loginAttempts, user } from '$lib/server/db/schema';
import { startGrace } from '$lib/server/enrolment';
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
