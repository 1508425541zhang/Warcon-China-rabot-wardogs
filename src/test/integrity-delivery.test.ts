import { afterAll, beforeAll, describe, expect, spyOn, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { integrityActions, integrityCases, integrityRules, listEntries, outbox, organizations, servers } from '$lib/server/db/schema';
import { integrityDeliverySkipReason } from '$lib/server/integrity/delivery';
import { deliverOne } from '$lib/server/outbox';
import { withServer, PRIORITY } from '$lib/server/dispatcher';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import { forgetMemory, memoryFor } from '$lib/server/observe';
import { WardogsClient } from '$lib/server/rcon';
import { grantEntry, serverListOf } from '$lib/server/lists';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

describe.skipIf(!hasTestDb)('Integrity delivery guard', () => {
	let env: Env;
	let world: World;
	let clientSpy: ReturnType<typeof spyOn>;
	const steamId = (n: number) => `7656119800000${String(n).padStart(4, '0')}`;
	const created = async (n: number, action: 'KICK' | 'QUARANTINE_24H' | 'QUARANTINE_7D' = 'KICK') => {
		const steam = steamId(n);
		const caseId = crypto.randomUUID();
		const actionId = crypto.randomUUID();
		await env.db.insert(integrityCases).values({
			id: caseId, orgId: world.org.id, serverId: world.server.id, steamId: steam,
			createdAt: new Date(),
			confidence: 'B', trigger: 'ABNORMAL_INFANTRY_WINDOW', ruleVersion: 1,
			riskScore: 85, riskBreakdown: [], snapshot: {}
		});
		await env.db.insert(integrityActions).values({
			id: actionId, caseId, orgId: world.org.id, serverId: world.server.id,
			steamId: steam, action, source: 'RULE', deliveryState: 'pending',
			effectiveAt: action === 'KICK' ? null : new Date()
		});
		const [row] = await env.db.insert(outbox).values({
			serverId: world.server.id, triggerName: 'Community Integrity', triggerKind: 'integrity',
			action: 'kick', params: { steamId: steam, reason: 'test' }, target: steam, steamId: steam,
			detail: { actionId, caseId }, okMessage: 'Kicked', dedupeKey: `integrity-test:${actionId}`,
			state: 'sending', leaseUntil: new Date(Date.now() + 60_000)
		}).returning();
		return { row, actionId, caseId };
	};
	const rule = async (patch: Record<string, unknown>) => {
		await env.db.update(integrityRules).set({
			autoKickEnabled: true, autoQuarantine24hEnabled: true,
			autoQuarantine7dEnabled: true, autoSuspendedAt: null, ...patch
		}).where(eq(integrityRules.orgId, world.org.id));
	};
	const actionOf = async (id: string) =>
		(await env.db.select().from(integrityActions).where(eq(integrityActions.id, id)))[0];
	const outboxOf = async (id: number) =>
		(await env.db.select().from(outbox).where(eq(outbox.id, id)))[0];

	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
		const [server] = await env.db.select().from(servers).where(eq(servers.id, world.server.id));
		const [org] = await env.db.select().from(organizations).where(eq(organizations.id, world.org.id));
		const memory = memoryFor(server, org);
		memory.playersAt = Date.now();
		memory.players = Array.from({ length: 30 }, (_, i) => ({
			steamId: steamId(820 + i), name: 'test', faction: 'Blue', kills: 0,
			deaths: 0, cash: 0, ping: 20
		}));
		await env.db.insert(integrityRules).values({ orgId: world.org.id, config: {} });
		await rule({});
		expect(await acquireOrRenew(env, 'integrity delivery test')).toBe(true);
		clientSpy = spyOn(WardogsClient, 'forServer').mockImplementation(async () => {
			throw new Error('RCON must not be called');
		});
	});
	afterAll(async () => {
		clientSpy.mockRestore();
		forgetMemory(world.server.id);
		await releaseOwnership(env);
	});

	test('pending kick is skipped after its switch closes', async () => {
		const { row, actionId } = await created(820);
		await rule({ autoKickEnabled: false });
		await deliverOne(env, row);
		expect((await outboxOf(row.id)).state).toBe('skipped');
		expect((await outboxOf(row.id)).outcome).toBe('Integrity enforcement disabled before delivery');
		expect((await actionOf(actionId)).effectiveAt).toBeNull();
		expect(clientSpy).not.toHaveBeenCalled();
	});

	test('a claimed kick waiting for the server lane rechecks the switch', async () => {
		await rule({});
		const { row, actionId } = await created(821);
		let release!: () => void;
		let entered!: () => void;
		const enteredLane = new Promise<void>((resolve) => { entered = resolve; });
		const held = new Promise<void>((resolve) => { release = resolve; });
		const blocking = withServer(world.server.id, PRIORITY.command, async () => {
			entered();
			await held;
		});
		await enteredLane;
		const delivery = deliverOne(env, row);
		await Bun.sleep(30);
		await rule({ autoKickEnabled: false });
		release();
		await blocking;
		await delivery;
		expect((await outboxOf(row.id)).state).toBe('skipped');
		expect((await actionOf(actionId)).effectiveAt).toBeNull();
		expect(clientSpy).not.toHaveBeenCalled();
	});

	test('a circuit breaker stops a claimed kick', async () => {
		await rule({});
		const { row, actionId } = await created(822);
		await rule({ autoSuspendedAt: new Date() });
		await deliverOne(env, row);
		expect((await outboxOf(row.id)).state).toBe('skipped');
		expect((await actionOf(actionId)).effectiveAt).toBeNull();
		expect(clientSpy).not.toHaveBeenCalled();
	});

	test('closing the quarantine switch leaves its active list entry in place', async () => {
		await rule({});
		const { row, actionId } = await created(823, 'QUARANTINE_24H');
		const list = await serverListOf(env, { id: world.server.id, orgId: world.org.id }, 'ban');
		const entry = await grantEntry(env, list, {
			steamId: row.steamId!, reason: 'Integrity quarantine',
			expiresAt: new Date(Date.now() + 24 * 3600_000), addedByName: 'Community Integrity'
		});
		await env.db.update(integrityActions).set({ listEntryId: entry.id }).where(eq(integrityActions.id, actionId));
		await rule({ autoQuarantine24hEnabled: false });
		await deliverOne(env, row);
		expect((await outboxOf(row.id)).state).toBe('skipped');
		expect((await actionOf(actionId)).effectiveAt).toBeInstanceOf(Date);
		expect((await env.db.select().from(listEntries).where(eq(listEntries.id, entry.id)))[0].removedAt).toBeNull();
		expect(clientSpy).not.toHaveBeenCalled();
	});

	test('mismatched case identity and reverted actions fail closed', async () => {
		await rule({});
		const { row, actionId } = await created(824);
		expect(await integrityDeliverySkipReason(env, { ...row, steamId: steamId(825) })).toContain('changed');
		await env.db.update(integrityActions).set({ revertedAt: new Date() }).where(eq(integrityActions.id, actionId));
		expect(await integrityDeliverySkipReason(env, row)).toContain('changed');
	});
});
