import { createHash, randomBytes } from 'node:crypto';
import { sql } from 'drizzle-orm';
import type { Env } from '../env';
import { ApiError } from '../http';
import { toSessionUser, type SessionUser } from '../access';
import { qqPolicy } from './config';
import type { RosterPlayer } from './protocol';

/** Group identity is checked by the signed webhook; player identity matches the server roster. */
export async function bindByPlayer(
	env: Env,
	serverId: string,
	memberId: string,
	steamId: string,
	name: string,
	roster: RosterPlayer[]
) {
	if (
		!qqPolicy(serverId) ||
		!/^ob11:[1-9]\d+$/.test(memberId) ||
		!/^\d{17}$/.test(steamId) ||
		!name.trim() ||
		name.length > 100
	)
		throw new ApiError(400, '格式：/绑定 SteamID64 游戏内昵称');
	const live = roster.find((player) => player.steamId === steamId);
	const [saved] = live
		? []
		: await env.db.execute<{ name: string }>(sql`
		SELECT name FROM player_sessions WHERE server_id=${serverId} AND steam_id=${steamId} ORDER BY last_seen DESC LIMIT 1`);
	const actual = live?.name ?? saved?.name;
	if (!actual || actual.trim().normalize('NFC') !== name.trim().normalize('NFC'))
		throw new ApiError(400, 'SteamID64 与本服游戏昵称不匹配，请核对后重新输入。');
	return env.db.transaction(async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${`qq-bind:${serverId}`},0))`
		);
		const rows = await tx.execute<{ member_id: string; steam_id: string }>(sql`
			SELECT member_id,steam_id FROM qq_links WHERE server_id=${serverId} AND (member_id=${memberId} OR steam_id=${steamId})`);
		if (rows.some((r) => r.member_id === memberId && r.steam_id === steamId))
			return { steamId, already: true };
		if (rows.some((r) => r.member_id === memberId))
			throw new ApiError(409, '你已绑定其他 SteamID，请先发送 /解绑。');
		if (rows.length) throw new ApiError(409, '该 SteamID 已绑定其他 QQ，请联系管理员处理。');
		const inserted = await tx.execute(
			sql`INSERT INTO qq_links(server_id,member_id,user_id,steam_id) VALUES(${serverId},${memberId},NULL,${steamId}) ON CONFLICT DO NOTHING RETURNING steam_id`
		);
		if (!inserted.length)
			throw new ApiError(409, 'QQ 或 SteamID 已绑定，请查询绑定状态或联系管理员。');
		await tx.execute(
			sql`DELETE FROM qq_link_codes WHERE server_id=${serverId} AND member_id=${memberId}`
		);
		return { steamId, already: false };
	});
}

export async function unbindQq(env: Env, serverId: string, memberId: string) {
	await env.db.execute(
		sql`DELETE FROM qq_links WHERE server_id=${serverId} AND member_id=${memberId}`
	);
	await env.db.execute(
		sql`DELETE FROM qq_link_codes WHERE server_id=${serverId} AND member_id=${memberId}`
	);
}

const hash = (code: string) => createHash('sha256').update(code).digest('hex');
export async function createLinkCode(env: Env, serverId: string, memberId: string) {
	const code = randomBytes(16).toString('hex');
	await env.db.transaction(async (tx) => {
		await tx.execute(
			sql`DELETE FROM qq_link_codes WHERE expires_at < now() OR (server_id=${serverId} AND member_id=${memberId})`
		);
		await tx.execute(
			sql`INSERT INTO qq_link_codes(code_hash,server_id,member_id,expires_at) VALUES(${hash(code)},${serverId},${memberId},now()+interval '5 minutes')`
		);
	});
	return code;
}
export async function bindAccount(env: Env, code: string, actor: SessionUser) {
	if (!/^[a-f0-9]{32}$/.test(code) || actor.apiKey) throw new ApiError(400, '绑定码无效。');
	return env.db.transaction(async (tx) => {
		const [request] = await tx.execute<{ server_id: string; member_id: string }>(
			sql`DELETE FROM qq_link_codes WHERE code_hash=${hash(code)} AND expires_at>now() RETURNING server_id,member_id`
		);
		if (!request || !qqPolicy(request.server_id)) throw new ApiError(400, '绑定码已过期或已使用。');
		const [verified] = await tx.execute<{ steam_id: string }>(
			sql`SELECT account_id AS steam_id FROM account WHERE user_id=${actor.id} AND provider_id='steam' LIMIT 1`
		);
		if (!verified || !/^\d{17}$/.test(verified.steam_id))
			throw new ApiError(403, '请先在账号设置中登录 Steam 完成验证。');
		const existing = await tx.execute(
			sql`SELECT 1 FROM qq_links WHERE server_id=${request.server_id} AND (member_id=${request.member_id} OR steam_id=${verified.steam_id})`
		);
		if (existing.length)
			throw new ApiError(409, 'QQ 或 Steam 已绑定；如需更换，请先在此页面解绑。');
		await tx.execute(
			sql`INSERT INTO qq_links(server_id,member_id,user_id,steam_id) VALUES(${request.server_id},${request.member_id},${actor.id},${verified.steam_id})`
		);
		return { serverId: request.server_id, steamId: verified.steam_id };
	});
}
export async function linkedAccount(env: Env, serverId: string, memberId: string) {
	const [direct] = await env.db.execute<{ steam_id: string }>(
		sql`SELECT steam_id FROM qq_links WHERE server_id=${serverId} AND member_id=${memberId} AND user_id IS NULL`
	);
	if (direct)
		return {
			steamId: direct.steam_id,
			actor: toSessionUser({ id: `qq:${memberId}`, name: memberId })
		};
	const [row] = await env.db.execute<{ steam_id: string; profile: Record<string, unknown> }>(sql`
	 SELECT l.steam_id, row_to_json(u) AS profile FROM qq_links l JOIN "user" u ON u.id=l.user_id
	 JOIN account a ON a.user_id=u.id AND a.provider_id='steam' AND a.account_id=l.steam_id
	 WHERE l.server_id=${serverId} AND l.member_id=${memberId} AND coalesce(u.banned,false)=false LIMIT 1`);
	if (!row) throw new ApiError(403, '尚未绑定。格式：/绑定 SteamID64 游戏内昵称');
	return { steamId: row.steam_id, actor: toSessionUser(row.profile) };
}
