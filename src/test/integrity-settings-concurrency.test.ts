import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import { integrityRules } from '$lib/server/db/schema';
import {
	getIntegrityRules,
	saveIntegrityEnforcement,
	saveIntegrityRules
} from '$lib/server/integrity/rules';
import { DEFAULT_INTEGRITY_RULES } from '$lib/server/integrity/score';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

const request = new Request('http://localhost:5173/api/orgs/test/integrity', { method: 'PUT' });

describe.skipIf(!hasTestDb)('concurrent Integrity setting saves', () => {
	test('a Worker circuit breaker committed while an admin save waits survives that save', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const actor = world.users.owner!;
		await saveIntegrityEnforcement(env, request, actor, world.org.id, {
			autoActionMaxPerHour: 10
		});
		const suspendedAt = new Date();
		let pending: Promise<unknown> | undefined;
		await env.db.transaction(async (tx) => {
			await tx
				.select()
				.from(integrityRules)
				.where(eq(integrityRules.orgId, world.org.id))
				.for('update');
			pending = saveIntegrityEnforcement(env, request, actor, world.org.id, {
				autoActionMaxPerHour: 7
			});
			// Old code read outside the lock, then waited to overwrite the Worker's suspension.
			// Wait until the save is blocked on this row before committing the circuit breaker.
			let blocked = false;
			for (let i = 0; i < 100; i++) {
				const [row] = await env.sql`
					SELECT COUNT(*)::int AS n FROM pg_stat_activity
					WHERE datname = current_database()
					  AND wait_event_type = 'Lock'
					  AND query ILIKE '%integrity_rules%'
				`;
				if (Number(row.n) > 0) {
					blocked = true;
					break;
				}
				await Bun.sleep(20);
			}
			expect(blocked).toBe(true);
			await tx
				.update(integrityRules)
				.set({ autoSuspendedAt: suspendedAt })
				.where(eq(integrityRules.orgId, world.org.id));
		});
		await pending;
		const [row] = await env.db
			.select()
			.from(integrityRules)
			.where(eq(integrityRules.orgId, world.org.id));
		expect(row.autoActionMaxPerHour).toBe(7);
		expect(row.autoSuspendedAt?.getTime()).toBe(suspendedAt.getTime());
	});

	test('ordinary settings preserve suspension; only confirmed resume clears it', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const actor = world.users.owner!;
		await saveIntegrityEnforcement(env, request, actor, world.org.id, {
			autoActionMaxPerHour: 10
		});
		await env.db
			.update(integrityRules)
			.set({ autoSuspendedAt: new Date() })
			.where(eq(integrityRules.orgId, world.org.id));
		const ordinary = await saveIntegrityEnforcement(env, request, actor, world.org.id, {
			autoActionMaxPercentOnline: 8
		});
		expect(ordinary.autoSuspendedAt).toBeInstanceOf(Date);
		await expect(
			saveIntegrityEnforcement(env, request, actor, world.org.id, { resume: true })
		).rejects.toMatchObject({ status: 400 });
		const resumed = await saveIntegrityEnforcement(env, request, actor, world.org.id, {
			resume: true,
			confirmation: 'ENABLE_EXPERIMENTAL_INTEGRITY'
		});
		expect(resumed.autoSuspendedAt).toBeNull();
	});

	test('two admins changing separate rule fields do not lose either change', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		await saveIntegrityRules(env, request, world.users.owner!, world.org.id, {
			headshotMinPct: DEFAULT_INTEGRITY_RULES.headshotMinPct
		});
		await getIntegrityRules(env, world.org.id); // Prime the old implementation's stale cache.
		await Promise.all([
			saveIntegrityRules(env, request, world.users.owner!, world.org.id, {
				headshotMinPct: 71
			}),
			saveIntegrityRules(env, request, world.users.site!, world.org.id, {
				penetrationMinPct: 51
			})
		]);
		const [row] = await env.db
			.select()
			.from(integrityRules)
			.where(eq(integrityRules.orgId, world.org.id));
		expect(row.version).toBe(3);
		expect(row.config).toMatchObject({ headshotMinPct: 71, penetrationMinPct: 51 });
	});
});
