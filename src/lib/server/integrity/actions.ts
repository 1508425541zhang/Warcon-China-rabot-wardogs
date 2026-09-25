import { and, eq, isNull } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import { integrityActions, type OutboxRow } from '../db/schema';

export type IntegrityDeliveryState = 'pending' | 'delivered' | 'failed' | 'skipped' | 'unknown';
export type EffectiveAction = 'KICK' | 'QUARANTINE_24H' | 'QUARANTINE_7D';

/** A high score or queued outbox row is not evidence of a completed player action. */
export function effectiveActionKinds(
	rows: readonly Pick<
		typeof integrityActions.$inferSelect,
		'action' | 'source' | 'effectiveAt' | 'revertedAt' | 'deliveryState'
	>[]
): EffectiveAction[] {
	return rows
		.filter(
			(row) =>
				row.source === 'RULE' &&
				row.deliveryState === 'delivered' &&
				row.effectiveAt !== null &&
				row.revertedAt === null
		)
		.map((row) => row.action)
		.filter(
			(action): action is EffectiveAction =>
				action === 'KICK' || action === 'QUARANTINE_24H' || action === 'QUARANTINE_7D'
		);
}

/** Called in the same transaction that finalizes the Integrity outbox row. */
export async function recordIntegrityDelivery(
	db: DbOrTx,
	row: Pick<OutboxRow, 'triggerKind' | 'action' | 'detail' | 'serverId' | 'steamId'>,
	state: Exclude<IntegrityDeliveryState, 'pending'>,
	at = new Date()
): Promise<void> {
	if (row.triggerKind !== 'integrity') return;
	const detail = row.detail as Record<string, unknown> | null;
	const actionId = detail?.actionId;
	const caseId = detail?.caseId;
	if (
		row.action !== 'kick' ||
		typeof actionId !== 'string' ||
		typeof caseId !== 'string' ||
		!row.steamId
	)
		throw new Error('Integrity outbox row lacks a valid action identity.');
	const [action] = await db
		.select()
		.from(integrityActions)
		.where(
			and(
				eq(integrityActions.id, actionId),
				eq(integrityActions.caseId, caseId),
				eq(integrityActions.serverId, row.serverId),
				eq(integrityActions.steamId, row.steamId)
			)
		)
		.limit(1);
	if (!action) throw new Error('Integrity action is missing at delivery finalization.');
	if (action.revertedAt) return; // A human reversal remains authoritative.
	const [updated] = await db
		.update(integrityActions)
		.set({
			deliveryState: state,
			...(state === 'delivered' ? { effectiveAt: at } : {})
		})
		.where(and(eq(integrityActions.id, actionId), isNull(integrityActions.revertedAt)))
		.returning({ id: integrityActions.id });
	if (!updated) throw new Error('Integrity action changed during delivery finalization.');
}
