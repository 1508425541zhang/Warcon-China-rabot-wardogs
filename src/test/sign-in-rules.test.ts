// The sign-in rules against a real database.
import { beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { user } from '$lib/server/db/schema';
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
});
