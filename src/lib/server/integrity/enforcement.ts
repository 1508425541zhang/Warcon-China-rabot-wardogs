import { and, eq, gte, isNull, ne, sql } from 'drizzle-orm';
import type { Env } from '../env';
import {
	integrityActions,
	integrityCases,
	integrityRules,
	integrityScores,
	integrityWindows,
	listEntries,
	lists,
	outbox,
	serverLists,
	serverLive
} from '../db/schema';
import { withOwnedTransaction } from '../leadership';
import { memoryOf } from '../observe';
import { serverListOf } from '../lists';
import { wakeDelivery } from '../outbox';
import { gateway } from '../gateway';
import { writeAudit } from '../audit';
import {
	decideIntegrityAction,
	type EnforcementSettings,
	type IntegrityDecision
} from './decisions';
import { validateIntegrityRules } from './rules';
import type { BehaviorFinding } from './windows';
import type { IntegrityScore } from './score';
import type { Player } from '$lib/types';
import { independentEvidence } from './independence';

const COOLDOWN_MS = 15 * 60_000;
const HOUR_MS = 60 * 60_000;
const REASON = (hours: number, caseId: string) =>
	`社区风控：临时隔离 ${hours} 小时。案件号：${caseId}。如有异议请联系服务器管理员复核。 / Community Integrity: temporary ${hours}h restriction. Case ${caseId}. Contact an administrator to appeal.`;
const KICK_REASON = (caseId: string) =>
	`社区风控：暂时移出服务器。案件号：${caseId}。如有异议请联系服务器管理员复核。 / Community Integrity: temporary removal. Case ${caseId}. Contact an administrator to appeal.`;

/** A persisted case is mandatory; the final switches and evidence are rechecked under a row lock. */
export async function enforceIntegrityCase(
	env: Env,
	input: {
		orgId: string;
		serverId: string;
		steamId: string;
		caseId: string;
		finding: BehaviorFinding;
		score: IntegrityScore;
	}
): Promise<IntegrityDecision> {
	const memory = memoryOf(input.serverId);
	if (!memory || memory.server.orgId !== input.orgId || !/^\d{17}$/.test(input.steamId))
		return 'OBSERVE';
	const [preflight] = await env.db
		.select({
			kick: integrityRules.autoKickEnabled,
			day: integrityRules.autoQuarantine24hEnabled,
			week: integrityRules.autoQuarantine7dEnabled,
			suspended: integrityRules.autoSuspendedAt
		})
		.from(integrityRules)
		.where(eq(integrityRules.orgId, input.orgId))
		.limit(1);
	if (!preflight || preflight.suspended || !(preflight.kick || preflight.day || preflight.week))
		return 'OBSERVE';
	// Ensure the server's own ban list exists before the action transaction starts.
	const banList = await serverListOf(env, memory.server, 'ban');
	const result = await withOwnedTransaction(env, async (tx) => {
		const [row] = await tx
			.select()
			.from(integrityRules)
			.where(eq(integrityRules.orgId, input.orgId))
			.for('update');
		if (
			!row ||
			row.autoSuspendedAt ||
			!(row.autoKickEnabled || row.autoQuarantine24hEnabled || row.autoQuarantine7dEnabled)
		)
			return { decision: 'OBSERVE' as IntegrityDecision, circuit: false };
		const settings: EnforcementSettings = {
			autoKickEnabled: row.autoKickEnabled,
			autoQuarantine24hEnabled: row.autoQuarantine24hEnabled,
			autoQuarantine7dEnabled: row.autoQuarantine7dEnabled,
			autoActionMaxPerHour: row.autoActionMaxPerHour,
			autoActionMaxPercentOnline: row.autoActionMaxPercentOnline,
			autoSuspendedAt: row.autoSuspendedAt
		};
		const now = new Date();
		const [[caseRow], [savedScore], [live], prior, previous] = await Promise.all([
			tx.select().from(integrityCases).where(eq(integrityCases.id, input.caseId)).limit(1),
			tx
				.select()
				.from(integrityScores)
				.where(
					and(
						eq(integrityScores.orgId, input.orgId),
						eq(integrityScores.serverId, input.serverId),
						eq(integrityScores.steamId, input.steamId),
						eq(integrityScores.source, 'window')
					)
				)
				.orderBy(sql`${integrityScores.id} DESC`)
				.limit(1),
			tx.select().from(serverLive).where(eq(serverLive.serverId, input.serverId)).limit(1),
			tx
				.select({ id: integrityWindows.id, eventIds: integrityWindows.eventIds })
				.from(integrityWindows)
				.where(
					and(
						eq(integrityWindows.orgId, input.orgId),
						eq(integrityWindows.steamId, input.steamId),
						input.finding.windowId === null
							? sql`FALSE`
							: ne(integrityWindows.id, input.finding.windowId),
						gte(integrityWindows.observedAt, new Date(now.getTime() - 24 * HOUR_MS))
					)
				),
			tx
				.select()
				.from(integrityActions)
				.where(
					and(
						eq(integrityActions.orgId, input.orgId),
						eq(integrityActions.steamId, input.steamId),
						gte(integrityActions.createdAt, new Date(now.getTime() - 24 * HOUR_MS))
					)
				)
				.orderBy(sql`${integrityActions.createdAt} DESC`)
		]);
		if (
			!caseRow ||
			caseRow.status !== 'OPEN' ||
			caseRow.orgId !== input.orgId ||
			caseRow.serverId !== input.serverId ||
			caseRow.steamId !== input.steamId ||
			!savedScore ||
			savedScore.steamId !== input.steamId ||
			savedScore.windowId !== input.finding.windowId ||
			!savedScore.currentBehaviorAnomaly ||
			savedScore.score !== caseRow.riskScore
		)
			return { decision: 'OBSERVE' as IntegrityDecision, circuit: false };
		const roster = Array.isArray(live?.players) ? (live.players as Player[]) : [];
		const feedHealthy =
			!!live?.ok &&
			!!live?.feedAt &&
			now.getTime() - live.feedAt.getTime() < 5 * 60_000 &&
			!!live.playersAt &&
			now.getTime() - live.playersAt.getTime() < 5 * 60_000;
		const decision = decideIntegrityAction({
			score: input.score,
			finding: input.finding,
			confidence: caseRow.confidence as 'A' | 'B' | 'C' | 'D',
			feedHealthy,
			playerOnline:
				roster.some((player) => player.steamId === input.steamId) &&
				memory.playersAt > 0 &&
				now.getTime() - memory.playersAt < 5 * 60_000 &&
				memory.players.some((player) => player.steamId === input.steamId),
			identityReliable:
				input.finding.eventIds.length > 0 && input.finding.steamId === input.steamId,
			priorIndependentWindow: prior.some((row) =>
				independentEvidence(row.eventIds, input.finding.eventIds)
			),
			previousActions: previous
				.map((action) => action.action)
				.filter(
					(action): action is 'KICK' | 'QUARANTINE_24H' | 'QUARANTINE_7D' =>
						action === 'KICK' || action === 'QUARANTINE_24H' || action === 'QUARANTINE_7D'
				),
			rules: validateIntegrityRules(row.config as Record<string, unknown>),
			settings
		});
		if (decision === 'OBSERVE') return { decision, circuit: false };
		if (previous.some((action) => now.getTime() - action.createdAt.getTime() < COOLDOWN_MS))
			return { decision: 'OBSERVE' as IntegrityDecision, circuit: false };
		const [hourly] = await tx
			.select({ n: sql<number>`COUNT(*)::int` })
			.from(integrityActions)
			.where(
				and(
					eq(integrityActions.orgId, input.orgId),
					eq(integrityActions.source, 'RULE'),
					gte(integrityActions.createdAt, new Date(now.getTime() - HOUR_MS))
				)
			);
		const cap = Math.min(
			settings.autoActionMaxPerHour,
			Math.max(1, Math.floor((roster.length * settings.autoActionMaxPercentOnline) / 100))
		);
		if (Number(hourly?.n ?? 0) >= cap) {
			await tx
				.update(integrityRules)
				.set({ autoSuspendedAt: now })
				.where(eq(integrityRules.orgId, input.orgId));
			return { decision: 'OBSERVE' as IntegrityDecision, circuit: true };
		}
		const activeBans = await tx
			.select({ entry: listEntries })
			.from(listEntries)
			.innerJoin(serverLists, eq(serverLists.listId, listEntries.listId))
			.innerJoin(lists, eq(lists.id, listEntries.listId))
			.where(
				and(
					eq(serverLists.serverId, input.serverId),
					eq(lists.kind, 'ban'),
					eq(listEntries.steamId, input.steamId),
					isNull(listEntries.removedAt),
					sql`(${listEntries.expiresAt} IS NULL OR ${listEntries.expiresAt} > ${now})`
				)
			);
		// An administrator's ban always wins; rule-created entries are never silently shortened.
		const autoIds = new Set(previous.map((action) => action.listEntryId).filter(Boolean));
		if (activeBans.some(({ entry }) => !autoIds.has(entry.id)))
			return { decision: 'OBSERVE' as IntegrityDecision, circuit: false };
		let listEntryId: string | null = null;
		let expiresAt: Date | null = null;
		if (decision !== 'KICK') {
			const hours = decision === 'QUARANTINE_7D' ? 168 : 24;
			expiresAt = new Date(now.getTime() + hours * HOUR_MS);
			const existing = activeBans.find(({ entry }) => autoIds.has(entry.id))?.entry;
			if (existing) {
				listEntryId = existing.id;
				if (!existing.expiresAt || existing.expiresAt >= expiresAt)
					return { decision: 'OBSERVE' as IntegrityDecision, circuit: false };
				await tx
					.update(listEntries)
					.set({ expiresAt, reason: REASON(hours, input.caseId) })
					.where(eq(listEntries.id, existing.id));
			} else {
				listEntryId = crypto.randomUUID();
				await tx.insert(listEntries).values({
					id: listEntryId,
					listId: banList.id,
					steamId: input.steamId,
					reason: REASON(hours, input.caseId),
					expiresAt,
					addedByName: 'Community Integrity rule'
				});
			}
		}
		const actionId = crypto.randomUUID();
		await tx.insert(integrityActions).values({
			id: actionId,
			caseId: input.caseId,
			orgId: input.orgId,
			serverId: input.serverId,
			steamId: input.steamId,
			action: decision,
			source: 'RULE',
			listEntryId,
			createdAt: now,
			expiresAt
		});
		await tx
			.update(integrityScores)
			.set({
				level:
					decision === 'KICK'
						? 'AUTO_KO'
						: decision === 'QUARANTINE_24H'
							? 'AUTO_QUARANTINE_24H'
							: 'AUTO_QUARANTINE_7D'
			})
			.where(eq(integrityScores.id, savedScore.id));
		await tx.insert(outbox).values({
			serverId: input.serverId,
			triggerName: 'Community Integrity',
			triggerKind: 'integrity',
			action: 'kick',
			params: {
				steamId: input.steamId,
				reason:
					decision === 'KICK'
						? KICK_REASON(input.caseId)
						: REASON(decision === 'QUARANTINE_7D' ? 168 : 24, input.caseId)
			},
			target: input.steamId,
			steamId: input.steamId,
			okMessage: `Community Integrity ${decision}: ${input.steamId}`,
			detail: { caseId: input.caseId, actionId, source: 'RULE' },
			dedupeKey: `integrity:${actionId}:kick`
		});
		return { decision, circuit: false };
	});
	if (result.circuit)
		await writeAudit(env, null, {
			actorName: 'Community Integrity',
			orgId: input.orgId,
			category: 'system',
			action: 'integrity.enforcement.circuit_breaker',
			outcome: 'ok',
			message: 'Experimental automatic enforcement suspended by rate cap',
			detail: { serverId: input.serverId, caseId: input.caseId }
		});
	if (result.decision !== 'OBSERVE') {
		wakeDelivery();
		if (result.decision !== 'KICK')
			await gateway()
				.syncServer(env, memory.server, memory.org, 15_000)
				.catch((err) => console.warn('[warcon] Integrity ban list sync:', err));
		await writeAudit(env, null, {
			actorName: 'Community Integrity',
			orgId: input.orgId,
			server: { id: memory.server.id, name: memory.server.name },
			category: 'system',
			action: 'integrity.enforcement.action',
			target: input.steamId,
			outcome: 'ok',
			message: `${result.decision} queued for case ${input.caseId}`,
			detail: { caseId: input.caseId, decision: result.decision }
		});
	}
	return result.decision;
}
