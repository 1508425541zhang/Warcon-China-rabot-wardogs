import { and, eq, isNull } from 'drizzle-orm';
import type { Tx } from '../db';
import type { SessionUser } from '../access';
import {
	integrityActions,
	integrityCases,
	listEntries,
	lists,
	outbox,
	servers
} from '../db/schema';
import { ensureServerLists } from '../lists';
import { ApiError } from '../http';

/** Called under the case row lock, in the same transaction as the human review. */
export async function applyReviewPenalty(
	tx: Tx,
	c: typeof integrityCases.$inferSelect,
	actor: SessionUser,
	reviewReason: string
) {
	const id = `review-7d:${c.id}`;
	const [prior] = await tx.select().from(integrityActions).where(eq(integrityActions.id, id));
	if (prior) return { ...prior, reused: true }; // Never extend a ban by saving the same case again.
	const [server] = await tx
		.select({ id: servers.id })
		.from(servers)
		.where(and(eq(servers.id, c.serverId), eq(servers.orgId, c.orgId)));
	if (!server || !/^\d{17}$/.test(c.steamId))
		throw new ApiError(409, '案件服务器或玩家身份无效，审核和封禁均未保存。');
	await ensureServerLists(tx, c.serverId, c.orgId);
	const [list] = await tx
		.select()
		.from(lists)
		.where(and(eq(lists.serverId, c.serverId), eq(lists.orgId, c.orgId), eq(lists.kind, 'ban')))
		.for('update');
	const now = new Date(),
		requestedExpiry = new Date(now.getTime() + 7 * 86400000);
	const reason = `人工确认违规，封禁7天。案件 ${c.id}：${reviewReason}`.slice(0, 200);
	const [existing] = await tx
		.select()
		.from(listEntries)
		.where(
			and(
				eq(listEntries.listId, list.id),
				eq(listEntries.steamId, c.steamId),
				isNull(listEntries.removedAt)
			)
		)
		.for('update');
	const entryId = existing?.id ?? crypto.randomUUID();
	let expiresAt: Date | null = requestedExpiry;
	if (existing) {
		// A permanent or longer existing ban wins. Expired, not-yet-swept entries can be renewed.
		if (existing.expiresAt === null || existing.expiresAt > requestedExpiry)
			expiresAt = existing.expiresAt;
		else
			await tx
				.update(listEntries)
				.set({ expiresAt, reason, addedBy: actor.id, addedByName: actor.username })
				.where(eq(listEntries.id, entryId));
	} else
		await tx
			.insert(listEntries)
			.values({
				id: entryId,
				listId: list.id,
				steamId: c.steamId,
				reason,
				expiresAt,
				addedBy: actor.id,
				addedByName: actor.username
			});
	await tx.update(lists).set({ updatedAt: now }).where(eq(lists.id, list.id));
	const [action] = await tx
		.insert(integrityActions)
		.values({
			id,
			caseId: c.id,
			orgId: c.orgId,
			serverId: c.serverId,
			steamId: c.steamId,
			action: 'QUARANTINE_7D',
			source: 'REVIEW',
			listEntryId: entryId,
			createdAt: now,
			effectiveAt: now,
			expiresAt,
			deliveryState: 'pending'
		})
		.returning();
	await tx
		.insert(outbox)
		.values({
			serverId: c.serverId,
			triggerName: '人工案件确认违规',
			triggerKind: 'integrity',
			action: 'kick',
			params: {
				steamId: c.steamId,
				reason:
					expiresAt === null || expiresAt > requestedExpiry
						? '人工确认违规，已有更长期封禁继续生效。'
						: reason
			},
			target: c.steamId,
			steamId: c.steamId,
			okMessage: `案件 ${c.id}：人工确认违规并封禁7天`,
			detail: { caseId: c.id, actionId: id, source: 'REVIEW', reviewerId: actor.id },
			dedupeKey: `integrity:${id}:kick`
		});
	return { ...action, reused: false };
}
