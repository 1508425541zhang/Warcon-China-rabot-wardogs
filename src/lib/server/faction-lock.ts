import { and, desc, eq, gte, inArray, isNull, lt } from 'drizzle-orm';
import type { Env } from './env';
import type { ServerRow } from './db/schema';
import type { Player, Status } from '$lib/types';
import { factionLockRules, factionLockEvents, factionMovePermits, matches } from './db/schema';
import { factionLockDecision, originalFactionLeading } from '$lib/faction-lock-policy';
import { ACTIONS } from './actions';
import { GameError, type WardogsClient } from './rcon';
import { isOwner } from './leadership';
import { writeAudit } from './audit';

export async function factionLockView(env: Env, serverId: string) {
	const [[rule], events] = await Promise.all([
		env.db.select().from(factionLockRules).where(eq(factionLockRules.serverId, serverId)),
		env.db
			.select()
			.from(factionLockEvents)
			.where(eq(factionLockEvents.serverId, serverId))
			.orderBy(desc(factionLockEvents.id))
			.limit(50)
	]);
	return {
		rule: rule ?? { enabled: false, graceSeconds: 120, capacities: {} },
		events: events.map((e) => ({
			...e,
			createdAt: e.createdAt.toISOString(),
			updatedAt: e.updatedAt.toISOString()
		}))
	};
}
/** Only called on a fresh roster. Pending operations must also see a fresh status. */
export async function runFactionLock(
	env: Env,
	server: ServerRow,
	client: WardogsClient,
	input: {
		players: Player[];
		status: Status;
		statusAt: number;
		trusted: boolean;
		changes: { player: Player; from: string | null }[];
		startupAt: number;
		boundary: boolean;
		now: Date;
	}
) {
	const { now, players, status } = input;
	const [rule] = await env.db
		.select()
		.from(factionLockRules)
		.where(eq(factionLockRules.serverId, server.id));
	if (!rule?.enabled || !isOwner()) return;
	const [round] = await env.db
		.select()
		.from(matches)
		.where(and(eq(matches.serverId, server.id), isNull(matches.endedAt)))
		.orderBy(desc(matches.id))
		.limit(1);
	if (!round) return;
	await env.db
		.update(factionLockEvents)
		.set({ state: 'error', reason: '执行中断，结果未知；停止自动重试', updatedAt: now })
		.where(
			and(
				eq(factionLockEvents.serverId, server.id),
				eq(factionLockEvents.state, 'executing'),
				lt(factionLockEvents.updatedAt, new Date(now.getTime() - 120000))
			)
		);
	const teams = status.scores.map((s) => s.name);
	const transition =
		input.boundary ||
		!input.trusted ||
		now.getTime() - round.startedAt.getTime() < rule.graceSeconds * 1000 ||
		now.getTime() - input.startupAt < rule.graceSeconds * 1000 ||
		round.map !== status.map ||
		(status.matchSeconds !== null && status.matchSeconds < rule.graceSeconds) ||
		(status.scoreCap !== null && status.scores.some((s) => s.score >= status.scoreCap!));
	const permits = await env.db
		.select()
		.from(factionMovePermits)
		.where(and(eq(factionMovePermits.serverId, server.id), gte(factionMovePermits.expiresAt, now)));
	const authorized = (id: string, to: string) =>
		permits.some((p) => p.steamId === id && p.faction === to);
	const full = (faction: string): boolean | null => {
		const cap = (rule.capacities as Record<string, unknown>)[faction];
		return typeof cap === 'number' && Number.isInteger(cap) && cap > 0
			? players.filter((p) => p.faction === faction).length >= cap
			: null;
	};
	const pending = await env.db
		.select()
		.from(factionLockEvents)
		.where(
			and(
				eq(factionLockEvents.serverId, server.id),
				inArray(factionLockEvents.state, ['pending', 'warned'])
			)
		);
	for (const change of input.changes) {
		const p = change.player;
		if (pending.some((e) => e.steamId === p.steamId)) continue;
		const decision = factionLockDecision({
			from: change.from,
			to: p.faction,
			teams,
			authorized: authorized(p.steamId, p.faction ?? ''),
			transition,
			full: null,
			leading: null
		});
		if (decision === 'UNCHANGED') continue;
		await env.db.insert(factionLockEvents).values({
			serverId: server.id,
			matchId: round.id,
			steamId: p.steamId,
			fromFaction: change.from ?? '',
			toFaction: p.faction ?? '',
			state: decision === 'RESTORE' ? 'pending' : 'skipped',
			reason: decision,
			createdAt: now,
			updatedAt: now
		});
	}
	const finish = async (
		e: typeof factionLockEvents.$inferSelect,
		state: string,
		reason: string
	) => {
		await env.db
			.update(factionLockEvents)
			.set({ state, reason, updatedAt: now })
			.where(eq(factionLockEvents.id, e.id));
		await writeAudit(env, null, {
			server,
			orgId: server.orgId,
			category: 'trigger',
			action: 'faction_lock.' + state,
			target: e.steamId,
			actorName: '禁止自行换边',
			outcome: state === 'error' ? 'error' : 'ok',
			message: reason,
			detail: { from: e.fromFaction, to: e.toFaction, incident: e.id }
		});
	};
	for (const e of pending) {
		const p = players.find((p) => p.steamId === e.steamId);
		if (
			!p ||
			e.matchId !== round.id ||
			transition ||
			p.faction !== e.toFaction ||
			authorized(e.steamId, e.toFaction)
		) {
			await finish(e, 'skipped', '玩家离线、已变更阵营、处于换图保护或存在面板调队记录');
			continue;
		}
		if (now.getTime() - e.createdAt.getTime() > 120000) {
			await finish(e, 'skipped', '事件过期，停止执行');
			continue;
		}
		if (now.getTime() - e.updatedAt.getTime() < 8000 || now.getTime() - input.statusAt > 10000)
			continue;
		if (!isOwner()) return;
		const [currentRule] = await env.db
			.select()
			.from(factionLockRules)
			.where(eq(factionLockRules.serverId, server.id));
		if (!currentRule?.enabled) return;
		// Re-read permits immediately before effects, including admin moves issued after this tick began.
		const [permit] = await env.db
			.select()
			.from(factionMovePermits)
			.where(
				and(
					eq(factionMovePermits.serverId, server.id),
					eq(factionMovePermits.steamId, e.steamId),
					gte(factionMovePermits.createdAt, e.createdAt),
					gte(factionMovePermits.expiresAt, now)
				)
			)
			.limit(1);
		if (permit && permit.faction !== e.fromFaction) {
			await finish(e, 'skipped', '新的面板调队操作');
			continue;
		}
		const isFull = full(e.fromFaction),
			leading = originalFactionLeading(status.scores, e.fromFaction);
		const [claim] = await env.db
			.update(factionLockEvents)
			.set({ state: 'executing', updatedAt: now })
			.where(and(eq(factionLockEvents.id, e.id), eq(factionLockEvents.state, e.state)))
			.returning();
		if (!claim) continue;
		try {
			let confirmedFull = isFull === true;
			if (!confirmedFull) {
				try {
					await ACTIONS.changeTeam.run(client, { steamId: e.steamId, faction: e.fromFaction });
					await finish(e, 'restored', '已调回原阵营');
					continue;
				} catch (err) {
					if (err instanceof GameError && ['faction_full', 'team_full'].includes(err.code))
						confirmedFull = true;
					else throw err;
				}
			}
			if (!confirmedFull || leading !== true) {
				await finish(
					e,
					'skipped',
					leading === false ? '原阵营已满但未领先，保持当前阵营' : '原阵营分数不可用，不踢出'
				);
				continue;
			}
			if (e.state === 'warned') {
				await ACTIONS.kick.run(client, {
					steamId: e.steamId,
					reason: '禁止自行换边：原阵营已满且领先，无法调回。'
				});
				await finish(e, 'kicked', '警告后重新确认原阵营已满且领先，已踢出');
			} else {
				await ACTIONS.whisper.run(client, {
					steamId: e.steamId,
					message: '禁止自行换边：原阵营已满且领先，无法调回。8 秒后仍符合条件将踢出。'
				});
				await finish(e, 'warned', '警告已发送，等待重新检查后踢出');
			}
		} catch (err) {
			await finish(e, 'error', err instanceof Error ? err.message : '操作失败');
		}
	}
	// Expired permits are not evidence and should not accumulate indefinitely.
	await env.db
		.delete(factionMovePermits)
		.where(
			and(
				eq(factionMovePermits.serverId, server.id),
				lt(factionMovePermits.expiresAt, new Date(now.getTime() - 86400000))
			)
		);
}
