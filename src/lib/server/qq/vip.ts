import { and, eq, sql } from 'drizzle-orm';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import type { Env } from '../env';
import type { DbOrTx } from '../db';
import { siteSettings, servers, lists, serverLists, integrityActions } from '../db/schema';
import { ApiError } from '../http';

const KEY = 'qqVip';
const entrySchema = z.object({
	serverId: z.string().min(1).max(100),
	steamId: z.string().regex(/^\d{17}$/),
	enabled: z.boolean(),
	reserve: z.boolean(),
	allowOverkill: z.boolean(),
	whitelist: z.boolean(),
	note: z.string().trim().max(200).default('')
});
export type VipEntry = z.infer<typeof entrySchema>;
export type VipSettings = { revision: string; entries: VipEntry[] };
export async function vipSettings(env: { db: DbOrTx }): Promise<VipSettings> {
	const [row] = await env.db.select().from(siteSettings).where(eq(siteSettings.key, KEY));
	return row ? (row.value as VipSettings) : { revision: 'empty', entries: [] };
}
export async function serverVips(env: { db: DbOrTx }, serverId: string) {
	return (await vipSettings(env)).entries.filter((v) => v.serverId === serverId && v.enabled);
}
export async function vipFor(env: { db: DbOrTx }, serverId: string, steamId: string) {
	return (await serverVips(env, serverId)).find((v) => v.steamId === steamId);
}
export function vipAllowsMetric(vip: VipEntry | undefined, metric: string) {
	return (
		!vip?.enabled || (!vip.whitelist && !(vip.allowOverkill && ['kpm', 'kd'].includes(metric)))
	);
}
export async function vipRiskKickExempt(
	env: Env,
	row: { serverId: string; steamId: string | null; triggerKind: string; action: string }
) {
	return (
		row.triggerKind === 'risk_kick' &&
		row.action === 'kick' &&
		!!row.steamId &&
		!!(await vipFor(env, row.serverId, row.steamId))?.whitelist
	);
}
export async function saveVips(env: Env, input: unknown, userId: string) {
	const parsed = z
		.object({ revision: z.string().min(1).max(100), entries: z.array(entrySchema).max(500) })
		.safeParse(input);
	if (!parsed.success)
		throw new ApiError(400, 'VIP 配置无效：请输入有效的 17 位 SteamID64，备注不超过 200 字。');
	const { revision, entries } = parsed.data;
	if (new Set(entries.map((v) => `${v.serverId}:${v.steamId}`)).size !== entries.length)
		throw new ApiError(400, '同一服务器不能重复绑定相同 SteamID64。');
	if (entries.some((v) => !v.reserve && !v.allowOverkill && !v.whitelist))
		throw new ApiError(400, '每项 VIP 至少选择一种权益。');
	await env.db.transaction(async (tx) => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended('qq-vip-settings',0))`);
		const [old] = await tx.select().from(siteSettings).where(eq(siteSettings.key, KEY));
		if (revision !== (old ? (old.value as VipSettings).revision : 'empty'))
			throw new ApiError(409, 'VIP 名单已被其他管理员修改，请重新加载。');
		const choices = await tx.select({ id: servers.id, orgId: servers.orgId }).from(servers);
		if (entries.some((v) => !choices.some((s) => s.id === v.serverId)))
			throw new ApiError(400, '服务器不存在。');
		// The list reconciler owns game delivery and removes only panel-managed slots.
		for (const serverId of new Set(
			entries.filter((v) => v.enabled && v.reserve).map((v) => v.serverId)
		)) {
			const server = choices.find((s) => s.id === serverId)!;
			await tx
				.insert(lists)
				.values({
					id: randomUUID(),
					orgId: server.orgId,
					serverId,
					kind: 'reserve',
					name: 'Server',
					createdBy: userId
				})
				.onConflictDoNothing();
			const [list] = await tx
				.select()
				.from(lists)
				.where(and(eq(lists.serverId, serverId), eq(lists.kind, 'reserve')));
			await tx.insert(serverLists).values({ serverId, listId: list.id }).onConflictDoNothing();
		}
		const value: VipSettings = { revision: randomUUID(), entries };
		await tx
			.insert(siteSettings)
			.values({ key: KEY, value, updatedAt: new Date(), updatedBy: userId })
			.onConflictDoUpdate({
				target: siteSettings.key,
				set: { value, updatedAt: new Date(), updatedBy: userId }
			});
	});
	return vipSettings(env);
}

/** Whitelisting permits re-entry after automatic quarantine, without erasing evidence or human bans. */
export async function vipAutomaticBanExempt(
	env: Env,
	serverId: string,
	entry: { id: string; steamId: string; addedBy: string | null; addedByName: string }
) {
	if (
		entry.addedBy ||
		!['Community Integrity rule', 'Model A-test rule'].includes(entry.addedByName) ||
		!(await vipFor(env, serverId, entry.steamId))?.whitelist
	)
		return false;
	if (entry.addedByName === 'Model A-test rule') {
		const rows = await env.db.execute(
			sql`SELECT 1 FROM integrity_model_runs WHERE server_id=${serverId} AND steam_id=${entry.steamId} AND list_entry_id=${entry.id} AND action='QUARANTINE_24H' LIMIT 1`
		);
		return rows.length > 0;
	}
	const actions = await env.db
		.select({ source: integrityActions.source })
		.from(integrityActions)
		.where(
			and(
				eq(integrityActions.serverId, serverId),
				eq(integrityActions.listEntryId, entry.id),
				sql`${integrityActions.revertedAt} IS NULL`
			)
		);
	return actions.some((a) => a.source === 'RULE') && !actions.some((a) => a.source === 'REVIEW');
}
