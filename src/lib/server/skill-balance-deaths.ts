import { and, desc, eq, sql } from 'drizzle-orm';
import type { Env } from './env';
import {
	skillBalanceRuns,
	skillBalanceRules,
	servers,
	matches,
	kills,
	factionMovePermits,
	organizations,
	type KillRow
} from './db/schema';
import type { BalanceMove } from './skill-balance';
import { planSkillBalance, type BalancePlan } from '$lib/skill-balance-policy';
import type { Player, Status } from '$lib/types';
import { mapId } from '$lib/format';
import { isOwner, withOwnedTransaction } from './leadership';
import { withServer, PRIORITY } from './dispatcher';
import { WardogsClient, GameError } from './rcon';
import { ACTIONS } from './actions';
import { writeAudit } from './audit';

export const BALANCE_DEATH_MAX_AGE_MS = 5000;
type FrozenPlan = BalancePlan & {
	selectedClock: number;
	instanceId: string;
	gameMatchId: string;
	ruleUpdatedAt: string;
};
export function freshBalanceDeath(
	k: KillRow,
	selectedAt: Date,
	plan: FrozenPlan,
	now = Date.now()
) {
	return (
		!!k.killerSteamId &&
		/^\d{17}$/.test(k.killerSteamId) &&
		k.killerSteamId !== k.victimSteamId &&
		!k.suicide &&
		!k.teamKill &&
		k.ts.getTime() > selectedAt.getTime() &&
		now - k.ts.getTime() >= 0 &&
		now - k.ts.getTime() <= BALANCE_DEATH_MAX_AGE_MS &&
		k.instanceId === plan.instanceId &&
		k.matchId === plan.gameMatchId &&
		k.eventTime > plan.selectedClock
	);
}

/** Legacy Feed consumer: dispatch promptly, independently of the slower statistical consumer. */
export async function processSkillBalanceDeaths(
	env: Env,
	serverId: string,
	events: readonly KillRow[]
) {
	if (!isOwner() || !events.some((k) => Date.now() - k.ts.getTime() <= BALANCE_DEATH_MAX_AGE_MS))
		return;
	const [run] = await env.db
		.select()
		.from(skillBalanceRuns)
		.where(
			and(eq(skillBalanceRuns.serverId, serverId), eq(skillBalanceRuns.state, 'waiting_death'))
		)
		.orderBy(desc(skillBalanceRuns.createdAt))
		.limit(1);
	if (!run) return;
	const plan = run.plan as FrozenPlan;
	const candidates = events
		.filter((k) => k.matchRow === run.matchId && freshBalanceDeath(k, run.createdAt, plan))
		.sort((a, b) => b.eventTime - a.eventTime);
	if (!candidates.length) return;
	const [server] = await env.db.select().from(servers).where(eq(servers.id, serverId));
	if (!server) return;
	const client = await WardogsClient.forServer(env, server);
	for (const death of candidates) {
		if (!freshBalanceDeath(death, run.createdAt, plan)) continue;
		await withServer(
			serverId,
			PRIORITY.delivery,
			async () => {
				if (!isOwner() || !freshBalanceDeath(death, run.createdAt, plan)) return;
				const [[rule], [round], [current], [org]] = await Promise.all([
					env.db.select().from(skillBalanceRules).where(eq(skillBalanceRules.serverId, serverId)),
					env.db.select().from(matches).where(eq(matches.id, run.matchId)),
					env.db.select().from(skillBalanceRuns).where(eq(skillBalanceRuns.id, run.id)),
					env.db.select().from(organizations).where(eq(organizations.id, server.orgId))
				]);
				if (!current || current.state !== 'waiting_death' || !org || org.suspendedAt) return;
				const moves = current.moves as BalanceMove[];
				const index = moves.findIndex(
					(m) => m.steamId === death.victimSteamId && m.state === 'waiting'
				);
				if (index < 0) return;
				const cancel = async (reason: string) => {
					await env.db
						.update(skillBalanceRuns)
						.set({ state: 'cancelled', reason, updatedAt: new Date() })
						.where(
							and(eq(skillBalanceRuns.id, run.id), eq(skillBalanceRuns.state, 'waiting_death'))
						);
				};
				if (
					!rule?.enabled ||
					rule.updatedAt.toISOString() !== plan.ruleUpdatedAt ||
					!round ||
					round.endedAt
				) {
					await cancel('已关闭、设置改变或对局结束；已完成调队不强制撤回');
					return;
				}
				// Every operation goes through fresh server state in the same dispatcher lane.
				const status = (await ACTIONS.status.run(client, {})) as Status;
				const roster = ((await ACTIONS.players.run(client, {})) as { players: Player[] }).players;
				if (
					mapId(status.map) !== mapId(round.map ?? '') ||
					(status.matchSeconds !== null && status.matchSeconds < death.eventTime - 5) ||
					(status.scoreCap !== null && status.scores.some((s) => s.score >= status.scoreCap!))
				) {
					await cancel('地图或对局阶段改变，停止后续调队');
					return;
				}
				const idxPair = Math.floor(index / 2),
					other = moves[idxPair * 2 + (index % 2 === 0 ? 1 : 0)];
				if (['sending', 'unknown'].includes(other.state)) return;
				const pairStarted = other.state === 'confirmed';
				const still = planSkillBalance(
					status,
					plan.pairs.flatMap((p) => [p.strong, p.weak]),
					rule.leadPoints
				);
				if (!pairStarted && (!still || still.strong !== plan.strong || still.weak !== plan.weak))
					return;
				const target = moves[index],
					player = roster.find((p) => p.steamId === target.steamId);
				if (!player || player.faction !== target.from) return;
				// A later kill by this player is direct evidence they are active again; never use the earlier death.
				const [later] = await env.db
					.select({ id: kills.eventId })
					.from(kills)
					.where(
						and(
							eq(kills.serverId, serverId),
							eq(kills.matchRow, run.matchId),
							eq(kills.killerSteamId, target.steamId),
							sql`${kills.eventTime}>${death.eventTime}`
						)
					)
					.limit(1);
				if (later) return;
				const [latest] = await env.db
					.select({ clock: sql<number>`max(${kills.eventTime})` })
					.from(kills)
					.where(
						and(
							eq(kills.serverId, serverId),
							eq(kills.matchRow, run.matchId),
							eq(kills.instanceId, death.instanceId)
						)
					);
				if (Number(latest?.clock) > death.eventTime + 5) return;
				const [permit] = await env.db
					.select({ id: factionMovePermits.id })
					.from(factionMovePermits)
					.where(
						and(
							eq(factionMovePermits.serverId, serverId),
							eq(factionMovePermits.steamId, target.steamId),
							sql`${factionMovePermits.expiresAt}>now()`
						)
					)
					.limit(1);
				if (permit) return;
				// Durable claim BEFORE network dispatch. Uncertain/crashed sends are never replayed.
				const claimed = await withOwnedTransaction(env, async (tx) => {
					const [r] = await tx
						.select()
						.from(skillBalanceRules)
						.where(eq(skillBalanceRules.serverId, serverId))
						.for('share');
					const [locked] = await tx
						.select()
						.from(skillBalanceRuns)
						.where(eq(skillBalanceRuns.id, run.id))
						.for('update');
					if (
						!r?.enabled ||
						r.updatedAt.toISOString() !== plan.ruleUpdatedAt ||
						locked.state !== 'waiting_death' ||
						!freshBalanceDeath(death, run.createdAt, plan)
					)
						return null;
					const list = locked.moves as BalanceMove[],
						m = list[index];
					if (
						m.state !== 'waiting' ||
						m.eventId === death.eventId ||
						(m.eventTime !== undefined && m.eventTime >= death.eventTime)
					)
						return null;
					list[index] = {
						...m,
						state: 'sending',
						eventId: death.eventId,
						instanceId: death.instanceId,
						eventTime: death.eventTime,
						attemptedAt: new Date().toISOString()
					};
					await tx
						.update(skillBalanceRuns)
						.set({ moves: list, updatedAt: new Date(), reason: '收到新的被击杀事件，正在调队' })
						.where(eq(skillBalanceRuns.id, run.id));
					// WardogsClient.raw records the faction-lock permit immediately before PATCH.
					return list;
				});
				if (!claimed) return;
				let state = 'unknown',
					reason = '';
				try {
					const [dispatchRule] = await env.db
						.select()
						.from(skillBalanceRules)
						.where(eq(skillBalanceRules.serverId, serverId));
					if (
						!dispatchRule?.enabled ||
						dispatchRule.updatedAt.toISOString() !== plan.ruleUpdatedAt ||
						!isOwner() ||
						!freshBalanceDeath(death, run.createdAt, plan)
					) {
						state = 'waiting';
						reason = '死亡事件在排队中超时，等待下一次被击杀';
					} else {
						// Deliberately do NOT call ACTIONS.changeTeam: it also kills the player.
						await client.json('PATCH', `/v1/players/${target.steamId}`, { faction: target.to });
						const after = (
							(await ACTIONS.players.run(client, {})) as { players: Player[] }
						).players.find((p) => p.steamId === target.steamId);
						state = after?.faction === target.to ? 'confirmed' : 'unknown';
						reason =
							state === 'confirmed'
								? '被击杀事件触发，阵营已确认；未调用强制死亡'
								: '调队结果未确认，停止自动重试';
					}
				} catch (e) {
					state =
						e instanceof GameError && ['faction_full', 'team_full'].includes(e.code)
							? 'waiting'
							: 'unknown';
					reason =
						state === 'waiting'
							? '目标阵营满员，调队保护冷却120秒后等待新的被击杀事件'
							: e instanceof Error
								? e.message
								: '调队失败，结果未知';
				}
				await env.db.transaction(async (tx) => {
					const [locked] = await tx
						.select()
						.from(skillBalanceRuns)
						.where(eq(skillBalanceRuns.id, run.id))
						.for('update');
					const list = locked.moves as BalanceMove[];
					list[index] = { ...list[index], state, reason };
					const final = list.every((m) => m.state === 'confirmed') ? 'done' : locked.state;
					await tx
						.update(skillBalanceRuns)
						.set({
							moves: list,
							state: final,
							reason: final === 'done' ? '六名玩家均在各自被击杀事件后完成调队' : reason,
							updatedAt: new Date()
						})
						.where(eq(skillBalanceRuns.id, run.id));
				});
				await writeAudit(env, null, {
					server,
					orgId: server.orgId,
					category: 'trigger',
					action: 'skill_balance.death_move',
					target: target.steamId,
					actorName: '强弱阵营平衡',
					outcome: state === 'confirmed' ? 'ok' : 'error',
					message: reason,
					detail: { runId: run.id, eventId: death.eventId, from: target.from, to: target.to, state }
				});
			},
			BALANCE_DEATH_MAX_AGE_MS
		);
	}
}
