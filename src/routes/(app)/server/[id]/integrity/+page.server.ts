import { historyPolicy } from '$lib/server/integrity/history-retention';
import { retainedCase } from '$lib/server/integrity/case-retention';
import {
	committeeEnabled,
	legacyEnabled,
	shortModelEnabled,
	longModelEnabled
} from '$lib/integrity-engines';
import { modelRuns } from '$lib/server/integrity/model-runtime';
import { integrityActionPage } from '$lib/server/integrity/action-history';
import { loadDistributionDashboard } from '$lib/server/integrity/distribution-dashboard';
import { loadShortRisk } from '$lib/server/integrity/short-risk';
import { COMMITTEE_VOTING_VERSION } from '$lib/server/integrity/committee';
import { aiJobViews } from '$lib/server/integrity/ai-queue';
import { aiSettings } from '$lib/server/integrity/ai';
import { loadCurrentRisks } from '$lib/server/integrity/current-risk';
import { error } from '@sveltejs/kit';
import { mapId } from '$lib/format';
import { and, desc, eq, gte, inArray, sql } from 'drizzle-orm';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { orgRoleFor, requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import {
	integrityCases,
	steamProfiles,
	integrityLabels,
	integrityActions,
	outbox,
	listEntries,
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
import { STATISTICAL_MODEL_CONFIG } from '$lib/server/integrity/statistical-config';
import type { Player, Status } from '$lib/types';

export const load: PageServerLoad = async ({ locals, params, url }) => {
	const env = getEnv();
	try {
		const { server, user } = await requireServerCap(env, locals, params.id, 'integrity.view');
		const now = new Date();
		const retention = await historyPolicy(env, server.id);
		const history = await integrityActionPage(env, server.id, {
			page: Number(url.searchParams.get('actionsPage') ?? 1),
			pageSize: retention.policy.pageSize,
			filter: url.searchParams.get('actionsFilter') ?? 'all',
			before: url.searchParams.has('actionsBefore')
				? new Date(url.searchParams.get('actionsBefore')!)
				: undefined
		});
		const rules = await getIntegrityRules(env, server.orgId),
			legacy = legacyEnabled(rules.assessmentMode),
			committee = committeeEnabled(rules.assessmentMode);
		const [cases, scores, reports, [live], recentKills, liveScores, mappings] = await Promise.all([
			env.db
				.select({
					name: steamProfiles.persona,
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
				.leftJoin(steamProfiles, eq(steamProfiles.steamId, integrityCases.steamId))
				.where(
					and(
						eq(integrityCases.serverId, server.id),
						retainedCase,
						sql`${integrityCases.status} NOT IN ('AI_ARCHIVED', 'AI_CLEARED')`
					)
				)
				.orderBy(desc(integrityCases.createdAt), desc(integrityCases.id))
				.limit(5),
			legacy || committee
				? env.db
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
						.limit(50)
				: Promise.resolve([]),
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
			legacy
				? env.db
						.select()
						.from(kills)
						.where(
							and(
								eq(kills.serverId, server.id),
								gte(kills.ts, new Date(now.getTime() - 10 * 60_000))
							)
						)
						.orderBy(desc(kills.ts))
						.limit(3001)
				: Promise.resolve([]),
			legacy
				? env.db
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
						.limit(1000)
				: Promise.resolve([]),
			legacy ? weaponOverrides(env, server.orgId) : Promise.resolve(new Map())
		]);
		const status = live?.status && typeof live.status === 'object' ? (live.status as Status) : null;
		const roster = Array.isArray(live?.players) ? (live.players as Player[]) : [];
		const recentScore = new Map<string, (typeof liveScores)[number]>();
		for (const row of liveScores)
			if (!recentScore.has(row.steamId)) recentScore.set(row.steamId, row);
		const currentRisks = legacy
			? await loadCurrentRisks(
					env,
					server.orgId,
					roster.map((p) => p.steamId),
					recentScore,
					rules.config,
					now
				)
			: new Map();
		const infantry = liveInfantryMetrics(recentKills.slice(0, 3000), status, mappings);
		const latestKill = recentKills[0];
		const feedMetricsAvailable =
			!!latestKill &&
			(!status?.map || mapId(status.map) === mapId(latestKill.map)) &&
			(status?.matchSeconds == null || status.matchSeconds >= latestKill.eventTime - 5);
		const onlinePlayers = (legacy ? roster : [])
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
		const caseReviews = cases.length
			? await env.db
					.selectDistinctOn([integrityLabels.caseId], {
						caseId: integrityLabels.caseId,
						label: integrityLabels.label,
						reason: integrityLabels.reason,
						createdAt: integrityLabels.createdAt
					})
					.from(integrityLabels)
					.where(
						and(
							eq(integrityLabels.orgId, server.orgId),
							inArray(
								integrityLabels.caseId,
								cases.map((c) => c.id)
							)
						)
					)
					.orderBy(
						integrityLabels.caseId,
						desc(integrityLabels.createdAt),
						desc(integrityLabels.id)
					)
			: [];
		const canConfigure = (await orgRoleFor(env, user, server.orgId)) === 'owner';
		const comparison = committee
			? await shadowComparison(env, server.orgId, rules.config.koThreshold)
			: {
					total: 0,
					normalNormal: 0,
					normalAbnormal: 0,
					abnormalNormal: 0,
					abnormalAbnormal: 0,
					since: null
				};
		const [shadowRows, labelRows] = await Promise.all([
			committee
				? env.db
						.select({
							windowId: integrityScores.windowId,
							statistical: integrityScores.statistical
						})
						.from(integrityScores)
						.where(
							and(
								eq(integrityScores.serverId, server.id),
								eq(integrityScores.source, 'window'),
								sql`${integrityScores.statistical}->>'modelVersion' = ${STATISTICAL_MODEL_CONFIG.modelVersion}`,
								sql`${integrityScores.statistical}->'committee'->>'votingVersion' = ${COMMITTEE_VOTING_VERSION}`,
								gte(integrityScores.scoredAt, new Date(now.getTime() - 30 * 86_400_000))
							)
						)
						.orderBy(desc(integrityScores.scoredAt), desc(integrityScores.id))
						.limit(5001)
				: Promise.resolve([]),
			committee
				? env.db
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
								retainedCase,
								sql`${integrityCases.statistical}->>'modelVersion' = ${STATISTICAL_MODEL_CONFIG.modelVersion}`,
								sql`${integrityCases.statistical}->'committee'->>'votingVersion' = ${COMMITTEE_VOTING_VERSION}`,
								gte(integrityLabels.createdAt, new Date(now.getTime() - 30 * 86_400_000))
							)
						)
						.orderBy(desc(integrityLabels.createdAt))
						.limit(5001)
				: Promise.resolve([])
		]);

		const reviewCaseIds = [
			...new Set([
				...cases.map((c) => c.id),
				...history.rows.flatMap((a) => (a.source === 'REVIEW' && a.caseId ? [a.caseId] : []))
			])
		];
		const reviewPenalties = reviewCaseIds.length
			? await env.db
					.select({ action: integrityActions, entry: listEntries })
					.from(integrityActions)
					.leftJoin(listEntries, eq(listEntries.id, integrityActions.listEntryId))
					.where(
						and(
							eq(integrityActions.serverId, server.id),
							eq(integrityActions.source, 'REVIEW'),
							inArray(integrityActions.caseId, reviewCaseIds)
						)
					)
			: [];
		const committeeShadow = summarizeCommitteeShadow(
			shadowRows.slice(0, 5000),
			labelRows.slice(0, 5000),
			shadowRows.length > 5000 || labelRows.length > 5000
		);
		return {
			longModelResults: longModelEnabled(rules.assessmentMode)
				? await modelRuns(env, server.orgId, server.id)
				: [],
			distributions: committee
				? await loadDistributionDashboard(env, server.orgId, server.id)
				: await import('$lib/server/integrity/distribution-dashboard').then((m) =>
						m.emptyDistributionDashboard()
					),
			shortRisk: shortModelEnabled(rules.assessmentMode)
				? await loadShortRisk(env, server.id)
				: null,
			aiJobs: committee
				? await aiJobViews(
						env,
						cases.map((c) => c.id)
					)
				: [],
			aiAutoEnabled: committee && !!(await aiSettings(env, server.orgId))?.autoEnabled,
			reviewPenalties: reviewPenalties.map(({ action, entry }) => ({
				caseId: action.caseId,
				id: action.id,
				deliveryState: action.deliveryState,
				expiresAt: entry?.expiresAt?.toISOString() ?? null,
				active:
					!!entry &&
					!entry.removedAt &&
					!action.revertedAt &&
					(!entry.expiresAt || entry.expiresAt > now)
			})),
			cases: cases.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			scores: scores.map((item) => ({ ...item, scoredAt: item.scoredAt.toISOString() })),
			reports: reports.map((item) => ({ ...item, createdAt: item.createdAt.toISOString() })),
			actions: history.rows,
			actionsPagination: {
				page: history.page,
				pages: history.pages,
				total: history.total,
				pageSize: history.pageSize,
				filter: history.filter,
				before: history.before
			},
			historyPolicy: retention.policy,
			historyPolicyRevision: retention.revision,
			historyLastCleanup: retention.lastCleanup,
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
			labels: caseReviews.map((row) => ({
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
			canConfigure,
			orgIntegrityUrl: canConfigure ? `/orgs/${encodeURIComponent(server.orgId)}/integrity` : null
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
