import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callApi, stubGateway } from './call';
import { GET, POST } from '../routes/api/servers/[id]/scheduled-tasks/+server';
import { getServer, serverAccessFor } from '$lib/server/access';
import {
	changeScheduledTasks,
	runScheduledTasks,
	scheduledTasksView,
	scheduledDeliverySkipReason
} from '$lib/server/scheduled-tasks';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import {
	matches,
	servers,
	weaponRestrictionRules,
	factionLockRules,
	user,
	triggers,
	outbox,
	siteSettings
} from '$lib/server/db/schema';
import {
	scheduleDue,
	scheduleInputSchema,
	type ScheduledTask,
	type ScheduleInput,
	type ScheduleStore
} from '$lib/scheduled-task-policy';

test('schedule times require offsets; exact due/grace boundaries and a new match identity', () => {
	const now = Date.now();
	const task: ScheduledTask = {
		id: 'test',
		authorId: 'a',
		name: 'test',
		action: { kind: 'faction_lock', enabled: true },
		when: { kind: 'at', at: new Date(now).toISOString() },
		baselineMatch: 5,
		createdAt: new Date(now - 1000).toISOString(),
		state: 'pending',
		finishedAt: null,
		outcome: ''
	};
	expect(scheduleDue(task, now - 1, null)).toBe('wait');
	expect(scheduleDue(task, now, null)).toBe('run');
	expect(scheduleDue(task, now + 300000, null)).toBe('run');
	expect(scheduleDue(task, now + 300001, null)).toBe('expired');
	task.when = { kind: 'next_match' };
	expect(scheduleDue(task, now, { id: 5, startedAt: new Date(now) })).toBe('wait');
	expect(scheduleDue(task, now, { id: 6, startedAt: new Date(now) })).toBe('run');
	expect(
		scheduleInputSchema.safeParse({
			name: 'x',
			when: { kind: 'at', at: '2026-10-02T12:00' },
			action: task.action
		}).success
	).toBe(false);
	expect(
		scheduleInputSchema.safeParse({
			name: 'x',
			when: { kind: 'next_match' },
			action: { kind: 'weapon_restriction', enabled: true, causes: [], groups: [] }
		}).success
	).toBe(false);
});
describe.skipIf(!hasTestDb)('durable planned automation', () => {
	async function setup() {
		const env = await testEnv(),
			w = await seedWorld(env);
		stubGateway();
		await acquireOrRenew(env, 'scheduled-tests');
		await env.db
			.update(servers)
			.set({ feedTokenHash: crypto.randomUUID() })
			.where(eq(servers.id, w.server.id));
		const server = (await getServer(env, w.server.id))!,
			author = w.users.admin!,
			access = (await serverAccessFor(env, author, server.id))!;
		const [match] = await env.db
			.insert(matches)
			.values({ serverId: server.id, map: 'Europe', startedAt: new Date(Date.now() - 600000) })
			.returning();
		async function update(body: Record<string, unknown>) {
			const current = await scheduledTasksView(env, server.id);
			return changeScheduledTasks(env, server, access, author, {
				revision: current.revision,
				...body
			});
		}
		async function create(task: ScheduleInput) {
			return (await update({ operation: 'create', task })).tasks.at(-1)!;
		}
		const run = (at: Date = new Date(), statusAt = at.getTime()) =>
			runScheduledTasks(env, server, { now: at, statusAt, map: 'Europe' });
		return { env, w, server, author, access, match, update, create, run };
	}
	test('routes isolate servers, reject viewers and API keys, enforce operation capabilities and revisions', async () => {
		const s = await setup();
		for (const principal of ['anon', 'viewer', 'elsewhere', 'outsider', 'keyView'] as const)
			expect(
				(await callApi(GET, s.w.users[principal], { params: { id: s.server.id } })).status
			).not.toBe(200);
		expect((await callApi(GET, s.author, { params: { id: s.server.id } })).status).toBe(200);
		expect(
			(
				await callApi(POST, s.w.users.keyAll, {
					method: 'POST',
					params: { id: s.server.id },
					body: { revision: '', operation: 'settings', enabled: true }
				})
			).status
		).toBe(403);
		await s.update({ operation: 'settings', enabled: true });
		expect(
			(
				await callApi(POST, s.author, {
					method: 'POST',
					params: { id: s.server.id },
					body: { revision: '', operation: 'settings', enabled: false }
				})
			).status
		).toBe(409);
		const task = {
			name: 'x',
			when: { kind: 'next_match' },
			action: { kind: 'faction_lock', enabled: true }
		};
		await expect(
			changeScheduledTasks(
				s.env,
				s.server,
				{ ...s.access, caps: new Set(['automation.manage']) },
				s.author,
				{
					revision: (await scheduledTasksView(s.env, s.server.id)).revision,
					operation: 'create',
					task
				}
			)
		).rejects.toMatchObject({ status: 403 });
		expect(
			(
				await callApi(POST, s.author, {
					method: 'POST',
					params: { id: s.w.otherServer.id },
					body: { revision: '', operation: 'settings', enabled: true }
				})
			).status
		).toBe(404);
	});
	test('next round applies exact weapon choices once, including a same-map next round; restart same round never triggers', async () => {
		const s = await setup();
		await s.update({ operation: 'settings', enabled: true });
		await s.create({
			name: '武器',
			when: { kind: 'next_match' },
			action: {
				kind: 'weapon_restriction',
				enabled: true,
				causes: ['Id.Item.SVD', 'Id.Item.SVD'],
				groups: []
			}
		});
		await s.run();
		expect((await scheduledTasksView(s.env, s.server.id)).tasks[0].state).toBe('pending');
		const now = new Date();
		await s.env.db.update(matches).set({ endedAt: now }).where(eq(matches.id, s.match.id));
		await s.env.db.insert(matches).values({ serverId: s.server.id, map: 'Europe', startedAt: now });
		await Promise.all([s.run(), s.run()]);
		const [rule] = await s.env.db
			.select()
			.from(weaponRestrictionRules)
			.where(eq(weaponRestrictionRules.serverId, s.server.id));
		expect(rule.causes).toEqual(['Id.Item.SVD']);
		expect(rule.enabled).toBe(true);
		const version = rule.updatedAt.getTime();
		await s.run();
		expect(
			(
				await s.env.db
					.select()
					.from(weaponRestrictionRules)
					.where(eq(weaponRestrictionRules.serverId, s.server.id))
			)[0].updatedAt.getTime()
		).toBe(version);
		expect((await scheduledTasksView(s.env, s.server.id)).tasks[0].state).toBe('applied');
	});
	test('at-time faction lock preserves capacities and grace; pause, stale status and cancellation stop work', async () => {
		const s = await setup();
		const at = new Date(Date.now() + 1000);
		await s.env.db
			.insert(factionLockRules)
			.values({ serverId: s.server.id, enabled: false, graceSeconds: 90, capacities: { Red: 40 } });
		await s.create({
			name: '禁止换边',
			when: { kind: 'at', at: at.toISOString() },
			action: { kind: 'faction_lock', enabled: true }
		});
		await s.run(at);
		expect((await scheduledTasksView(s.env, s.server.id)).tasks[0].state).toBe('pending');
		await s.update({ operation: 'settings', enabled: true });
		await s.run(at, at.getTime() - 30001);
		expect((await scheduledTasksView(s.env, s.server.id)).tasks[0].state).toBe('pending');
		await s.run(at);
		const [rule] = await s.env.db
			.select()
			.from(factionLockRules)
			.where(eq(factionLockRules.serverId, s.server.id));
		expect(rule.enabled).toBe(true);
		expect(rule.graceSeconds).toBe(90);
		expect(rule.capacities).toEqual({ Red: 40 });
		const task = await s.create({
			name: '允许换边',
			when: { kind: 'at', at: at.toISOString() },
			action: { kind: 'faction_lock', enabled: false }
		});
		await s.update({ operation: 'cancel', taskId: task.id });
		await s.run(at);
		expect(
			(
				await s.env.db
					.select()
					.from(factionLockRules)
					.where(eq(factionLockRules.serverId, s.server.id))
			)[0].enabled
		).toBe(true);
	});
	test('expired windows and disabled authors are skipped rather than applying stale restrictions', async () => {
		const s = await setup(),
			at = new Date(Date.now() + 1000);
		await s.update({ operation: 'settings', enabled: true });
		await s.create({
			name: '过期',
			when: { kind: 'at', at: at.toISOString() },
			action: { kind: 'faction_lock', enabled: true }
		});
		await s.run(new Date(at.getTime() + 300001));
		expect((await scheduledTasksView(s.env, s.server.id)).tasks[0].outcome).toContain('错过');
		await s.create({
			name: '失权',
			when: { kind: 'at', at: at.toISOString() },
			action: { kind: 'faction_lock', enabled: true }
		});
		await s.env.db.update(user).set({ banned: true }).where(eq(user.id, s.author.id));
		await s.run(at);
		expect((await scheduledTasksView(s.env, s.server.id)).tasks.at(-1)!.outcome).toContain('停用');
		expect(
			await s.env.db
				.select()
				.from(factionLockRules)
				.where(eq(factionLockRules.serverId, s.server.id))
		).toHaveLength(0);
	});
	test('trigger switches reset ping counters and refuse a target in another server', async () => {
		const s = await setup(),
			at = new Date(Date.now() + 1000);
		await s.update({ operation: 'settings', enabled: true });
		const [trigger] = await s.env.db
			.insert(triggers)
			.values({
				id: crypto.randomUUID(),
				serverId: s.server.id,
				orgId: s.server.orgId,
				kind: 'ping_kick',
				name: 'ping',
				enabled: true,
				config: {},
				state: { old: 1 },
				createdBy: s.author.id
			})
			.returning();
		await s.create({
			name: '停用规则',
			when: { kind: 'at', at: at.toISOString() },
			action: { kind: 'trigger', triggerId: trigger.id, enabled: false }
		});
		await s.run(at);
		const [updated] = await s.env.db.select().from(triggers).where(eq(triggers.id, trigger.id));
		expect(updated.enabled).toBe(false);
		expect(updated.state).toBeNull();
		expect(
			await scheduledDeliverySkipReason(s.env, {
				triggerId: trigger.id,
				serverId: s.server.id,
				triggerKind: 'ping_kick',
				detail: {},
				createdAt: new Date()
			})
		).toContain('停用');
		await s.env.db
			.update(triggers)
			.set({ serverId: s.w.otherServer.id })
			.where(eq(triggers.id, trigger.id));
		await expect(
			s.create({
				name: '越权',
				when: { kind: 'next_match' },
				action: { kind: 'trigger', triggerId: trigger.id, enabled: true }
			})
		).rejects.toMatchObject({ status: 404 });
	});
	test('broadcast is durably queued only once and checked again for cancellation and authority before delivery', async () => {
		const s = await setup(),
			at = new Date(Date.now() + 1000);
		await s.update({ operation: 'settings', enabled: true });
		const task = await s.create({
			name: '广播',
			when: { kind: 'at', at: at.toISOString() },
			action: { kind: 'broadcast', message: '新一局规则已生效' }
		});
		await s.run(at);
		await s.run(at);
		const messages = await s.env.db.select().from(outbox).where(eq(outbox.serverId, s.server.id));
		expect(messages).toHaveLength(1);
		expect(await scheduledDeliverySkipReason(s.env, messages[0])).toBeNull();
		await s.env.db.update(user).set({ banned: true }).where(eq(user.id, s.author.id));
		expect(await scheduledDeliverySkipReason(s.env, messages[0])).toContain('停用');
		await s.update({ operation: 'cancel', taskId: task.id });
		expect(await scheduledDeliverySkipReason(s.env, messages[0])).toContain('取消');
	});
	test('completed receipts are bounded at 100 and lost worker ownership cannot apply a task', async () => {
		const s = await setup(),
			at = new Date(Date.now() + 1000);
		await s.update({ operation: 'settings', enabled: true });
		await s.create({
			name: '失去主控',
			when: { kind: 'at', at: at.toISOString() },
			action: { kind: 'faction_lock', enabled: true }
		});
		await releaseOwnership(s.env);
		await s.run(at);
		expect((await scheduledTasksView(s.env, s.server.id)).tasks[0].state).toBe('pending');
		const state = await scheduledTasksView(s.env, s.server.id),
			pending = state.tasks[0];
		const value: ScheduleStore = {
			...state,
			tasks: [
				...Array.from({ length: 120 }, (_, i) => ({
					...pending,
					id: 'finished-' + i,
					state: 'applied' as const,
					finishedAt: new Date().toISOString()
				})),
				pending
			]
		};
		await s.env.db
			.update(siteSettings)
			.set({ value })
			.where(eq(siteSettings.key, 'scheduledTasks:' + s.server.id));
		await s.update({ operation: 'settings', enabled: false });
		const bounded = await scheduledTasksView(s.env, s.server.id);
		expect(bounded.tasks).toHaveLength(101);
		expect(bounded.tasks.some((t) => t.id === pending.id)).toBe(true);
	});
});
