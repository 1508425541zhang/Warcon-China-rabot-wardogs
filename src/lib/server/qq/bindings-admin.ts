import { sql } from 'drizzle-orm';
import { z } from 'zod';
import type { Env } from '../env';
import type { SessionUser, ServerRow } from '../access';
import { ApiError } from '../http';
import { auditLog } from '../db/schema';
import { validQqIdentity } from './official';
import { qqCredentials } from './config';

export async function bindingsView(env: Env, serverId: string, query = '', page = 1) {
	query = query.trim().slice(0, 128);
	page = Math.max(1, Math.min(100000, Math.floor(page) || 1));
	const match = sql`l.server_id=${serverId} AND (${query}='' OR strpos(l.member_id,${query})>0 OR strpos(l.steam_id,${query})>0 OR strpos(lower(coalesce(p.name,'')),lower(${query}))>0)`;
	const player = sql`LEFT JOIN LATERAL (SELECT name FROM player_sessions WHERE server_id=l.server_id AND steam_id=l.steam_id ORDER BY last_seen DESC LIMIT 1) p ON true`;
	const [count] = await env.db.execute<{ total: string }>(sql`SELECT count(*) AS total FROM qq_links l ${player} WHERE ${match}`);
	const total = Number(count.total);
	page = Math.min(page, Math.max(1, Math.ceil(total / 50)));
	const links = await env.db.execute<{member_id:string;steam_id:string;user_id:string|null;name:string|null;account_name:string|null;previous_member_id:string|null}>(sql`
		SELECT l.member_id,l.steam_id,l.user_id,p.name,u.name AS account_name,m.previous_member_id FROM qq_links l ${player}
		LEFT JOIN "user" u ON u.id=l.user_id
		LEFT JOIN LATERAL (SELECT detail->>'previousMemberId' AS previous_member_id FROM audit_log
			WHERE server_id=l.server_id AND target=l.member_id AND action='qq.binding.migrate' AND outcome='ok'
			AND detail->>'previousMemberId' LIKE 'ob11:%' ORDER BY id DESC LIMIT 1) m ON true
		WHERE ${match} ORDER BY l.member_id LIMIT 50 OFFSET ${(page-1)*50}`);
	const recent = await env.db.execute<{member_id:string;last_seen:string}>(sql`
		SELECT member_id,max(created_at)::text AS last_seen FROM qq_inbox WHERE server_id=${serverId}
		AND created_at>now()-interval '30 days' AND member_id LIKE 'official:%' GROUP BY member_id ORDER BY max(created_at) DESC LIMIT 100`);
	return { links: Array.from(links), recent: Array.from(recent), total, page, query };
}

const inputSchema = z.object({
	action: z.enum(['add','change','remove','migrate']), memberId: z.string().max(160),
	previousMemberId: z.string().max(160).optional(),
	steamId: z.string().regex(/^\d{17}$/).optional(),
	expectedSteamId: z.string().regex(/^\d{17}$/).optional(),
	expectedUserId: z.string().nullable().optional(),
	reason: z.string().trim().min(1).max(300)
});

export async function manageBinding(env: Env, server: ServerRow, actor: SessionUser, raw: unknown) {
	const parsed = inputSchema.safeParse(raw);
	if (!parsed.success) throw new ApiError(400,'请填写有效的用户标识、17 位 SteamID64 和处理原因。');
	const input = parsed.data;
	if (!validQqIdentity(input.memberId)) throw new ApiError(400,'用户标识格式无效，请从近期互动用户中选择或复制完整官方标识。');
	if (input.action !== 'remove' && !input.steamId) throw new ApiError(400,'请填写 SteamID64。');
	if (input.action !== 'add' && (!input.expectedSteamId || input.expectedUserId === undefined)) throw new ApiError(400,'缺少原绑定信息，请刷新页面。');
	if (input.action === 'add' || input.action === 'migrate') {
		const credentials = qqCredentials();
		if (credentials?.provider !== 'official' || !input.memberId.startsWith(`official:${credentials.selfId}:`))
			throw new ApiError(400,'补绑必须使用当前官方机器人的完整用户标识。');
	}
	if (input.action === 'migrate' && (!input.previousMemberId || !/^ob11:[1-9]\d+$/.test(input.previousMemberId) || input.steamId !== input.expectedSteamId))
		throw new ApiError(400,'迁移需指定原旧版标识，SteamID 必须保持一致。');
	return env.db.transaction(async tx => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended(${`qq-bind:${server.id}`},0))`);
		const sourceMember = input.action === 'migrate' ? input.previousMemberId! : input.memberId;
		const [before] = await tx.execute<{steam_id:string;user_id:string|null}>(sql`SELECT steam_id,user_id FROM qq_links WHERE server_id=${server.id} AND member_id=${sourceMember}`);
		if (input.action === 'add' ? !!before : !before || before.steam_id !== input.expectedSteamId || before.user_id !== input.expectedUserId)
			throw new ApiError(409,'绑定已发生变化，请刷新后重新操作。');
		if (input.action === 'migrate') {
			const claimed = await tx.execute(sql`SELECT 1 FROM qq_links WHERE server_id=${server.id} AND member_id=${input.memberId}`);
			if (claimed.length) throw new ApiError(409,'该官方用户已有绑定，请核实后处理，不能直接覆盖。');
		}
		if (input.action !== 'remove') {
			const conflict = await tx.execute(sql`SELECT 1 FROM qq_links WHERE server_id=${server.id} AND steam_id=${input.steamId!} AND member_id<>${sourceMember}`);
			if (conflict.length) throw new ApiError(409,'该 SteamID 已绑定其他用户，请先核实并解绑原记录。');
		}
		if (input.action === 'remove') await tx.execute(sql`DELETE FROM qq_links WHERE server_id=${server.id} AND member_id=${input.memberId}`);
		else if (input.action === 'migrate') await tx.execute(sql`UPDATE qq_links SET member_id=${input.memberId} WHERE server_id=${server.id} AND member_id=${sourceMember}`);
		else if (input.action === 'add') await tx.execute(sql`INSERT INTO qq_links(server_id,member_id,user_id,steam_id) VALUES(${server.id},${input.memberId},NULL,${input.steamId!})`);
		else if (before!.steam_id !== input.steamId) await tx.execute(sql`UPDATE qq_links SET steam_id=${input.steamId!},user_id=NULL WHERE server_id=${server.id} AND member_id=${input.memberId}`);
		await tx.execute(sql`DELETE FROM qq_link_codes WHERE server_id=${server.id} AND member_id IN (${input.memberId},${sourceMember})`);
		await tx.insert(auditLog).values({actorId:actor.id,actorName:actor.username,serverId:server.id,serverName:server.name,orgId:server.orgId,
			category:'player',action:`qq.binding.${input.action}`,target:input.memberId,outcome:'ok',detail:{before:before??null,previousMemberId:sourceMember,steamId:input.action==='remove'?null:input.steamId,reason:input.reason}});
		return { ok: true };
	});
}
