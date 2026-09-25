import { and, eq } from 'drizzle-orm';
import type { Env } from '../env';
import {
	integrityActions,
	integrityCases,
	integrityRules,
	servers,
	type OutboxRow
} from '../db/schema';

const DISABLED = 'Integrity enforcement disabled before delivery';

/** Read committed state, including setting changes made after the outbox row was claimed. */
export async function integrityDeliverySkipReason(
	env: Env,
	row: Pick<OutboxRow, 'triggerKind' | 'action' | 'detail' | 'serverId' | 'steamId'>
): Promise<string | null> {
	if (row.triggerKind !== 'integrity') return null;
	const detail = row.detail as Record<string, unknown> | null;
	const actionId = detail?.actionId;
	const caseId = detail?.caseId;
	if (
		row.action !== 'kick' ||
		typeof actionId !== 'string' ||
		typeof caseId !== 'string' ||
		!row.steamId
	)
		return 'Integrity action identity is invalid before delivery';
	const [match] = await env.db
		.select({ action: integrityActions, caseRow: integrityCases, server: servers })
		.from(integrityActions)
		.innerJoin(integrityCases, eq(integrityCases.id, integrityActions.caseId))
		.innerJoin(servers, eq(servers.id, integrityActions.serverId))
		.where(and(eq(integrityActions.id, actionId), eq(integrityActions.caseId, caseId)))
		.limit(1);
	if (
		!match ||
		match.action.source !== 'RULE' ||
		match.action.revertedAt ||
		match.action.serverId !== row.serverId ||
		match.action.steamId !== row.steamId ||
		match.caseRow.orgId !== match.action.orgId ||
		match.caseRow.serverId !== row.serverId ||
		match.caseRow.steamId !== row.steamId ||
		match.server.orgId !== match.action.orgId ||
		match.caseRow.status !== 'OPEN'
	)
		return 'Integrity action or case changed before delivery';
	const [rules] = await env.db
		.select({
			version: integrityRules.version,
			kick: integrityRules.autoKickEnabled,
			day: integrityRules.autoQuarantine24hEnabled,
			week: integrityRules.autoQuarantine7dEnabled,
			suspended: integrityRules.autoSuspendedAt
		})
		.from(integrityRules)
		.where(eq(integrityRules.orgId, match.action.orgId))
		.limit(1);
	if (!rules || rules.suspended || rules.version !== match.caseRow.ruleVersion) return DISABLED;
	switch (match.action.action) {
		case 'KICK':
			return rules.kick ? null : DISABLED;
		case 'QUARANTINE_24H':
			return rules.day ? null : DISABLED;
		case 'QUARANTINE_7D':
			return rules.week ? null : DISABLED;
		default:
			return 'Integrity action type is invalid before delivery';
	}
}
