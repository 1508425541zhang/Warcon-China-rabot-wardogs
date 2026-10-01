import { and, desc, eq, inArray, isNull, sql } from 'drizzle-orm';
import { randomUUID } from 'node:crypto';
import type { Env } from './env';
import type { DbOrTx } from './db';
import {
	siteSettings,
	matches,
	triggers,
	weaponRestrictionRules,
	factionLockRules,
	factionLockEvents,
	user,
	outbox,
	type ServerRow,
	type OutboxRow
} from './db/schema';
import { ApiError } from './http';
import {
	serverAccessFor,
	toSessionUser,
	type SessionUser,
	type ServerAccess,
	getServer
} from './access';
import { requireRuleCaps } from './triggers';
import { isOwner, withOwnedTransaction } from './leadership';
import { gateway } from './gateway';
import { wakeDelivery } from './outbox';
import { writeAudit } from './audit';
import {
	scheduleDue,
	scheduleInputSchema,
	SCHEDULE_GRACE_MS,
	type ScheduleStore,
	type ScheduledTask,
	type ScheduledAction
} from '$lib/scheduled-task-policy';
const key = (id: string) => 'scheduledTasks:' + id;
async function store(db: DbOrTx, id: string, lock = false): Promise<ScheduleStore> {
	const [row] = await db.execute<{ value: ScheduleStore }>(
		sql`SELECT value FROM site_settings WHERE key=${key(id)} ${lock ? sql`FOR UPDATE` : sql``}`
	);
	return row?.value ?? { revision: '', enabled: false, tasks: [] };
}
async function save(db: DbOrTx, id: string, value: ScheduleStore) {
	value.revision = randomUUID();
	// Keep all pending/queued work and at most 100 completed receipts.
	const pending = value.tasks.filter((t) => ['pending', 'queued'].includes(t.state));
	const done = value.tasks.filter((t) => !['pending', 'queued'].includes(t.state)).slice(-100);
	value.tasks = [...done, ...pending];
	await db
		.insert(siteSettings)
		.values({ key: key(id), value, updatedAt: new Date() })
		.onConflictDoUpdate({ target: siteSettings.key, set: { value, updatedAt: new Date() } });
}
export async function scheduledTasksView(env: Env, id: string) {
	const value = await store(env.db, id);
	const messages = await env.db
		.select({ state: outbox.state, outcome: outbox.outcome, detail: outbox.detail })
		.from(outbox)
		.where(and(eq(outbox.serverId, id), eq(outbox.triggerKind, 'scheduled_task')))
		.orderBy(desc(outbox.id))
		.limit(200);
	return {
		...value,
		tasks: value.tasks.map((t) => {
			const message = messages.find((m) => (m.detail as { taskId?: string })?.taskId === t.id);
			return {
				...t,
				deliveryState: message?.state ?? null,
				deliveryOutcome: message?.outcome ?? ''
			};
		})
	};
}
async function checkAction(
	env: Env,
	server: ServerRow,
	access: ServerAccess,
	action: ScheduledAction
) {
	if (!access.caps.has('automation.manage')) throw new ApiError(403, '需要自动化管理权限。');
	const needs =
		action.kind === 'broadcast'
			? ['chat.send']
			: action.kind === 'faction_lock'
				? ['players.moderate', 'chat.send']
				: action.kind === 'weapon_restriction'
					? ['players.moderate']
					: [];
	if (needs.some((c) => !access.caps.has(c as import('$lib/capabilities').Capability)))
		throw new ApiError(403, '没有此任务所需的操作权限。');
	if (action.kind === 'weapon_restriction' && action.enabled && !server.feedTokenHash)
		throw new ApiError(400, '启用武器限制需要先配置 Kill Feed。');
	if (action.kind === 'trigger') {
		const [rule] = await env.db
			.select()
			.from(triggers)
			.where(and(eq(triggers.id, action.triggerId), eq(triggers.serverId, server.id)));
		if (!rule) throw new ApiError(404, '自动化规则已删除或不属于本服务器。');
		requireRuleCaps(rule.kind as import('$lib/types').TriggerKind, rule.config, server, access);
	}
}
async function authorAccess(env: Env, server: ServerRow, id: string) {
	const [person] = await env.db.select().from(user).where(eq(user.id, id));
	if (!person || person.banned) throw new ApiError(403, '任务创建者已停用或删除。');
	const access = await serverAccessFor(env, toSessionUser(person), server.id);
	if (!access) throw new ApiError(403, '任务创建者已失去服务器权限。');
	return access;
}
export async function changeScheduledTasks(
	env: Env,
	server: ServerRow,
	access: ServerAccess,
	author: SessionUser,
	body: Record<string, unknown>
) {
	if (author.apiKey) throw new ApiError(403, '计划任务需要管理员账号会话。');
	if (!access.caps.has('automation.manage')) throw new ApiError(403, '需要自动化管理权限。');
	if (typeof body.revision !== 'string') throw new ApiError(400, '缺少配置版本。');
	return env.db.transaction(async (tx) => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended(${key(server.id)},0))`);
		const value = await store(tx, server.id, true);
		if (value.revision !== body.revision) throw new ApiError(409, '计划任务已改变，请刷新后重试。');
		if (body.operation === 'create') {
			const parsed = scheduleInputSchema.safeParse(body.task);
			if (!parsed.success) throw new ApiError(400, '任务时间或操作配置不正确。');
			const input = parsed.data,
				now = new Date();
			if (input.when.kind === 'at' && Date.parse(input.when.at) <= now.getTime())
				throw new ApiError(400, '执行时间必须在未来，且包含时区。');
			await checkAction({ ...env, db: tx } as unknown as Env, server, access, input.action);
			if (value.tasks.filter((t) => ['pending', 'queued'].includes(t.state)).length >= 100)
				throw new ApiError(400, '最多保留100项未完成任务。');
			const [match] = await tx
				.select({ id: matches.id })
				.from(matches)
				.where(and(eq(matches.serverId, server.id), isNull(matches.endedAt)))
				.orderBy(desc(matches.id))
				.limit(1);
			if (input.when.kind === 'next_match' && !match)
				throw new ApiError(400, '尚未识别当前对局，暂不能设置下一局任务。');
			value.tasks.push({
				...input,
				id: randomUUID(),
				authorId: author.id,
				createdAt: now.toISOString(),
				baselineMatch: match?.id ?? 0,
				state: 'pending',
				finishedAt: null,
				outcome: ''
			});
		} else if (body.operation === 'settings' && typeof body.enabled === 'boolean')
			value.enabled = body.enabled;
		else if (body.operation === 'cancel') {
			const task = value.tasks.find((t) => t.id === body.taskId);
			if (!task || !['pending', 'queued'].includes(task.state))
				throw new ApiError(409, '任务不存在或已执行。');
			if (task.state === 'queued') {
				const [delivery] = await tx
					.select()
					.from(outbox)
					.where(eq(outbox.dedupeKey, 'schedule:' + task.id))
					.limit(1);
				if (delivery && delivery.state !== 'pending')
					throw new ApiError(409, '广播已开始发送或已经完成，不能取消。');
			}
			task.state = 'cancelled';
			task.finishedAt = new Date().toISOString();
			task.outcome = '管理员取消';
		} else if (body.operation === 'clear_finished')
			value.tasks = value.tasks.filter((t) => ['pending', 'queued'].includes(t.state));
		else throw new ApiError(400, '未知计划任务操作。');
		await save(tx, server.id, value);
		return value;
	});
}
export async function runScheduledTasks(
	env: Env,
	server: ServerRow,
	input: { now: Date; statusAt: number; map: string }
) {
	if (!isOwner() || input.now.getTime() - input.statusAt > 30000) return;
	const initial = await store(env.db, server.id);
	if (!initial.enabled || !initial.tasks.some((t) => ['pending', 'queued'].includes(t.state)))
		return;
	const outcomes = await withOwnedTransaction(env, async (tx) => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended(${key(server.id)},0))`);
		const value = await store(tx, server.id, true);
		if (!value.enabled) return [];
		const [match] = await tx
			.select()
			.from(matches)
			.where(and(eq(matches.serverId, server.id), isNull(matches.endedAt)))
			.orderBy(desc(matches.id))
			.limit(1);
		if (!match || match.map !== input.map) return [];
		const scoped = { ...env, db: tx } as unknown as Env,
			done: ScheduledTask[] = [];
		for (const task of value.tasks) {
			if (task.state === 'queued') {
				const [delivery] = await tx
					.select()
					.from(outbox)
					.where(eq(outbox.dedupeKey, 'schedule:' + task.id))
					.limit(1);
				if (delivery && !['pending', 'sending'].includes(delivery.state)) {
					task.state = delivery.state === 'delivered' ? 'applied' : 'skipped';
					task.outcome = delivery.outcome;
					task.finishedAt = (delivery.doneAt ?? input.now).toISOString();
					done.push(task);
				}
				continue;
			}
			const due = scheduleDue(task, input.now.getTime(), match);
			if (due === 'wait') continue;
			if (due === 'expired') {
				task.state = 'skipped';
				task.outcome = '错过执行窗口（最多延迟5分钟），不补执行';
			} else {
				try {
					await checkAction(
						scoped,
						server,
						await authorAccess(scoped, server, task.authorId),
						task.action
					);
				} catch (error) {
					if (!(error instanceof ApiError)) throw error;
					task.state = 'skipped';
					task.outcome = error.message;
				}
				if (task.state === 'pending') {
					const action = task.action;
					if (action.kind === 'weapon_restriction') {
						const values = {
							serverId: server.id,
							enabled: action.enabled,
							causes: [...new Set(action.causes)],
							groups: [...new Set(action.groups)],
							updatedAt: input.now
						};
						await tx
							.insert(weaponRestrictionRules)
							.values(values)
							.onConflictDoUpdate({
								target: weaponRestrictionRules.serverId,
								set: action.enabled ? values : { enabled: false, updatedAt: input.now }
							});
					} else if (action.kind === 'faction_lock') {
						await tx
							.insert(factionLockRules)
							.values({ serverId: server.id, enabled: action.enabled, updatedAt: input.now })
							.onConflictDoUpdate({
								target: factionLockRules.serverId,
								set: { enabled: action.enabled, updatedAt: input.now }
							});
						if (!action.enabled)
							await tx
								.update(factionLockEvents)
								.set({ state: 'skipped', reason: '计划任务关闭换边限制', updatedAt: input.now })
								.where(
									and(
										eq(factionLockEvents.serverId, server.id),
										inArray(factionLockEvents.state, ['pending', 'warned'])
									)
								);
					} else if (action.kind === 'trigger')
						await tx
							.update(triggers)
							.set({
								enabled: action.enabled,
								updatedAt: input.now,
								state: sql`CASE WHEN kind='ping_kick' THEN NULL ELSE state END`
							})
							.where(and(eq(triggers.id, action.triggerId), eq(triggers.serverId, server.id)));
					else {
						await tx
							.insert(outbox)
							.values({
								serverId: server.id,
								triggerName: '计划任务：' + task.name,
								triggerKind: 'scheduled_task',
								action: 'broadcast',
								params: { message: action.message },
								detail: { taskId: task.id },
								dedupeKey: 'schedule:' + task.id,
								okMessage: '计划广播已发送'
							})
							.onConflictDoNothing();
						task.state = 'queued';
					}
					if (task.state === 'pending') task.state = 'applied';
					task.outcome = task.state === 'queued' ? '等待服务器确认广播' : '配置已生效';
				}
			}
			task.finishedAt = task.state === 'queued' ? null : input.now.toISOString();
			done.push(task);
		}
		if (done.length) await save(tx, server.id, value);
		return done;
	});
	if (!outcomes) return;
	for (const task of outcomes) {
		if (task.action.kind === 'trigger' && task.state === 'applied')
			gateway().triggersChanged(server.id);
		await writeAudit(env, null, {
			server,
			orgId: server.orgId,
			actorName: '计划任务',
			category: 'trigger',
			action: 'schedule.' + task.state,
			target: task.id,
			outcome: task.state === 'skipped' ? 'error' : 'ok',
			message: task.outcome,
			detail: { name: task.name, authorId: task.authorId, action: task.action, when: task.when }
		});
	}
	if (outcomes.some((t) => t.state === 'queued')) wakeDelivery();
}
export async function scheduledDeliverySkipReason(
	env: Env,
	row: Pick<OutboxRow, 'triggerKind' | 'triggerId' | 'detail' | 'serverId' | 'createdAt'>
) {
	// An earlier observation may already have queued an intent while this server lane was busy.
	// Recheck the rule inside the delivery lane so a planned stop also stops those intents.
	if (row.triggerId) {
		const [rule] = await env.db
			.select({ enabled: triggers.enabled })
			.from(triggers)
			.where(and(eq(triggers.id, row.triggerId), eq(triggers.serverId, row.serverId)));
		if (!rule?.enabled) return '自动化规则已停用或删除';
	}
	if (row.triggerKind !== 'scheduled_task') return null;
	const value = await store(env.db, row.serverId),
		taskId = (row.detail as { taskId?: string })?.taskId;
	const task = value.tasks.find((t) => t.id === taskId);
	if (!value.enabled || !task || task.state !== 'queued') return '计划任务已取消或停用';
	if (Date.now() - row.createdAt.getTime() > SCHEDULE_GRACE_MS) return '计划广播已过期';
	const server = await getServer(env, row.serverId);
	if (!server) return '服务器已删除';
	try {
		await checkAction(env, server, await authorAccess(env, server, task.authorId), task.action);
	} catch (error) {
		if (error instanceof ApiError) return error.message;
		throw error;
	}
	return null;
}
