import { sql } from 'drizzle-orm';
import type { Env } from '../env';
import { ApiError, publicMessage } from '../http';
import { gateway } from '../gateway';
import { addServerEntry } from '../lists';
import { qqCredentials, qqPolicy } from './config';
import { loadQqSettings } from './settings';
import { closeVotes, command, communityServer } from './community';
import { QqClient, type QqTransport, type RosterPlayer } from './protocol';
import { OfficialQqClient, OfficialGateway, validQqIdentity } from './official';
import { acceptOfficialEvent } from './official-inbox';
import { processIntegrityNotice } from './integrity-notices';

type Order = {
	id: string;
	server_id: string;
	steam_id: string;
	kind: string;
	params: { message: string; faction: string; hours: number; map: string };
	created_at: Date;
};
type Message = {
	id: string;
	server_id: string;
	group_id: string;
	member_id: string;
	content: string;
	created_at: Date;
	reply: string | null;
};

/** Sending is durably recorded before the network call. Unknown outcomes are never resent. */
export async function deliverOrder(env: Env, order: Order) {
	const { server, org } = await communityServer(env, order.server_id);
	if (order.kind === 'friendly') {
		const targets = await env.db.execute<{ steam_id: string }>(
			sql`SELECT steam_id FROM qq_deliveries WHERE order_id=${order.id} AND state='pending' ORDER BY steam_id`
		);
		for (const target of targets) {
			// Recheck before each recipient, so leaving or switching sides cannot disclose team messages.
			const { players } = (await gateway().run(env, server, 'players', {})) as {
				players: RosterPlayer[];
			};
			const source = players.find((p) => p.steamId === order.steam_id);
			const recipient = players.find((p) => p.steamId === target.steam_id);
			if (
				source?.faction !== order.params.faction ||
				recipient?.faction !== order.params.faction ||
				Date.now() - new Date(order.created_at).getTime() > 120000
			) {
				await env.db.execute(
					sql`UPDATE qq_deliveries SET state='skipped',outcome='离线、阵营改变或广播已过期' WHERE order_id=${order.id} AND steam_id=${target.steam_id}`
				);
				continue;
			}
			await env.db.execute(
				sql`UPDATE qq_deliveries SET state='sending' WHERE order_id=${order.id} AND steam_id=${target.steam_id}`
			);
			try {
				await gateway().run(env, server, 'whisper', {
					steamId: target.steam_id,
					message: order.params.message
				});
				await env.db.execute(
					sql`UPDATE qq_deliveries SET state='done',outcome='游戏接口确认' WHERE order_id=${order.id} AND steam_id=${target.steam_id}`
				);
			} catch {
				await env.db.execute(
					sql`UPDATE qq_deliveries SET state='unknown',outcome='发送结果需人工核实，不自动重发' WHERE order_id=${order.id} AND steam_id=${target.steam_id}`
				);
			}
		}
		const result = await env.db.execute<{ state: string; n: string }>(
			sql`SELECT state,count(*) AS n FROM qq_deliveries WHERE order_id=${order.id} GROUP BY state`
		);
		const done = result.length === 1 && result[0].state === 'done';
		await env.db.execute(
			sql`UPDATE qq_orders SET state=${done ? 'done' : 'partial'},outcome=${result.map((r) => `${r.state}: ${r.n}人`).join('；')} WHERE id=${order.id}`
		);
		return;
	}
	if (order.kind === 'map') {
		if (Date.now() - new Date(order.created_at).getTime() > 120000)
			throw new ApiError(409, '投票结果已过期，未修改地图。');
		await gateway().run(env, server, 'setNextMap', { map: order.params.map });
		await env.db.execute(
			sql`UPDATE qq_orders SET state='done',outcome=${`已设置下一张地图：${order.params.map}`} WHERE id=${order.id}`
		);
		return;
	}
	if (order.kind !== 'reserve') throw new Error('Unknown community order kind.');
	// Reuse the existing desired-state list, expiry processing and audited server synchronization.
	const actor = {
		id: 'qq-economy',
		username: 'QQ积分兑换',
		name: 'QQ积分兑换',
		role: 'member' as const,
		mustChangePassword: false,
		image: null,
		defaultOrgId: server.orgId,
		authComplete: true,
		authGraceStartedAt: null
	};
	const { sync } = await addServerEntry(
		env,
		new Request(`${env.ORIGIN}/api/qq/webhook`),
		actor,
		server,
		org,
		'reserve',
		{
			steamId: order.steam_id,
			reason: `QQ积分订单 ${order.id}`,
			expiresAt: new Date(Date.now() + order.params.hours * 3600000).toISOString()
		}
	);
	await env.db.execute(
		sql`UPDATE qq_orders SET state='done',outcome=${sync.ok ? '限时预留位已同步到游戏' : '限时预留位已登记，等待 WARCON 重试同步；尚未确认游戏生效'} WHERE id=${order.id}`
	);
}

export async function processMessage(env: Env, client: QqTransport, selfId?: string) {
	const [message] = await env.db
		.execute<Message>(sql`UPDATE qq_inbox SET state='processing',started_at=now()
	 WHERE id=(SELECT id FROM qq_inbox WHERE state='pending' ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING *`);
	if (!message) return;
	if (
		!(message.id.startsWith('ob11:') || message.id.startsWith('official:')) ||
		(selfId !== undefined && !message.id.startsWith(`${client instanceof OfficialQqClient ? 'official' : 'ob11'}:${selfId}:`)) ||
		!validQqIdentity(message.member_id) ||
		Date.now() - new Date(message.created_at).getTime() > 240000 ||
		!qqPolicy(message.server_id)?.groups.includes(message.group_id)
	) {
		await env.db.execute(
			sql`UPDATE qq_inbox SET state='done',reply_state='expired',reply='指令已过期或群授权已撤销，未执行业务操作' WHERE id=${message.id}`
		);
		return;
	}
	let reply: string;
	try {
		reply = await command(env, message as Message);
	} catch (error) {
		reply = publicMessage(error, '服务暂时不可用，请稍后查看订单或联系管理员。');
	}
	await env.db.execute(
		sql`UPDATE qq_inbox SET state='done',reply=${reply},reply_state='sending' WHERE id=${message.id}`
	);
	// Do not deliver stale command results after a long outage.
	if (Date.now() - new Date(message.created_at).getTime() > 240000) {
		await env.db.execute(sql`UPDATE qq_inbox SET reply_state='expired' WHERE id=${message.id}`);
		return;
	}
	try {
		await client.reply(message.group_id, message.id, reply);
		await env.db.execute(sql`UPDATE qq_inbox SET reply_state='done' WHERE id=${message.id}`);
	} catch {
		await env.db.execute(sql`UPDATE qq_inbox SET reply_state='unknown' WHERE id=${message.id}`);
	}
}

declare global {
	var __warconQq: ReturnType<typeof setInterval> | undefined;
}
let active: Promise<void> | null = null;
let official: { fingerprint: string; client: OfficialQqClient; gateway: OfficialGateway } | null = null;

export function startQq(env: Env) {
	if (globalThis.__warconQq) clearInterval(globalThis.__warconQq);
	globalThis.__warconQq = setInterval(() => {
		if (active) return;
		active = (async () => {
			await loadQqSettings(env);
			const credentials = qqCredentials();
			if (credentials?.provider === 'official') {
				const fingerprint = credentials.selfId + ':' + credentials.secret;
				if (official?.fingerprint !== fingerprint) {
					official?.gateway.stop();
					const client = new OfficialQqClient(credentials.selfId, credentials.secret);
					const gateway = new OfficialGateway(client, (payload) => acceptOfficialEvent(env, payload, credentials.selfId));
					official = { fingerprint, client, gateway }; gateway.start();
				}
				await pass(env, official.client, credentials.selfId);
			} else {
				official?.gateway.stop(); official = null;
				if (credentials)
				await pass(env, new QqClient(credentials.url, credentials.token), credentials.selfId);
			}
		})()
			.catch((error) => {
				console.error('[qq]', publicMessage(error));
			})
			.finally(() => {
				active = null;
			});
	}, 1000);
}
export async function stopQq() {
	official?.gateway.stop(); official = null;
	if (globalThis.__warconQq) clearInterval(globalThis.__warconQq);
	globalThis.__warconQq = undefined;
	await active;
}

async function pass(env: Env, client: QqTransport, selfId: string) {
	// Cross-process advisory lock: a slow recipient loop must never race another web replica.
	const connection = await env.sql.reserve();
	try {
		const [lock] = await connection`SELECT pg_try_advisory_lock(1869754673, 1) AS held`;
		if (!lock.held) return;
		try {
			// With this lock held, any processing records came from a terminated prior worker.
			await env.db.execute(
				sql`UPDATE qq_orders SET state='unknown',outcome='服务中断，需人工核实，禁止自动重发或重复扣款' WHERE state='processing'`
			);
			await env.db.execute(
				sql`UPDATE qq_deliveries SET state='unknown',outcome='服务中断，发送结果未知' WHERE state='sending'`
			);
			await env.db.execute(
				sql`UPDATE qq_inbox SET state='done',reply_state='unknown',reply='服务中断，请查询订单；不要重复兑换' WHERE state='processing'`
			);
			await env.db.execute(
				sql`UPDATE qq_inbox SET reply_state='unknown' WHERE reply_state='sending'`
			);
			await processMessage(env, client, selfId);
			await processIntegrityNotice(env, client, selfId);
			await closeVotes(env);
			const [order] = await env.db.execute<Order>(
				sql`UPDATE qq_orders SET state='processing',started_at=now() WHERE id=(SELECT id FROM qq_orders WHERE state='pending' ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING *`
			);
			if (order)
				try {
					await deliverOrder(env, order as Order);
				} catch (error) {
					await env.db.execute(
						sql`UPDATE qq_orders SET state='unknown',outcome=${publicMessage(error, '结果待核实，请管理员检查游戏状态及订单后结算。')} WHERE id=${order.id}`
					);
				}
		} finally {
			await connection`SELECT pg_advisory_unlock(1869754673, 1)`;
		}
	} finally {
		connection.release();
	}
}
