import { sql } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import type { Env } from '../env';
import { ApiError } from '../http';
import { qqPolicy } from './config';

/** Called within the worker's fenced observation transaction. No credit across untrusted gaps. */
export async function creditWarmth(
	db: DbOrTx,
	serverId: string,
	at: Date,
	steamIds: string[],
	gapMs: number,
	playerCount: number,
	trusted: boolean
) {
	const policy = qqPolicy(serverId);
	const ids = [...new Set(steamIds.filter((id) => /^\d{17}$/.test(id)))].sort();
	if (
		!policy ||
		!trusted ||
		!ids.length ||
		playerCount > policy.lowAt ||
		!Number.isFinite(gapMs) ||
		gapMs <= 0 ||
		gapMs > 60000
	)
		return;
	const inserted =
		await db.execute(sql`INSERT INTO qq_warm_ticks(server_id, observed_at) VALUES(${serverId}, ${at})
	 ON CONFLICT(server_id) DO UPDATE SET observed_at = excluded.observed_at
	 WHERE qq_warm_ticks.observed_at < excluded.observed_at RETURNING server_id`);
	if (!inserted.length) return;
	await db.execute(sql`INSERT INTO qq_wallets(server_id, steam_id)
	 SELECT ${serverId}, value FROM jsonb_array_elements_text((${JSON.stringify(ids)}::text)::jsonb) ORDER BY value ON CONFLICT DO NOTHING`);
	await db.execute(sql`WITH previous AS (
	 SELECT steam_id, balance, warm_ms FROM qq_wallets WHERE server_id = ${serverId} AND steam_id IN ${ids} ORDER BY steam_id FOR UPDATE
	), changed AS (
	 UPDATE qq_wallets w SET warm_ms = p.warm_ms + ${Math.floor(gapMs)},
	 balance = p.balance + (((p.warm_ms + ${Math.floor(gapMs)}) / 60000 - p.warm_ms / 60000) * ${policy.pointsPerMinute})
	 FROM previous p WHERE w.server_id = ${serverId} AND w.steam_id = p.steam_id
	 RETURNING w.steam_id, w.balance - p.balance AS delta
	) INSERT INTO qq_ledger(id, server_id, steam_id, delta, reason)
	 SELECT ${`warm:${serverId}:${at.toISOString()}:`} || steam_id, ${serverId}, steam_id, delta, '暖服时长' FROM changed WHERE delta > 0`);
}

export async function debit(
	db: DbOrTx,
	serverId: string,
	steamId: string,
	cost: number,
	id: string,
	reason: string
) {
	if (!Number.isSafeInteger(cost) || cost <= 0) throw new ApiError(400, '积分价格无效。');
	const rows = await db.execute(
		sql`UPDATE qq_wallets SET balance = balance - ${cost} WHERE server_id = ${serverId} AND steam_id = ${steamId} AND balance >= ${cost} RETURNING balance`
	);
	if (!rows.length) throw new ApiError(409, '积分不足，请先暖服。', 'insufficient_points');
	await db.execute(
		sql`INSERT INTO qq_ledger(id,server_id,steam_id,delta,reason) VALUES(${id},${serverId},${steamId},${-cost},${reason})`
	);
}

export async function wallet(env: Env, serverId: string, steamId: string) {
	const [row] = await env.db.execute<{ balance: string; warm_ms: string }>(
		sql`SELECT balance, warm_ms FROM qq_wallets WHERE server_id=${serverId} AND steam_id=${steamId}`
	);
	return {
		balance: Number(row?.balance || 0),
		warmMinutes: Math.floor(Number(row?.warm_ms || 0) / 60000)
	};
}

/** Explicit operator reconciliation only. An uncertain network result must never trigger an automatic refund. */
export async function reconcileOrder(
	env: Env,
	serverId: string,
	id: string,
	refund: boolean,
	actor: string
) {
	return env.db.transaction(async (tx) => {
		const [order] = await tx.execute<{
			steam_id: string;
			cost: string;
			state: string;
			kind: string;
		}>(
			sql`SELECT steam_id,cost,state,kind FROM qq_orders WHERE id=${id} AND server_id=${serverId} FOR UPDATE`
		);
		if (!order || !['unknown', 'failed', 'partial'].includes(order.state))
			throw new ApiError(409, '订单不在可人工处理状态。');
		if (refund && Number(order.cost) > 0) {
			await tx.execute(
				sql`UPDATE qq_wallets SET balance=balance+${Number(order.cost)} WHERE server_id=${serverId} AND steam_id=${order.steam_id}`
			);
			await tx.execute(
				sql`INSERT INTO qq_ledger(id,server_id,steam_id,delta,reason) VALUES(${`refund:${id}`},${serverId},${order.steam_id},${Number(order.cost)},${`人工退款:${actor}`})`
			);
		}
		if (refund && order.kind === 'map') {
			const voteId = id.slice('map:'.length);
			const ballots = await tx.execute(
				sql`SELECT l.steam_id,-l.delta AS amount FROM qq_ledger l JOIN qq_ballots b ON b.steam_id=l.steam_id AND b.vote_id=${voteId} WHERE l.server_id=${serverId} AND l.id=('vote:' || ${voteId} || ':' || b.steam_id) ORDER BY l.steam_id`
			);
			for (const ballot of ballots) {
				await tx.execute(
					sql`UPDATE qq_wallets SET balance=balance+${Number(ballot.amount)} WHERE server_id=${serverId} AND steam_id=${ballot.steam_id}`
				);
				await tx.execute(
					sql`INSERT INTO qq_ledger(id,server_id,steam_id,delta,reason) VALUES(${`refund:${id}:${ballot.steam_id}`},${serverId},${ballot.steam_id},${Number(ballot.amount)},${`地图投票退款:${actor}`})`
				);
			}
		}
		await tx.execute(
			sql`UPDATE qq_orders SET state=${refund ? 'refunded' : 'done'}, outcome=${`人工核实:${actor}`} WHERE id=${id}`
		);
	});
}
