import { and, eq, gte, inArray, sql } from 'drizzle-orm';
import type { Env } from '../env';
import { integrityReports, steamProfiles } from '../db/schema';
import { steamBanSignals } from './steam-signals';
import {
	integrityLevel,
	scoreIntegrity,
	type IntegrityRuleConfig,
	type RiskComponent
} from './score';

type SavedRisk = { score: number; level: string; breakdown: unknown };

/** Read-time assessment only. Never creates enforcement evidence or actions. */
export function currentRisk(
	saved: SavedRisk | undefined,
	account: ReturnType<typeof steamBanSignals>,
	reporters: number,
	config: IntegrityRuleConfig
) {
	const independent = scoreIntegrity(
		{
			behaviorReasons: [],
			kpm180: 0,
			uniqueVictims: 0,
			previousKpm: [],
			uniqueReporters: reporters,
			repeatHighRiskWindow: false,
			infantryKills: 0,
			headshots: 0,
			penetrations: 0,
			burstPoints: 0,
			...account,
			wardogsPlaytimeHours: null
		},
		config
	);
	if (!saved && !account.known && reporters === 0) return null;
	// Historic malformed evidence cannot be safely decomposed; preserve its recorded score.
	if (saved && !Array.isArray(saved.breakdown))
		return { ...saved, breakdown: [] as RiskComponent[] };
	const behavior = ((saved?.breakdown ?? []) as RiskComponent[]).filter(
		(part) =>
			part &&
			typeof part.code === 'string' &&
			Number.isFinite(part.points) &&
			part.points >= 0 &&
			part.code !== 'steam_ban_prior' &&
			part.code !== 'unique_reports'
	);
	const breakdown = [...behavior, ...independent.breakdown];
	const score = Math.min(
		100,
		breakdown.reduce((sum, part) => sum + part.points, 0)
	);
	return { score, level: integrityLevel(score, config), breakdown };
}

/** Batch cached data; rendering never waits for an external Steam request. */
export async function loadCurrentRisks(
	env: Env,
	orgId: string,
	steamIds: string[],
	saved: Map<string, SavedRisk>,
	config: IntegrityRuleConfig,
	now = new Date()
) {
	const ids = [...new Set(steamIds)];
	if (!ids.length) return new Map<string, ReturnType<typeof currentRisk>>();
	const [profiles, reports] = await Promise.all([
		env.db.select().from(steamProfiles).where(inArray(steamProfiles.steamId, ids)),
		env.db
			.select({
				steamId: integrityReports.targetSteamId,
				count: sql<number>`COUNT(DISTINCT ${integrityReports.reporterSteamId})`
			})
			.from(integrityReports)
			.where(
				and(
					eq(integrityReports.orgId, orgId),
					inArray(integrityReports.targetSteamId, ids),
					gte(integrityReports.createdAt, new Date(now.getTime() - 86400000))
				)
			)
			.groupBy(integrityReports.targetSteamId)
	]);
	const byId = new Map(profiles.map((row) => [row.steamId, row]));
	const counts = new Map(reports.map((row) => [row.steamId, Number(row.count)]));
	return new Map(
		ids.map((id) => [
			id,
			currentRisk(saved.get(id), steamBanSignals(byId.get(id), now), counts.get(id) ?? 0, config)
		])
	);
}
