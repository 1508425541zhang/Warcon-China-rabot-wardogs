import { loadCurrentRisks } from '$lib/server/integrity/current-risk';
import { error } from '@sveltejs/kit';
import { mapId } from '$lib/format';
import { and, desc, eq, gte, sql } from 'drizzle-orm';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { orgRoleFor, requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import {
	integrityCases,
	integrityLabels,
	integrityActions,
	integrityReports,
	integrityScores,
	kills,
	serverLive
} from '$lib/server/db/schema';
import { getIntegrityRules } from '$lib/server/integrity/rules';
import { weaponOverrides } from '$lib/server/integrity/weapon-map';
import { liveInfantryMetrics } from '$lib/server/integrity/live';
import { shadowComparison } from '$lib/server/integrity/baselines';
import { summarizeCommitteeShadow } from '$lib/server/integrity/shadow-dashboard';
import type { Player, Status } from '$lib/types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { server, user } = await requireServerCap(env, locals, params.id, 'integrity.view');
		const now = new Date();
		const [cases, scores, reports, actions, [live], rules, recentKills, liveScores, mappings] =
			await Promise.all([
				env.db
					.select({
						id: integrityCases.id,
						steamId: integrityCases.steamId,
						createdAt: integrityCases.createdAt,
						status: integrityCases.status,
						confidence: integrityCases.confidence,
						riskScore: integrityCases.riskScore,
						riskBreakdown: integrityCases.riskBreakdown,
						statistical: integrityCases.statistical,
						snapshot: integrityCases.snapshot,
						trigger: integrityCases.trigger
					})
					.from(integrityCases)
					.where(eq(integrityCases.serverId, server.id))
					.orderBy(desc(integrityCases.createdAt))
					.limit(50),
				env.db
					.select({
						id: integrityScores.id,
						steamId: integrityScores.steamId,
						scoredAt: integrityScores.scoredAt,
						score: integrityScores.score,
						level: integrityScores.level,
						breakdown: integrityScores.breakdown,
						statistical: integrityScores.statistical,
						ruleVersion: integrityScores.ruleVersion
					})
					.from(integrityScores)
					.where(eq(integrityScores.serverId, server.id))
					.orderBy(desc(integrityScores.scoredAt))
					.limit(50),
				env.db
					.select({
						id: integrityReports.id,
						targetSteamId: integrityReports.targetSteamId,
						reason: integrityReports.reason,
						createdAt: integrityReports.createdAt,
						status: integrityReports.status
					})
					.from(integrityReports)
					.where(eq(integrityReports.serverId, server.id))
					.orderBy(desc(integrityReports.createdAt))
					.limit(50),
				env.db
					.select()
					.from(integrityActions)
					.where(eq(integrityActions.serverId, server.id))
					.orderBy(desc(integrityActions.createdAt))
					.limit(50),
				env.db
					.select({
						feedAt: serverLive.feedAt,
						statusAt: serverLive.statusAt,
						playersAt: serverLive.playersAt,
						status: serverLive.status,
						players: serverLive.players
					})
					.from(serverLive)
					.where(eq(serverLive.serverId, server.id))
					.limit(1),
				getIntegrityRules(env, server.orgId),
				env.db
					.select()
					.from(kills)
					.where(
						and(eq(kills.serverId, server.id), gte(kills.ts, new Date(now.getTime() - 10 * 60_000)))
					)
					.orderBy(desc(kills.ts))
					.limit(3001),
				env.db
					.select({
						steamId: integrityScores.steamId,
						score: integrityScores.score,
						level: integrityScores.level,
						breakdown: integrityScores.breakdown,
						scoredAt: integrityScores.scoredAt
					})
					.from(integrityScores)
					.where(
						and(
							eq(integrityScores.serverId, server.id),
							gte(integrityScores.scoredAt, new Date(now.getTime() - 15 * 60_000))
						)
					)
					.orderBy(desc(integrityScores.score), desc(integrityScores.scoredAt))
					.limit(1000),
				weaponOverrides(env, server.orgId)
			]);
		const status = live?.status && typeof live.status === 'object' ? (live.status as Status) : null;
		const roster = Array.isArray(live?.players) ? (live.players as Player[]) : [];
		const recentScore = new Map<string, (typeof liveScores)[number]>();
		for (const row of liveScores)
			if (!recentScore.has(row.steamId)) recentScore.set(row.steamId, row);
		const currentRisks = await loadCurrentRisks(
			env,
			server.orgId,
			roster.map((p) => p.steamId),
			recentScore,
			rules.config,
			now
		);
		const infantry = liveInfantryMetrics(recentKills.slice(0, 3000), status, mappings);
		const latestKill = recentKills[0];
		const feedMetricsAvailable =
			!!latestKill &&
			(!status?.map || mapId(status.map) === mapId(latestKill.map)) &&
			(status?.matchSeconds == null || status.matchSeconds >= latestKill.eventTime - 5);
		const onlinePlayers = roster
			.filter((player) => /^\d{17}$/.test(player.steamId))
			.map((player) => ({
				steamId: player.steamId,
				name: player.name,
				kills: player.kills,
				deaths: player.deaths,
				infantry: infantry.get(player.steamId) ?? null,
				riskScore: currentRisks.get(player.steamId)?.score ?? null,
				riskLevel: currentRisks.get(player.steamId)?.level ?? null,
				riskBreakdown: currentRisks.get(player.steamId)?.breakdown ?? []
			}))
			.sort((a, b) => (b.riskScore ?? -1) - (a.riskScore ?? -1));
		const dryRun = await Promise.all(
			([24, 72, 168] as const).map(async (hours) => {
				const [row] = await env.db
					.select({
						windows: sql<number>`COUNT(DISTINCT ${integrityScores.windowId})`,
						koPlayers: sql<number>`COUNT(DISTINCT ${integrityScores.steamId}) FILTER (WHERE ${integrityScores.score} >= ${rules.config.koThreshold} AND ${integrityScores.currentBehaviorAnomaly})`,
						quarantinePlayers: sql<number>`COUNT(DISTINCT ${integrityScores.steamId}) FILTER (WHERE ${integrityScores.score} >= ${rules.config.quarantineThreshold} AND ${integrityScores.currentBehaviorAnomaly})`
					})
					.from(integrityScores)
					.where(
						and(
							eq(integrityScores.serverId, server.id),
							eq(integrityScores.source, 'window'),
							gte(integrityScores.scoredAt, new Date(Date.now() - hours * 60 * 60_000))
						)
					);
				return {
					hours,
					windows: Number(row?.windows ?? 0),
					koPlayers: Number(row?.koPlayers ?? 0),
					quarantinePlayers: Number(row?.quarantinePlayers ?? 0)
				};
			})
		);
		const contributorRows = (await env.db.execute(sql`
			SELECT part.item->>'code' AS code,
			       SUM(CASE WHEN part.item->>'points' ~ '^-?[0-9]{1,8}$'
			           THEN (part.item->>'points')::integer ELSE 0 END) AS points
			FROM integrity_scores AS s
			CROSS JOIN LATERAL jsonb_array_elements(
				CASE WHEN jsonb_typeof(s.breakdown) = 'array' THEN s.breakdown ELSE '[]'::jsonb END
			) AS part(item)
			WHERE s.server_id = ${server.id}
			  AND s.source = 'window'
			  AND s.scored_at >= ${new Date(Date.now() - 7 * 24 * 60 * 60_000)}
			GROUP BY part.item->>'code'
			ORDER BY points DESC
			LIMIT 5
		`)) as { code: string; points: number }[];
		const canConfigure = (await orgRoleFor(env, user, server.orgId)) === 'owner';
		const comparison = await shadowComparison(env, server.orgId, rules.config.koThreshold);
		const [shadowRows, labelRows] = await Promise.all([
			env.db
				.select({ windowId: integrityScores.windowId, statistical: integrityScores.statistical })
				.from(integrityScores)
				.where(
					and(
						eq(integrityScores.serverId, server.id),
						eq(integrityScores.source, 'window'),
						gte(integrityScores.scoredAt, new Date(now.getTime() - 30 * 86_400_000))
					)
				)
				.orderBy(desc(integrityScores.scoredAt), desc(integrityScores.id))
				.limit(5001),
			env.db
				.select({
					caseId: integrityLabels.caseId,
					label: integrityLabels.label,
					reason: integrityLabels.reason,
					statistical: integrityCases.statistical,
					createdAt: integrityLabels.createdAt
				})
				.from(integrityLabels)
				.innerJoin(integrityCases, eq(integrityCases.id, integrityLabels.caseId))
				.where(
					and(
						eq(integrityCases.serverId, server.id),
						gte(integrityLabels.createdAt, new Date(now.getTime() - 30 * 86_400_000))
					)
				)
				.orderBy(desc(integrityLabels.createdAt))
				.limit(5001)
		]);
		const committeeShadow = summarizeCommitteeShadow(
			shadowRows.slice(0, 5000),
			labelRows.slice(0, 5000),
			shadowRows.length > 5000 || labelRows.length > 5000
		);
		return {
			cases: cases.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			scores: scores.map((item) => ({ ...item, scoredAt: item.scoredAt.toISOString() })),
			reports: reports.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			actions: actions.map((item) => ({
				...item,
				createdAt: item.createdAt.toISOString(),
				effectiveAt: item.effectiveAt?.toISOString() ?? null,
				expiresAt: item.expiresAt?.toISOString() ?? null,
				revertedAt: item.revertedAt?.toISOString() ?? null
			})),
			feedAt: live?.feedAt?.toISOString() ?? null,
			feedConfigured: !!server.feedTokenHash,
			playersAt: live?.playersAt?.toISOString() ?? null,
			feedRowsTruncated: recentKills.length > 3000,
			feedMetricsAvailable,
			onlinePlayers,
			ruleVersion: rules.version,
			assessmentMode: rules.assessmentMode,
			comparison,
			committeeShadow,
			labels: labelRows.slice(0, 5000).map((row) => ({
				caseId: row.caseId,
				label: row.label,
				reason: row.reason,
				createdAt: row.createdAt.toISOString()
			})),
			kpmBands: rules.config.kpmBands,
			mode: rules.enforcement.autoSuspendedAt
				? 'suspended'
				: rules.enforcement.autoKickEnabled ||
					  rules.enforcement.autoQuarantine24hEnabled ||
					  rules.enforcement.autoQuarantine7dEnabled
					? 'experimental'
					: 'dry_run',
			dryRun,
			contributors: contributorRows.map((row) => ({ code: row.code, points: Number(row.points) })),
			canConfigure,
			orgIntegrityUrl: canConfigure ? `/orgs/${encodeURIComponent(server.orgId)}/integrity` : null
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
