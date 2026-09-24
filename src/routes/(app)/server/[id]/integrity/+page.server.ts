import { error } from '@sveltejs/kit';
import { and, desc, eq, gte, sql } from 'drizzle-orm';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import {
	integrityCases,
	integrityReports,
	integrityScores,
	serverLive
} from '$lib/server/db/schema';
import { getIntegrityRules } from '$lib/server/integrity/rules';

export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { server } = await requireServerCap(env, locals, params.id, 'integrity.view');
		const [cases, scores, reports, [live], rules] = await Promise.all([
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
				.select({ feedAt: serverLive.feedAt })
				.from(serverLive)
				.where(eq(serverLive.serverId, server.id))
				.limit(1),
			getIntegrityRules(env, server.orgId)
		]);
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
		return {
			cases: cases.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			scores: scores.map((item) => ({ ...item, scoredAt: item.scoredAt.toISOString() })),
			reports: reports.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			feedAt: live?.feedAt?.toISOString() ?? null,
			ruleVersion: rules.version,
			mode: rules.config.mode,
			dryRun,
			contributors: contributorRows.map((row) => ({ code: row.code, points: Number(row.points) }))
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
