import { and, desc, eq, gte, isNull, lt, ne, sql } from 'drizzle-orm';
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
import type { WardogsClient } from './rcon';

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
		executionAvailable: false,
		executionBlock:
			'本服RCON未提供当前存活、重生及装备状态，也没有仅死亡时生效的条件调队接口。仅生成候选名单，不执行交换，不主动杀死玩家。',
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

/** Candidate monitoring only until the game exposes an enforceable dead/spawn-safe move contract. */
export async function runSkillBalance(
	env: Env,
	server: ServerRow,
	_client: WardogsClient,
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
	if (
		!round?.map ||
		mapId(input.status.map) !== mapId(round.map) ||
		now.getTime() - Math.max(round.startedAt.getTime(), input.startupAt) < rule.graceSeconds * 1000
	)
		return;
	await env.db
		.update(skillBalanceRuns)
		.set({ state: 'cancelled', reason: '对局已结束，旧候选作废', updatedAt: now })
		.where(
			and(
				eq(skillBalanceRuns.serverId, server.id),
				ne(skillBalanceRuns.matchId, round.id),
				eq(skillBalanceRuns.state, 'waiting_safe')
			)
		);
	const id = `skill-balance:${server.id}:${round.id}`;
	const cancel = async (reason: string) => {
		await env.db
			.update(skillBalanceRuns)
			.set({ state: 'cancelled', reason, updatedAt: now })
			.where(and(eq(skillBalanceRuns.id, id), eq(skillBalanceRuns.state, 'waiting_safe')));
	};
	const [latest] = await env.db
		.select()
		.from(kills)
		.where(and(eq(kills.serverId, server.id), eq(kills.matchRow, round.id)))
		.orderBy(desc(kills.ts))
		.limit(1);
	if (!latest || now.getTime() - latest.ts.getTime() > 60000) {
		await cancel('当前对局Feed过期，候选作废');
		return;
	}
	const clock =
		input.status.matchSeconds ??
		latest.eventTime + Math.max(0, (now.getTime() - latest.ts.getTime()) / 1000);
	if (
		!Number.isFinite(clock) ||
		clock < rule.graceSeconds ||
		(input.status.scoreCap !== null &&
			input.status.scores.some((s) => s.score >= input.status.scoreCap!))
	) {
		await cancel('对局处于开局或结束保护');
		return;
	}
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
	) {
		await cancel('当前对局Feed无法覆盖完整窗口');
		return;
	}
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
	if (!plan) {
		await cancel('分差、选人或阵营条件不再满足');
		return;
	}

	// Current verified game build has no safe life-state / conditional move contract.
	// Never infer present death from a delayed Kill Feed or issue the forced-respawn action.
	await withOwnedTransaction(env, async (tx) => {
		const [current] = await tx
			.select()
			.from(skillBalanceRules)
			.where(eq(skillBalanceRules.serverId, server.id))
			.for('share');
		if (!current?.enabled || current.updatedAt.getTime() !== rule.updatedAt.getTime()) return;
		const values = {
			id,
			serverId: server.id,
			matchId: round.id,
			state: 'waiting_safe',
			reason: '已选出3对玩家；缺少可验证的死亡／重生安全调队接口，不执行交换',
			plan,
			moves: [],
			updatedAt: now
		};
		await tx
			.insert(skillBalanceRuns)
			.values(values)
			.onConflictDoUpdate({
				target: skillBalanceRuns.id,
				set: { plan, state: values.state, reason: values.reason, updatedAt: now },
				setWhere: sql`${skillBalanceRuns.state} IN ('waiting_safe','cancelled')`
			});
	});
}
