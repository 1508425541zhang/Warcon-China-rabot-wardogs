import { and, desc, eq, gte, inArray, isNull, lte, or, sql } from 'drizzle-orm';
import type { Env } from '../env';
import type { DbOrTx } from '../db';
import type { SessionUser } from '../access';
import { serverAccessFor } from '../access';
import { effectiveFeatures } from '$lib/features';
import { writeAudit } from '../audit';
import {
	account,
	integrityReporterStats,
	integrityReportEvents,
	integrityReports,
	integrityScores,
	kills,
	playerSessions,
	organizations,
	servers
} from '../db/schema';
import { ApiError, str } from '../http';
import { getIntegrityRules } from './rules';
import { scoreIntegrity } from './score';

export interface ReportTarget {
	steamId: string;
	name: string;
}

const frozenKill = (row: typeof kills.$inferSelect) => ({
	...row,
	ts: row.ts.toISOString()
});

async function copyReportEvents(
	db: DbOrTx,
	report: {
		id: number;
		serverId: string;
		targetSteamId: string;
		evidenceFrom: Date;
		evidenceUntil: Date;
	},
	eventIds?: readonly string[]
): Promise<void> {
	const conditions = [
		eq(kills.serverId, report.serverId),
		gte(kills.ts, report.evidenceFrom),
		lte(kills.ts, report.evidenceUntil),
		or(eq(kills.killerSteamId, report.targetSteamId), eq(kills.victimSteamId, report.targetSteamId))
	];
	if (eventIds) conditions.push(inArray(kills.eventId, [...eventIds]));
	const rows = await db
		.select()
		.from(kills)
		.where(and(...conditions))
		.orderBy(desc(kills.ts))
		.limit(1001);
	if (rows.length > 1000) throw new Error('Report evidence event limit exceeded.');
	if (rows.length)
		await db
			.insert(integrityReportEvents)
			.values(
				rows.map((row) => ({
					reportId: report.id,
					instanceId: row.instanceId,
					eventId: row.eventId,
					receivedAt: row.ts,
					event: frozenKill(row)
				}))
			)
			.onConflictDoNothing();
}

/** Copy post-report kill telemetry while its 180-second evidence period is open. */
export async function captureReportEvidence(
	env: Env,
	serverId: string,
	batch: readonly { eventId: string }[]
): Promise<void> {
	if (!batch.length) return;
	const now = new Date();
	const reports = await env.db
		.select({
			id: integrityReports.id,
			serverId: integrityReports.serverId,
			targetSteamId: integrityReports.targetSteamId,
			evidenceFrom: integrityReports.evidenceFrom,
			evidenceUntil: integrityReports.evidenceUntil
		})
		.from(integrityReports)
		.where(
			and(
				eq(integrityReports.serverId, serverId),
				gte(integrityReports.evidenceUntil, new Date(now.getTime() - 30_000)),
				lte(integrityReports.createdAt, now)
			)
		);
	const ids = [...new Set(batch.map((kill) => kill.eventId))];
	for (const report of reports) await copyReportEvents(env.db, report, ids);
}

export function parseReportCommand(message: string): { target: string; reason: string } | null {
	const match = /^!(?:report|ban)\s+(?:"([^"]+)"|(\S+))\s+(.+)$/i.exec(message.trim());
	if (!match) return null;
	return { target: match[1] ?? match[2], reason: match[3].trim() };
}

/** Resolve to one recently seen SteamID64. A nickname is never the identity key. */
export function resolveReportTarget(input: string, roster: readonly ReportTarget[]): ReportTarget {
	const target = input.trim();
	const candidates = [...new Map(roster.map((player) => [player.steamId, player])).values()];
	let matches = /^\d{17}$/.test(target)
		? candidates.filter((player) => player.steamId === target)
		: candidates.filter((player) => player.name === target);
	if (!matches.length)
		matches = candidates.filter((player) => player.name.toLowerCase() === target.toLowerCase());
	if (!matches.length && target.length >= 3)
		matches = candidates.filter((player) =>
			player.name.toLowerCase().includes(target.toLowerCase())
		);
	if (matches.length > 1)
		throw new ApiError(
			409,
			'Multiple players matched. Use the exact player name or SteamID64.',
			'multiple_targets'
		);
	if (!matches.length)
		throw new ApiError(404, 'Player not found on this server recently.', 'target_not_found');
	return matches[0];
}

export async function submitReport(
	env: Env,
	req: Request,
	actor: SessionUser,
	input: { serverId: unknown; target: unknown; reason: unknown }
): Promise<{ id: number; targetSteamId: string }> {
	const serverId = str(input.serverId, 100);
	const reason = str(input.reason, 300);
	const targetText = str(input.target, 200);
	if (!serverId || !targetText || reason.length < 3)
		throw new ApiError(400, 'Server, target and a reason of at least 3 characters are required.');
	const [[visible], [verified]] = await Promise.all([
		env.db
			.select({ server: servers, org: organizations })
			.from(servers)
			.innerJoin(organizations, eq(organizations.id, servers.orgId))
			.where(and(eq(servers.id, serverId), isNull(organizations.suspendedAt)))
			.limit(1),
		env.db
			.select({ steamId: account.accountId })
			.from(account)
			.where(and(eq(account.userId, actor.id), eq(account.providerId, 'steam')))
			.limit(1)
	]);
	if (!visible) throw new ApiError(404, 'Server not found.');
	const { server, org } = visible;
	// The public status switch is the same one used by public routes. Private servers need
	// an actual server grant; knowing an ID does not grant the right to create reports.
	if (!effectiveFeatures(org, server).status) {
		const access = await serverAccessFor(env, actor, serverId);
		if (!access?.caps.has('integrity.view'))
			throw new ApiError(404, 'Server not found.', 'not_found');
	}
	if (!verified || !/^\d{17}$/.test(verified.steamId))
		throw new ApiError(
			403,
			'Link a verified Steam account before reporting.',
			'steam_link_required'
		);
	const now = new Date();
	const recent = await env.db
		.select({ steamId: playerSessions.steamId, name: playerSessions.name })
		.from(playerSessions)
		.where(
			and(
				eq(playerSessions.serverId, serverId),
				gte(playerSessions.lastSeen, new Date(now.getTime() - 15 * 60_000))
			)
		)
		.orderBy(desc(playerSessions.lastSeen))
		.limit(250);
	const target = resolveReportTarget(targetText, recent.reverse());
	if (target.steamId === verified.steamId) throw new ApiError(400, 'You cannot report yourself.');
	const rules = await getIntegrityRules(env, server.orgId);
	const row = await env.db.transaction(async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${`reporter:${verified.steamId}`}, 0))`
		);
		const [duplicate] = await tx
			.select({ id: integrityReports.id })
			.from(integrityReports)
			.where(
				and(
					eq(integrityReports.orgId, server.orgId),
					eq(integrityReports.reporterSteamId, verified.steamId),
					eq(integrityReports.targetSteamId, target.steamId),
					gte(integrityReports.createdAt, new Date(now.getTime() - 10 * 60_000))
				)
			)
			.limit(1);
		if (duplicate) throw new ApiError(429, 'You recently reported this player. Please wait.');
		const [count] = await tx
			.select({ n: sql<number>`COUNT(*)` })
			.from(integrityReports)
			.where(
				and(
					eq(integrityReports.reporterSteamId, verified.steamId),
					gte(integrityReports.createdAt, new Date(now.getTime() - 60 * 60_000))
				)
			);
		if (Number(count?.n ?? 0) >= 5)
			throw new ApiError(429, 'Report limit reached. Please wait before reporting again.');
		const [created] = await tx
			.insert(integrityReports)
			.values({
				orgId: server.orgId,
				serverId,
				targetSteamId: target.steamId,
				reporterSteamId: verified.steamId,
				reason,
				source: 'panel',
				createdAt: now,
				evidenceFrom: new Date(now.getTime() - 180_000),
				evidenceUntil: new Date(now.getTime() + 180_000)
			})
			.returning({
				id: integrityReports.id,
				serverId: integrityReports.serverId,
				targetSteamId: integrityReports.targetSteamId,
				evidenceFrom: integrityReports.evidenceFrom,
				evidenceUntil: integrityReports.evidenceUntil
			});
		await copyReportEvents(tx, { ...created, evidenceUntil: now });
		await tx
			.insert(integrityReporterStats)
			.values({ orgId: server.orgId, steamId: verified.steamId, reportsSubmitted: 1 })
			.onConflictDoUpdate({
				target: [integrityReporterStats.orgId, integrityReporterStats.steamId],
				set: { reportsSubmitted: sql`${integrityReporterStats.reportsSubmitted} + 1` }
			});
		const [reporters] = await tx
			.select({ count: sql<number>`COUNT(DISTINCT ${integrityReports.reporterSteamId})` })
			.from(integrityReports)
			.where(
				and(
					eq(integrityReports.orgId, server.orgId),
					eq(integrityReports.targetSteamId, target.steamId),
					gte(integrityReports.createdAt, new Date(now.getTime() - 24 * 60 * 60_000))
				)
			);
		const score = scoreIntegrity(
			{
				behaviorReasons: [],
				kpm180: 0,
				uniqueVictims: 0,
				previousKpm: [],
				uniqueReporters: Number(reporters?.count ?? 0),
				repeatAutoKo: false,
				infantryKills: 0,
				headshots: 0,
				penetrations: 0,
				burstPoints: 0,
				vacBans: 0,
				gameBans: 0,
				daysSinceLastBan: null,
				wardogsPlaytimeHours: null
			},
			rules.config
		);
		await tx.insert(integrityScores).values({
			reportId: created.id,
			source: 'report',
			orgId: server.orgId,
			serverId,
			steamId: target.steamId,
			scoredAt: now,
			ruleVersion: rules.version,
			score: score.score,
			level: score.level,
			breakdown: score.breakdown,
			currentBehaviorAnomaly: false
		});
		return created;
	});
	await writeAudit(env, req, {
		actor,
		orgId: server.orgId,
		category: 'player',
		action: 'integrity.report.create',
		target: target.steamId,
		outcome: 'ok',
		message: `Report ${row.id} recorded for staff review`,
		detail: { reportId: row.id, serverId, reporterSteamId: verified.steamId }
	});
	return { id: row.id, targetSteamId: target.steamId };
}
