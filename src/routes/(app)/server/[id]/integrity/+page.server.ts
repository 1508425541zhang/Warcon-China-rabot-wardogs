import { error } from '@sveltejs/kit';
import { desc, eq } from 'drizzle-orm';
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
		return {
			cases: cases.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			scores: scores.map((item) => ({ ...item, scoredAt: item.scoredAt.toISOString() })),
			reports: reports.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			feedAt: live?.feedAt?.toISOString() ?? null,
			ruleVersion: rules.version,
			mode: rules.config.mode
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
