import { and, desc, eq, gte, isNull, lt, sql } from 'drizzle-orm';
import { mapId } from '$lib/format';
import { planSkillBalance, type BalancePlan } from '$lib/skill-balance-policy';
import type { Player, Status } from '$lib/types';
import type { Env } from './env';
import {
	skillBalanceRules,
	skillBalanceRuns,
	playerSessions,
	kills,
	matches,
	factionMovePermits,
	type ServerRow
} from './db/schema';
import { isOwner, withOwnedTransaction } from './leadership';
import { ACTIONS } from './actions';
import { GameError, type WardogsClient } from './rcon';
import { writeAudit } from './audit';

export type BalanceMove = {
	steamId: string;
	from: string;
	to: string;
	state: string;
	reason?: string;
};
export async function skillBalanceView(env: Env, serverId: string) {
	const [[rule], runs] = await Promise.all([
		env.db.select().from(skillBalanceRules).where(eq(skillBalanceRules.serverId, serverId)),
		env.db
			.select()
			.from(skillBalanceRuns)
			.where(eq(skillBalanceRuns.serverId, serverId))
			.orderBy(desc(skillBalanceRuns.createdAt))
			.limit(20)
	]);
	return {
		rule: rule ?? { enabled: false, graceSeconds: 300, leadPoints: 40 },
		runs: runs.map((r) => ({
			...r,
			plan: r.plan as BalancePlan,
			moves: r.moves as BalanceMove[],
			createdAt: r.createdAt.toISOString(),
			updatedAt: r.updatedAt.toISOString(),
			state:
				r.state === 'executing' && Date.now() - r.updatedAt.getTime() > 120000 ? 'unknown' : r.state
		}))
	};
}

/** Called inside the observation's server lane. One durable attempt per match, never replay an uncertain move. */
export async function runSkillBalance(
	env: Env,
	server: ServerRow,
	client: WardogsClient,
	input: {
		players: Player[];
		status: Status;
		statusAt: number;
		trusted: boolean;
		startupAt: number;
		boundary: boolean;
		now: Date;
	}
) {
	const now = input.now;
	if (!isOwner() || !input.trusted || input.boundary || Date.now() - input.statusAt > 30000) return;
	const [rule] = await env.db
		.select()
		.from(skillBalanceRules)
		.where(eq(skillBalanceRules.serverId, server.id));
	if (!rule?.enabled) return;
	const [round] = await env.db
		.select()
		.from(matches)
		.where(and(eq(matches.serverId, server.id), isNull(matches.endedAt)))
		.orderBy(desc(matches.id))
		.limit(1);
	const validStatus = (s: Status) =>
		!!round?.map &&
		mapId(s.map) === mapId(round.map) &&
		s.matchSeconds !== null &&
		Number.isFinite(s.matchSeconds) &&
		s.matchSeconds >= rule.graceSeconds &&
		!(s.scoreCap !== null && s.scores.some((t) => t.score >= s.scoreCap!));
	if (
		!round ||
		!validStatus(input.status) ||
		now.getTime() - Math.max(round.startedAt.getTime(), input.startupAt) < rule.graceSeconds * 1000
	)
		return;
	const id = `skill-balance:${server.id}:${round.id}`;
	if (
		(
			await env.db
				.select({ id: skillBalanceRuns.id })
				.from(skillBalanceRuns)
				.where(eq(skillBalanceRuns.id, id))
		).length
	)
		return;
	const clock = input.status.matchSeconds!;
	const scope = and(
		eq(kills.serverId, server.id),
		eq(kills.matchRow, round.id),
		gte(kills.ts, round.startedAt)
	);
	// Aggregate accepted Feed events, including vehicles/weapons. Unknown infantry classification does not become zero.
	const [[coverage], killStats, deathStats, sessions, permits] = await Promise.all([
		env.db
			.select({
				first: sql<number>`min(${kills.eventTime})`,
				last: sql<number>`max(${kills.eventTime})`,
				received: sql<Date>`max(${kills.ts})`
			})
			.from(kills)
			.where(scope),
		env.db
			.select({
				steamId: kills.killerSteamId,
				total: sql<number>`count(distinct (${kills.instanceId},${kills.eventId}))`,
				recent: sql<number>`count(distinct (${kills.instanceId},${kills.eventId})) filter (where ${kills.eventTime}>${clock - 180})`
			})
			.from(kills)
			.where(
				and(
					scope,
					sql`${kills.eventTime}<=${clock}`,
					eq(kills.suicide, false),
					eq(kills.teamKill, false),
					sql`${kills.killerSteamId}<>${kills.victimSteamId}`
				)
			)
			.groupBy(kills.killerSteamId),
		env.db
			.select({
				steamId: kills.victimSteamId,
				total: sql<number>`count(distinct (${kills.instanceId},${kills.eventId}))`
			})
			.from(kills)
			.where(and(scope, sql`${kills.eventTime}<=${clock}`))
			.groupBy(kills.victimSteamId),
		env.db
			.select()
			.from(playerSessions)
			.where(
				and(
					eq(playerSessions.serverId, server.id),
					isNull(playerSessions.leftAt),
					lt(playerSessions.joinedAt, new Date(now.getTime() - 180000))
				)
			),
		env.db
			.select()
			.from(factionMovePermits)
			.where(
				and(eq(factionMovePermits.serverId, server.id), gte(factionMovePermits.expiresAt, now))
			)
	]);
	if (
		coverage?.first === null ||
		coverage?.last === null ||
		!coverage?.received ||
		Number(coverage.first) > clock - 180 ||
		Number(coverage.last) > clock + 5 ||
		clock - Number(coverage.last) > 60 ||
		now.getTime() - new Date(coverage.received).getTime() > 60000
	)
		return;
	const ranked = input.players
		.filter(
			(p) =>
				sessions.some((s) => s.steamId === p.steamId && s.faction === p.faction) &&
				!permits.some((m) => m.steamId === p.steamId)
		)
		.map((p) => {
			const k = killStats.find((k) => k.steamId === p.steamId),
				deaths = Number(deathStats.find((d) => d.steamId === p.steamId)?.total ?? 0),
				total = Number(k?.total ?? 0);
			return {
				...p,
				kills: total,
				deaths,
				kpm: Number(k?.recent ?? 0) / 3,
				kd: total / Math.max(1, deaths)
			};
		});
	const plan = planSkillBalance(input.status, ranked, rule.leadPoints);
	if (!plan) return;
	const claimed = await withOwnedTransaction(env, async (tx) => {
		const [current] = await tx
			.select()
			.from(skillBalanceRules)
			.where(eq(skillBalanceRules.serverId, server.id))
			.for('share');
		if (!current?.enabled || current.updatedAt.getTime() !== rule.updatedAt.getTime()) return false;
		const rows = await tx
			.insert(skillBalanceRuns)
			.values({
				id,
				serverId: server.id,
				matchId: round.id,
				state: 'executing',
				reason: '已锁定三对玩家，开始逐对调队',
				plan,
				moves: []
			})
			.onConflictDoNothing()
			.returning();
		return rows.length === 1;
	});
	if (!claimed) return;
	const moves: BalanceMove[] = [];
	const save = async (state: string, reason: string) => {
		await env.db
			.update(skillBalanceRuns)
			.set({ state, reason, moves: [...moves], updatedAt: new Date() })
			.where(eq(skillBalanceRuns.id, id));
	};
	const roster = async () =>
		((await ACTIONS.players.run(client, {})) as { players: Player[] }).players;
	const preflight = async () => {
		if (!isOwner()) throw new Error('Worker 控制权已转移，停止调队');
		const [current] = await env.db
			.select()
			.from(skillBalanceRules)
			.where(eq(skillBalanceRules.serverId, server.id));
		const [active] = await env.db.select().from(matches).where(eq(matches.id, round.id));
		const status = (await ACTIONS.status.run(client, {})) as Status;
		if (
			!current?.enabled ||
			current.updatedAt.getTime() !== rule.updatedAt.getTime() ||
			active?.endedAt ||
			!validStatus(status) ||
			status.matchSeconds! < clock - 5
		)
			throw new Error('设置、地图或对局状态改变，停止调队');
		const still = planSkillBalance(status, ranked, rule.leadPoints);
		if (!still || still.strong !== plan.strong || still.weak !== plan.weak)
			throw new Error('阵营领先关系或分差改变，停止调队');
	};
	const move = async (p: Player, to: string) => {
		const entry: BalanceMove = { steamId: p.steamId, from: p.faction!, to, state: 'sending' };
		moves.push(entry);
		await save('executing', '正在调队；发送中断不会自动重复');
		await withOwnedTransaction(env, (tx) =>
			tx.insert(factionMovePermits).values({
				serverId: server.id,
				steamId: p.steamId,
				faction: to,
				expiresAt: new Date(Date.now() + 120000)
			})
		);
		try {
			await ACTIONS.changeTeam.run(client, { steamId: p.steamId, faction: to });
			const actual = (await roster()).find((q) => q.steamId === p.steamId);
			if (actual?.faction !== to) throw new Error('调队后未能确认目标阵营，停止后续操作');
			entry.state = 'confirmed';
			await save('executing', '调队已确认');
		} catch (e) {
			entry.state = e instanceof GameError ? 'failed' : 'unknown';
			entry.reason = e instanceof Error ? e.message : '调队失败';
			await save('partial', entry.reason);
			throw e;
		}
	};
	let completed = 0;
	try {
		for (const pair of plan.pairs) {
			await preflight();
			const fresh = await roster();
			const a = fresh.find((p) => p.steamId === pair.strong.steamId),
				b = fresh.find((p) => p.steamId === pair.weak.steamId);
			if (a?.faction !== plan.strong || b?.faction !== plan.weak)
				throw new Error('候选玩家离线或已换边，停止本轮平衡');
			// No spectator staging and no forced disconnect when a destination is full.
			await move(b, plan.strong);
			try {
				await preflight();
				await move(a, plan.weak);
			} catch (e) {
				// Compensate only when a fresh roster proves the second move did NOT happen.
				// A timeout/unknown result is never blindly retried.
				const status = (await ACTIONS.status.run(client, {})) as Status;
				const current = await roster();
				if (
					isOwner() &&
					validStatus(status) &&
					status.matchSeconds! >= clock - 5 &&
					current.find((p) => p.steamId === a.steamId)?.faction === plan.strong &&
					current.find((p) => p.steamId === b.steamId)?.faction === plan.strong
				) {
					await move({ ...b, faction: plan.strong }, plan.weak);
				}
				throw e;
			}
			completed++;
		}
		await save('done', '已完成3对玩家交换（共6人）');
	} catch (e) {
		await save(
			completed || moves.some((m) => m.state === 'confirmed') ? 'partial' : 'error',
			e instanceof Error ? e.message : '平衡失败'
		);
	}
	const [result] = await env.db.select().from(skillBalanceRuns).where(eq(skillBalanceRuns.id, id));
	await writeAudit(env, null, {
		server,
		orgId: server.orgId,
		category: 'trigger',
		action: 'skill_balance.' + result.state,
		actorName: '强弱阵营平衡',
		outcome: result.state === 'done' ? 'ok' : 'error',
		message: result.reason,
		detail: { runId: id, plan, moves, completedPairs: completed }
	});
}
