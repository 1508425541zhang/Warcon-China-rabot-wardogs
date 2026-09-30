import { vipFor } from '$lib/server/qq/vip';
import { sql } from 'drizzle-orm';
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, param, readJson, route } from '$lib/server/http';
import { requireServerCap } from '$lib/server/access';
import { requireSteamId } from '$lib/server/steam';
import { qqPolicy } from '$lib/server/qq/config';
import {
	communityQuery,
	createPurchase,
	playerSummary,
	castVote,
	openVote,
	voteStatus
} from '$lib/server/qq/community';
import { reconcileOrder, wallet } from '$lib/server/qq/economy';
import { writeAudit } from '$lib/server/audit';

export const GET = route(async (event) => {
	const env = getEnv();
	const id = param(event, 'id');
	const mode = event.url.searchParams.get('view') || 'player';
	if (!['player', 'vote', 'economy', 'status', 'players', 'maps', 'vip', 'battle'].includes(mode))
		throw new ApiError(400, 'Unknown query view.');
	const { server } = await requireServerCap(
		env,
		event.locals,
		id,
		['player', 'status', 'players', 'maps', 'vip', 'battle'].includes(mode)
			? 'server.view'
			: 'automation.manage'
	);
	if (['status', 'players', 'maps', 'battle'].includes(mode)) {
		const page = event.url.searchParams.get('page') || '1';
		if (!/^[1-9]\d{0,2}$/.test(page)) throw new ApiError(400, 'page must be 1–999.');
		return apiJson({ ok: true, ...(await communityQuery(env, server, mode, Number(page))) });
	}
	if (mode === 'vip') {
		const steamId = requireSteamId(event.url.searchParams.get('steamId'));
		const vip = await vipFor(env, id, steamId);
		return apiJson({
			ok: true,
			steamId,
			vip: !!vip,
			reserve: vip?.reserve || false,
			allowOverkill: vip?.allowOverkill || false,
			whitelist: vip?.whitelist || false
		});
	}
	if (mode === 'vote') return apiJson({ ok: true, text: await voteStatus(env, id) });
	if (mode === 'player')
		return apiJson({
			ok: true,
			...(await playerSummary(env, server, event.url.searchParams.get('target') || ''))
		});
	const steamId = requireSteamId(event.url.searchParams.get('steamId'));
	const [ledger, orders, deliveries] = await Promise.all([
		env.db.execute(
			sql`SELECT id,delta,reason,created_at FROM qq_ledger WHERE server_id=${id} AND steam_id=${steamId} ORDER BY created_at DESC LIMIT 100`
		),
		env.db.execute(
			sql`SELECT id,kind,cost,state,outcome,created_at FROM qq_orders WHERE server_id=${id} AND (steam_id=${steamId} OR kind='map') ORDER BY created_at DESC LIMIT 30`
		),
		env.db.execute(
			sql`SELECT d.* FROM qq_deliveries d JOIN qq_orders o ON o.id=d.order_id WHERE o.server_id=${id} AND o.steam_id=${steamId} ORDER BY o.created_at DESC LIMIT 500`
		)
	]);
	return apiJson({ ok: true, wallet: await wallet(env, id, steamId), ledger, orders, deliveries });
});

export const POST = route(async (event) => {
	const env = getEnv();
	const id = param(event, 'id');
	const { server, user } = await requireServerCap(env, event.locals, id, 'automation.manage');
	const body = await readJson<Record<string, unknown>>(event.request);
	const policy = qqPolicy(id);
	if (!policy) throw new ApiError(404, 'QQ 社区服务未启用。');
	let result: unknown;
	if (body.action === 'reconcile') {
		if (typeof body.refund !== 'boolean' || typeof body.orderId !== 'string')
			throw new ApiError(400, 'orderId and refund are required.');
		await reconcileOrder(env, id, body.orderId, body.refund, user.id);
		result = { reconciled: true };
	} else if (body.action === 'openVote') {
		await requireServerCap(env, event.locals, id, 'match.control');
		result = { voteId: await openVote(env, policy, server) };
	} else if (body.action === 'vote') {
		await requireServerCap(env, event.locals, id, 'match.control');
		result = {
			message: await castVote(env, policy, requireSteamId(body.steamId), String(body.choice || ''))
		};
	} else if (body.action === 'friendly' || body.action === 'reserve') {
		await requireServerCap(
			env,
			event.locals,
			id,
			body.action === 'friendly' ? 'chat.send' : 'slots.manage'
		);
		if (typeof body.requestId !== 'string' || !/^[A-Za-z0-9_-]{16,80}$/.test(body.requestId))
			throw new ApiError(400, 'requestId must contain 16–80 letters, digits, _ or -.');
		const steamId = requireSteamId(body.steamId);
		result = {
			orderId: await createPurchase(
				env,
				policy,
				steamId,
				`api:${id}:${steamId}:${body.requestId}`,
				body.action,
				String(body.message || '')
			)
		};
	} else throw new ApiError(400, 'Unknown community action.');
	await writeAudit(env, event.request, {
		actor: user,
		orgId: server.orgId,
		server: { id, name: server.name },
		category: 'server',
		action: `qq.${body.action}`,
		target: String(body.orderId || body.steamId || id),
		outcome: 'ok',
		message: 'QQ community API action',
		detail: result
	});
	return apiJson({ ok: true, result });
});
