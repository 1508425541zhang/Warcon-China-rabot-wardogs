import { error } from '@sveltejs/kit';
import { and, desc, eq, gte, sql } from 'drizzle-orm';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { orgRoleFor, requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import {
	integrityCases,
	integrityReports,
	integrityScores,
	kills,
	serverLive
} from '$lib/server/db/schema';
import { getIntegrityRules } from '$lib/server/integrity/rules';
import { weaponMappings, weaponOverrides } from '$lib/server/integrity/weapon-map';
import { DEFAULT_WEAPON_MAP, WEAPON_CATEGORIES } from '$lib/server/integrity/weapons';
import { liveInfantryMetrics } from '$lib/server/integrity/live';
import type { Player, Status } from '$lib/types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { server, user } = await requireServerCap(env, locals, params.id, 'integrity.view');
		const now = new Date();
		const [cases, scores, reports, [live], rules, recentKills, liveScores, mappings] =
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
		const infantry = liveInfantryMetrics(recentKills.slice(0, 3000), status, mappings);
		const onlinePlayers = roster
			.filter((player) => /^\d{17}$/.test(player.steamId))
			.map((player) => ({
				steamId: player.steamId,
				name: player.name,
				kills: player.kills,
				deaths: player.deaths,
				infantry: infantry.get(player.steamId) ?? null,
				riskScore: recentScore.get(player.steamId)?.score ?? null,
				riskLevel: recentScore.get(player.steamId)?.level ?? null,
				riskBreakdown: recentScore.get(player.steamId)?.breakdown ?? []
			}))
			.sort((a, b) => (b.riskScore ?? -1) - (a.riskScore ?? -1));
		const dryRun = await Promise.all(
			([24, 72, 168] as const).map(async (hours) => {
				const [row] = await env.db
					.select({
						windows: sql<number>`COUNT(*)`,
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
			       SUM((part.item->>'points')::integer)::integer AS points
			FROM integrity_scores AS s
			CROSS JOIN LATERAL jsonb_array_elements(s.breakdown) AS part(item)
			WHERE s.server_id = ${server.id}
			  AND s.source = 'window'
			  AND s.scored_at >= ${new Date(Date.now() - 7 * 24 * 60 * 60_000)}
			GROUP BY part.item->>'code'
			ORDER BY points DESC
			LIMIT 5
		`)) as { code: string; points: number }[];
		const canConfigure = (await orgRoleFor(env, user, server.orgId)) === 'owner';
		const overrides = canConfigure ? await weaponMappings(env, server.orgId) : [];
		return {
			cases: cases.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			scores: scores.map((item) => ({ ...item, scoredAt: item.scoredAt.toISOString() })),
			reports: reports.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			feedAt: live?.feedAt?.toISOString() ?? null,
			playersAt: live?.playersAt?.toISOString() ?? null,
			feedRowsTruncated: recentKills.length > 3000,
			onlinePlayers,
			ruleVersion: rules.version,
			kpmBands: rules.config.kpmBands,
			mode: rules.config.mode,
			dryRun,
			contributors: contributorRows.map((row) => ({ code: row.code, points: Number(row.points) })),
			canConfigure,
			ruleConfig: canConfigure ? rules.config : null,
			weaponOverrides: overrides.map((row) => ({ cause: row.cause, category: row.category })),
			weaponDefaults: canConfigure ? DEFAULT_WEAPON_MAP : {},
			weaponCategories: canConfigure ? WEAPON_CATEGORIES : []
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
