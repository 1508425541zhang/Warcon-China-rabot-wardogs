import { sql } from 'drizzle-orm';
import { randomUUID } from 'node:crypto';
import type { Env } from '../env';
import { ApiError } from '../http';
import { getOrg, getServer, type ServerRow } from '../access';
import { gateway } from '../gateway';
import { loadCareer, lastNameOf } from '../leaderboards';
import { resolveReportTarget, submitReport } from '../integrity/reports';
import { qqPolicies, qqPolicy, type QqPolicy } from './config';
import { createLinkCode, linkedAccount } from './identity';
import { debit, wallet } from './economy';
import { friendlyTargets, parseCommand, type RosterPlayer } from './protocol';

export async function communityServer(env: Env, id: string) {
	const server = await getServer(env, id);
	const org = server ? await getOrg(env, server.orgId) : null;
	if (!server || !org || org.suspendedAt || !qqPolicy(id))
		throw new ApiError(404, '服务器未启用 QQ 社区服务。');
	return { server, org };
}

export async function playerSummary(env: Env, server: ServerRow, target: string) {
	let steamId = target;
	if (!/^\d{17}$/.test(target)) {
		if (target.length < 2 || target.length > 100)
			throw new ApiError(400, '请输入 SteamID64 或至少两个字的玩家名。');
		const candidates = await env.db.execute<{ steamId: string; name: string }>(sql`
		 SELECT DISTINCT ON (steam_id) steam_id AS "steamId", name FROM player_sessions
		 WHERE server_id=${server.id} AND position(lower(${target}) in lower(name))>0 ORDER BY steam_id,last_seen DESC LIMIT 51`);
		steamId = resolveReportTarget(
			target,
			candidates as { steamId: string; name: string }[]
		).steamId;
	}
	const name = await lastNameOf(env, [server.id], steamId);
	if (!name) throw new ApiError(404, '本服没有该玩家的记录。');
	const career = await loadCareer(env, {
		serverId: server.id,
		ids: [server.id],
		nameOf: new Map([[server.id, server.name]]),
		steamId
	});
	const [span] = await env.db.execute<{ first: Date; last: Date }>(
		sql`SELECT min(joined_at) AS first,max(last_seen) AS last FROM player_sessions WHERE server_id=${server.id} AND steam_id=${steamId}`
	);
	const summary = `${name}（${steamId}）\n${server.name}已记录 ${career.matches} 场，${career.minutes} 分钟；击杀 ${career.kills} / 死亡 ${career.deaths}，${career.wins} 胜 ${career.losses} 负 ${career.draws} 平。\n最高连杀 ${career.killStreak}，爆头 ${career.headshots}。\n最近作战：\n${career.last.map((m) => `${m.startedAt.slice(0, 16).replace('T', ' ')} UTC · ${m.map} · ${m.faction || '阵营未知'} · ${m.kills}/${m.deaths} · ${m.result}`).join('\n') || '暂无可汇总比赛'}\n仅反映本服已保存数据；不推测未记录的战术、移动路线或游戏生涯。`;
	const timelineRows = await env.db.execute(
		sql`SELECT ts,map,killer_steam_id,killer_name,victim_name,cause,headshot,suicide,team_kill FROM kills WHERE server_id=${server.id} AND (killer_steam_id=${steamId} OR victim_steam_id=${steamId}) ORDER BY ts DESC LIMIT 12`
	);
	const timeline = timelineRows.reverse().map((event) => ({
		at: new Date(event.ts).toISOString(),
		map: String(event.map),
		action: event.suicide ? '自杀' : event.killer_steam_id === steamId ? '击杀' : '阵亡',
		other: event.killer_steam_id === steamId ? event.victim_name : event.killer_name || '环境',
		weapon: event.cause,
		headshot: event.headshot,
		teamKill: event.team_kill
	}));
	const processText = timeline.length
		? `\n最近已记录交战（按时间顺序，UTC）：\n${timeline
				.slice(-6)
				.map(
					(event) =>
						`${event.at.slice(5, 19).replace('T', ' ')} ${event.map} ${event.action} ${event.other}${event.weapon ? `，${event.weapon}` : ''}${event.headshot ? '，爆头' : ''}${event.teamKill ? '，友伤' : ''}`
				)
				.join('\n')}`
		: '\n没有可用的击杀回传，无法还原交战时间线。';
	return {
		steamId,
		name,
		career,
		firstSeen: span?.first,
		lastSeen: span?.last,
		timeline,
		summary: summary + processText
	};
}

export async function openVote(env: Env, policy: QqPolicy, server: ServerRow) {
	const caps = (await gateway().run(env, server, 'capabilities', {})) as {
		features: { rotationEdit: boolean };
	};
	if (!caps.features.rotationEdit)
		throw new ApiError(409, '当前游戏版本没有实时地图轮换接口，不能开启付费投票。');
	const catalog = (await gateway().run(env, server, 'maps', {})) as { maps: { id: string }[] };
	if (policy.maps.some((map) => !catalog.maps.some((m) => m.id === map)))
		throw new ApiError(409, '候选地图配置与游戏目录不一致，请管理员更新。');
	return env.db.transaction(async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${`qq-vote:${server.id}`},0))`
		);
		const [active] = await tx.execute<{ id: string }>(
			sql`SELECT id FROM qq_votes WHERE server_id=${server.id} AND (state='open' OR ends_at > now()-interval '5 minutes') LIMIT 1`
		);
		if (active) throw new ApiError(409, '本服已有投票或仍在五分钟冷却期，请发送 /投票 查看。');
		const id = randomUUID();
		await tx.execute(
			sql`INSERT INTO qq_votes(id,server_id,maps,ends_at) VALUES(${id},${server.id},(${JSON.stringify(policy.maps)}::text)::jsonb,now()+(${policy.voteSeconds} * interval '1 second'))`
		);
		return id;
	});
}

export async function castVote(env: Env, policy: QqPolicy, steamId: string, choice: string) {
	return env.db.transaction(async (tx) => {
		const [vote] = await tx.execute<{ id: string; maps: string[] }>(
			sql`SELECT id,maps FROM qq_votes WHERE server_id=${policy.serverId} AND state='open' AND ends_at>now() FOR UPDATE`
		);
		if (!vote) throw new ApiError(409, '没有进行中的投票，请先发送 /发起投票。');
		const map = /^\d+$/.test(choice) ? vote.maps[Number(choice) - 1] : choice;
		if (!vote.maps.includes(map)) throw new ApiError(400, '请选择候选地图编号或完整地图 ID。');
		const prior = await tx.execute(
			sql`SELECT 1 FROM qq_ballots WHERE vote_id=${vote.id} AND steam_id=${steamId}`
		);
		if (prior.length) return '你已经投过票，本轮每个 Steam 账号限投一次，不会重复扣分。';
		await debit(
			tx,
			policy.serverId,
			steamId,
			policy.voteCost,
			`vote:${vote.id}:${steamId}`,
			'下一张地图投票'
		);
		await tx.execute(
			sql`INSERT INTO qq_ballots(vote_id,steam_id,map) VALUES(${vote.id},${steamId},${map})`
		);
		return `投票成功：${map}，消耗 ${policy.voteCost} 积分。`;
	});
}

export async function voteStatus(env: Env, serverId: string) {
	const [vote] = await env.db.execute<{
		id: string;
		maps: string[];
		state: string;
		winner: string | null;
		ends_at: Date;
	}>(sql`SELECT * FROM qq_votes WHERE server_id=${serverId} ORDER BY ends_at DESC LIMIT 1`);
	if (!vote) return '暂无投票，发送 /发起投票 开启。';
	const ballots = await env.db.execute<{ map: string; n: string }>(
		sql`SELECT map,count(*) AS n FROM qq_ballots WHERE vote_id=${vote.id} GROUP BY map`
	);
	return `地图投票 ${vote.state}，截止 ${new Date(vote.ends_at).toISOString()}\n${(vote.maps as string[]).map((m, i) => `${i + 1}. ${m}：${ballots.find((b) => b.map === m)?.n || 0} 票`).join('\n')}\n${vote.winner ? `胜出：${vote.winner}；用 /订单 查询设置结果。` : '发送 /投票 编号；平票按候选列表顺序决定。'}`;
}

export async function createPurchase(
	env: Env,
	policy: QqPolicy,
	steamId: string,
	id: string,
	kind: 'friendly' | 'reserve',
	message = ''
) {
	const { server } = await communityServer(env, policy.serverId);
	const [existing] = await env.db.execute(
		sql`SELECT server_id,steam_id,kind,params FROM qq_orders WHERE id=${id}`
	);
	if (existing) {
		if (
			existing.server_id !== server.id ||
			existing.steam_id !== steamId ||
			existing.kind !== kind ||
			(kind === 'friendly' && existing.params.message !== `[友方广播] ${message}`)
		)
			throw new ApiError(409, 'requestId 已用于不同的请求。');
		return id;
	}
	let params: Record<string, unknown>;
	let recipients: RosterPlayer[] = [];
	if (kind === 'friendly') {
		if (!message || message.length > 180 || /[\x00-\x1f<>]/.test(message))
			throw new ApiError(400, '广播正文须为 1–180 字，不能含控制字符或富文本标签。');
		const { players } = (await gateway().run(env, server, 'players', {})) as {
			players: RosterPlayer[];
		};
		try {
			recipients = friendlyTargets(players, steamId);
		} catch {
			throw new ApiError(409, '你须在本服在线且已加入阵营。');
		}
		params = { message: `[友方广播] ${message}`, faction: recipients[0].faction };
	} else {
		const live = (await gateway().run(env, server, 'reserved', {})) as { reserved: string[] };
		if (live.reserved?.includes(steamId))
			throw new ApiError(409, '你已在游戏预留名单中，无需重复兑换。');
		const held = await env.db.execute(
			sql`SELECT 1 FROM list_entries e JOIN lists l ON l.id=e.list_id WHERE l.kind='reserve' AND l.org_id=${server.orgId} AND (l.server_id=${server.id} OR l.server_id IS NULL) AND e.steam_id=${steamId} AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()) LIMIT 1`
		);
		if (held.length) throw new ApiError(409, '你已有有效预留位，无需重复兑换。');
		params = { hours: policy.reserveHours };
	}
	const cost = kind === 'friendly' ? policy.broadcastCost : policy.reserveCost;
	return env.db.transaction(async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${`qq-purchase:${policy.serverId}:${steamId}`},0))`
		);
		const [prior] = await tx.execute(sql`SELECT id,kind,params FROM qq_orders WHERE id=${id}`);
		if (prior) {
			if (prior.kind !== kind || (kind === 'friendly' && prior.params.message !== params.message))
				throw new ApiError(409, 'requestId 已用于不同的请求。');
			return prior.id;
		}
		const recent = await tx.execute(
			sql`SELECT 1 FROM qq_orders WHERE server_id=${policy.serverId} AND steam_id=${steamId} AND kind=${kind} AND (created_at>now()-interval '1 minute' OR state IN ('pending','processing','unknown')) LIMIT 1`
		);
		if (recent.length) throw new ApiError(429, '请先查看已有订单结果，或等待一分钟冷却。');
		await debit(
			tx,
			policy.serverId,
			steamId,
			cost,
			`buy:${id}`,
			kind === 'friendly' ? '友方逐人广播' : '限时预留位'
		);
		await tx.execute(
			sql`INSERT INTO qq_orders(id,server_id,steam_id,kind,params,cost) VALUES(${id},${policy.serverId},${steamId},${kind},(${JSON.stringify(params)}::text)::jsonb,${cost})`
		);
		for (const recipient of recipients)
			await tx.execute(
				sql`INSERT INTO qq_deliveries(order_id,steam_id) VALUES(${id},${recipient.steamId})`
			);
		return id;
	});
}

export async function command(
	env: Env,
	message: { id: string; server_id: string; member_id: string; content: string }
) {
	const policy = qqPolicy(message.server_id);
	if (!policy) throw new ApiError(404, '本群未配置服务器。');
	const { server } = await communityServer(env, policy.serverId);
	const { name, arg } = parseCommand(message.content);
	if (name === '帮助')
		return `/绑定：验证 Steam\n/战绩 [玩家名或SteamID]，/总结 [玩家]，/举报 SteamID 原因\n/积分：余额；暖服人数≤${policy.lowAt}，每分钟${policy.pointsPerMinute}积分\n/发起投票，/投票 [编号]（${policy.voteCost}积分/票）\n/友方广播 正文（${policy.broadcastCost}积分，按同阵营逐人发送）\n/优先队列（${policy.reserveCost}积分兑换${policy.reserveHours}小时预留位）\n/订单：发送与兑换结果；/解绑：打开网页解绑`;
	if (name === '绑定')
		return `请先复制此绑定码：${await createLinkCode(env, server.id, message.member_id)}\n在 WARCON 网页的 /qq-link 页面输入，五分钟有效。仅在你本人触发绑定后操作。`;
	if (name === '解绑')
		return '请登录 WARCON 网页 /qq-link，解绑当前账号。积分保留在 Steam 账号下。';
	if (name === '投票' && !arg) return voteStatus(env, server.id);
	if (['战绩', '总结'].includes(name) && arg)
		return (await playerSummary(env, server, arg)).summary;
	const linked = await linkedAccount(env, server.id, message.member_id);
	if (['战绩', '总结', '个人战绩'].includes(name))
		return (await playerSummary(env, server, arg || linked.steamId)).summary;
	if (name === '积分') {
		const w = await wallet(env, server.id, linked.steamId);
		return `积分 ${w.balance}；累计有效暖服 ${w.warmMinutes} 分钟。`;
	}
	if (name === '举报') {
		const match = /^(\d{17})\s+(.{3,300})$/s.exec(arg);
		if (!match) throw new ApiError(400, '格式：/举报 SteamID64 至少三个字的原因');
		const report = await submitReport(
			env,
			new Request(`${env.ORIGIN}/api/qq/webhook`),
			linked.actor,
			{ serverId: server.id, target: match[1], reason: `[QQ群举报] ${match[2]}`.slice(0, 300) },
			server.id
		);
		return `举报 #${report.id} 已进入人工审核并采集前后证据；举报不会直接封禁玩家。`;
	}
	if (name === '发起投票') {
		await openVote(env, policy, server);
		return voteStatus(env, server.id);
	}
	if (name === '投票') return castVote(env, policy, linked.steamId, arg);
	if (name === '友方广播' || name === '优先队列') {
		const id = await createPurchase(
			env,
			policy,
			linked.steamId,
			`qq:${message.id}`,
			name === '友方广播' ? 'friendly' : 'reserve',
			arg
		);
		return `订单 ${id} 已受理。发送 /订单 查看结果。预留位按游戏支持的机制生效，不保证改变排队算法。`;
	}
	if (name === '订单') {
		const rows = await env.db.execute<{
			id: string;
			kind: string;
			state: string;
			outcome: string | null;
		}>(
			sql`SELECT id,kind,state,outcome FROM qq_orders WHERE server_id=${server.id} AND (steam_id=${linked.steamId} OR kind='map') ORDER BY created_at DESC LIMIT 5`
		);
		return (
			rows.map((o) => `${o.id}\n${o.kind} · ${o.state} · ${o.outcome || '等待处理'}`).join('\n') ||
			'暂无订单。'
		);
	}
	return '未知指令，请发送 /帮助。';
}

export async function closeVotes(env: Env) {
	for (const policy of qqPolicies())
		await env.db.transaction(async (tx) => {
			const [vote] = await tx.execute<{ id: string; maps: string[]; ends_at: Date }>(
				sql`SELECT id,maps,ends_at FROM qq_votes WHERE server_id=${policy.serverId} AND state='open' AND ends_at<=now() FOR UPDATE SKIP LOCKED`
			);
			if (!vote) return;
			const counts = await tx.execute<{ map: string; n: string }>(
				sql`SELECT map,count(*) AS n FROM qq_ballots WHERE vote_id=${vote.id} GROUP BY map`
			);
			const ranked = (vote.maps as string[])
				.map((map, index) => ({ map, index, n: Number(counts.find((c) => c.map === map)?.n || 0) }))
				.sort((a, b) => b.n - a.n || a.index - b.index);
			const winner = ranked[0]?.n ? ranked[0].map : null;
			await tx.execute(
				sql`UPDATE qq_votes SET state='closed',winner=${winner} WHERE id=${vote.id}`
			);
			if (winner)
				await tx.execute(
					sql`INSERT INTO qq_orders(id,server_id,steam_id,kind,params,cost,created_at) VALUES(${`map:${vote.id}`},${policy.serverId},'','map',(${JSON.stringify({ map: winner })}::text)::jsonb,0,${vote.ends_at})`
				);
		});
}
