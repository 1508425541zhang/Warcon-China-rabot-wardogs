import { and, desc, eq, gte, sql } from 'drizzle-orm';
import type { Env } from '../env';
import type { ServerRow } from '../db/schema';
import {
	integrityProfiles,
	integrityReports,
	integrityScores,
	integrityWindows,
	kills,
	serverLive
} from '../db/schema';
import { weaponOverrides } from './weapon-map';
import { liveInfantryMetrics } from './live';
import type { Player, Status } from '$lib/types';

/** Optional dossier section, isolated from the legacy dossier and career loaders. */
export async function loadPlayerIntegrity(env: Env, server: ServerRow, steamId: string) {
	const now = new Date();
	const [[profile], [risk], [window], [reports], [live], recentKills, mappings] = await Promise.all(
		[
			env.db
				.select()
				.from(integrityProfiles)
				.where(
					and(eq(integrityProfiles.orgId, server.orgId), eq(integrityProfiles.steamId, steamId))
				)
				.limit(1),
			env.db
				.select()
				.from(integrityScores)
				.where(
					and(
						eq(integrityScores.serverId, server.id),
						eq(integrityScores.steamId, steamId),
						gte(integrityScores.scoredAt, new Date(now.getTime() - 15 * 60_000))
					)
				)
				.orderBy(desc(integrityScores.score), desc(integrityScores.scoredAt))
				.limit(1),
			env.db
				.select()
				.from(integrityWindows)
				.where(and(eq(integrityWindows.serverId, server.id), eq(integrityWindows.steamId, steamId)))
				.orderBy(desc(integrityWindows.observedAt))
				.limit(1),
			env.db
				.select({ count: sql<number>`COUNT(DISTINCT ${integrityReports.reporterSteamId})` })
				.from(integrityReports)
				.where(
					and(
						eq(integrityReports.orgId, server.orgId),
						eq(integrityReports.targetSteamId, steamId),
						gte(integrityReports.createdAt, new Date(now.getTime() - 24 * 60 * 60_000))
					)
				),
			env.db
				.select({
					status: serverLive.status,
					players: serverLive.players,
					feedAt: serverLive.feedAt
				})
				.from(serverLive)
				.where(eq(serverLive.serverId, server.id))
				.limit(1),
			env.db
				.select()
				.from(kills)
				.where(
					and(
						eq(kills.serverId, server.id),
						eq(kills.killerSteamId, steamId),
						gte(kills.ts, new Date(now.getTime() - 10 * 60_000))
					)
				)
				.orderBy(desc(kills.ts))
				.limit(1001),
			weaponOverrides(env, server.orgId)
		]
	);
	const status = live?.status && typeof live.status === 'object' ? (live.status as Status) : null;
	const roster = Array.isArray(live?.players) ? (live.players as Player[]) : [];
	const current = roster.find((player) => player.steamId === steamId) ?? null;
	const metrics =
		recentKills.length <= 1000
			? (liveInfantryMetrics(recentKills, status, mappings).get(steamId) ?? null)
			: null;
	const latestKill = recentKills[0];
	return {
		aliases: profile && Array.isArray(profile.aliases) ? (profile.aliases as string[]) : [],
		firstSeen: profile?.firstSeen.toISOString() ?? null,
		lastSeen: profile?.lastSeen.toISOString() ?? null,
		riskScore: risk?.score ?? null,
		riskLevel: risk?.level ?? null,
		breakdown: risk?.breakdown ?? [],
		latestWindow: window
			? {
					kpm180: window.kpm180,
					infantryKills: window.infantryKills,
					uniqueVictims: window.uniqueVictims,
					observedAt: window.observedAt.toISOString()
				}
			: null,
		reports24h: Number(reports?.count ?? 0),
		current: current ? { kills: current.kills, deaths: current.deaths } : null,
		metrics,
		metricsAvailable:
			!!server.feedTokenHash &&
			!!live?.feedAt &&
			Date.now() - live.feedAt.getTime() < 5 * 60_000 &&
			recentKills.length <= 1000 &&
			!!latestKill &&
			(!status?.map || status.map === latestKill.map) &&
			(status?.matchSeconds == null || status.matchSeconds >= latestKill.eventTime - 5) &&
			metrics?.reliable !== false
	};
}
