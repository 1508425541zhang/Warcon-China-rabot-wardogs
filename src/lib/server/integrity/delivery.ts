import { and, eq } from 'drizzle-orm';
import type { Env } from '../env';
import {
	integrityActions,
	listEntries,
	lists,
	serverLists,
	integrityCases,
	integrityModelState,
	integrityRules,
	servers,
	type OutboxRow
} from '../db/schema';
import type { StatisticalAssessment } from './statistics';
import { STATISTICAL_AUTO_ACTION_ENABLED, STATISTICAL_MODEL_CONFIG } from './statistical-config';

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
		!['RULE', 'REVIEW'].includes(match.action.source) ||
		match.action.revertedAt ||
		match.action.serverId !== row.serverId ||
		match.action.steamId !== row.steamId ||
		match.caseRow.orgId !== match.action.orgId ||
		match.caseRow.serverId !== row.serverId ||
		match.caseRow.steamId !== row.steamId ||
		match.server.orgId !== match.action.orgId ||
		(match.action.source === 'RULE' && match.caseRow.status !== 'OPEN')
	)
		return 'Integrity action or case changed before delivery';
	if (match.action.source === 'REVIEW') {
		if (
			match.action.action !== 'QUARANTINE_7D' ||
			!match.action.listEntryId ||
			!match.caseRow.reviewedAt
		)
			return '人工审核处罚记录不完整';
		const [entry] = await env.db
			.select({ ban: listEntries })
			.from(listEntries)
			.innerJoin(lists, eq(lists.id, listEntries.listId))
			.innerJoin(serverLists, eq(serverLists.listId, lists.id))
			.where(
				and(
					eq(listEntries.id, match.action.listEntryId),
					eq(lists.kind, 'ban'),
					eq(lists.orgId, match.action.orgId),
					eq(serverLists.serverId, row.serverId)
				)
			);
		const ban = entry?.ban;
		return ban &&
			ban.steamId === row.steamId &&
			!ban.removedAt &&
			(!ban.expiresAt || ban.expiresAt > new Date())
			? null
			: '人工封禁已撤销或到期';
	}
	const [rules] = await env.db
		.select({
			version: integrityRules.version,
			assessmentMode: integrityRules.assessmentMode,
			kick: integrityRules.autoKickEnabled,
			day: integrityRules.autoQuarantine24hEnabled,
			week: integrityRules.autoQuarantine7dEnabled,
			suspended: integrityRules.autoSuspendedAt
		})
		.from(integrityRules)
		.where(eq(integrityRules.orgId, match.action.orgId))
		.limit(1);
	if (!rules || rules.suspended || rules.version !== match.caseRow.ruleVersion) return DISABLED;
	if (rules.assessmentMode === 'statistical') {
		const [state] = await env.db
			.select()
			.from(integrityModelState)
			.where(eq(integrityModelState.orgId, match.action.orgId))
			.limit(1);
		const frozen = match.caseRow.statistical as StatisticalAssessment | null;
		if (
			!STATISTICAL_AUTO_ACTION_ENABLED ||
			!state ||
			state.baselineStatus !== 'READY' ||
			!frozen ||
			frozen.modelVersion !== STATISTICAL_MODEL_CONFIG.modelVersion ||
			frozen.featureVersion !== STATISTICAL_MODEL_CONFIG.featureVersion ||
			frozen.weaponMapVersion !== state.weaponMapVersion ||
			!frozen.baselineGeneration ||
			frozen.baselineGeneration !== state.activeBaselineGeneration ||
			frozen.committee?.generation !== STATISTICAL_MODEL_CONFIG.modelVersion ||
			frozen.committee.decision !== 'KICK_CANDIDATE' ||
			frozen.committee.autoActionBlocked
		)
			return DISABLED;
	}
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
