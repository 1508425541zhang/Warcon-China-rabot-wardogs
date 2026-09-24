import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { accessibleServers, requireServerCap } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import { dossier } from '$lib/server/players';
import { loadCareer } from '$lib/server/leaderboards';
import { and, desc, eq, gte, sql } from 'drizzle-orm';
import {
	integrityProfiles,
	integrityReports,
	integrityScores,
	integrityWindows,
	kills,
	serverLive
} from '$lib/server/db/schema';
import { weaponOverrides } from '$lib/server/integrity/weapon-map';
import { liveInfantryMetrics } from '$lib/server/integrity/live';
import type { Player, Status } from '$lib/types';

/**
 * Checked here as well as in the server layout: a page's data can be asked for without its layouts
 * (SvelteKit's __data.json), so the layout's refusal protects nothing below it — and a dossier is
 * the most sensitive thing on the server. The sibling tabs guard the same way.
 */
export const load: PageServerLoad = async ({ locals, params }) => {
	const env = getEnv();
	try {
		const { server, access, user } = await requireServerCap(env, locals, params.id, 'server.view');
		if (!/^\d{17}$/.test(params.steamId)) error(404, 'Not a SteamID64.');
		const visible = (await accessibleServers(env, user, server.orgId)).filter(
			(s) => s.orgId === server.orgId
		);
		const [d, career] = await Promise.all([
			dossier(env, user, server, access, params.steamId),
			loadCareer(env, {
				serverId: server.id,
				ids: visible.map((s) => s.id),
				nameOf: new Map(visible.map((s) => [s.id, s.name])),
				steamId: params.steamId
			})
		]);
		let integrity = null;
		if (access.caps.has('integrity.view')) {
			const now = new Date();
			const [[profile], [risk], [window], [reports], [live], recentKills, mappings] =
				await Promise.all([
					env.db
						.select()
						.from(integrityProfiles)
						.where(
							and(
								eq(integrityProfiles.orgId, server.orgId),
								eq(integrityProfiles.steamId, params.steamId)
							)
						)
						.limit(1),
					env.db
						.select()
						.from(integrityScores)
						.where(
							and(
								eq(integrityScores.serverId, server.id),
								eq(integrityScores.steamId, params.steamId),
								gte(integrityScores.scoredAt, new Date(now.getTime() - 15 * 60_000))
							)
						)
						.orderBy(desc(integrityScores.score), desc(integrityScores.scoredAt))
						.limit(1),
					env.db
						.select()
						.from(integrityWindows)
						.where(
							and(
								eq(integrityWindows.serverId, server.id),
								eq(integrityWindows.steamId, params.steamId)
							)
						)
						.orderBy(desc(integrityWindows.observedAt))
						.limit(1),
					env.db
						.select({ count: sql<number>`COUNT(DISTINCT ${integrityReports.reporterSteamId})` })
						.from(integrityReports)
						.where(
							and(
								eq(integrityReports.orgId, server.orgId),
								eq(integrityReports.targetSteamId, params.steamId),
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
								eq(kills.killerSteamId, params.steamId),
								gte(kills.ts, new Date(now.getTime() - 10 * 60_000))
							)
						)
						.orderBy(desc(kills.ts))
						.limit(1001),
					weaponOverrides(env, server.orgId)
				]);
			const status =
				live?.status && typeof live.status === 'object' ? (live.status as Status) : null;
			const roster = Array.isArray(live?.players) ? (live.players as Player[]) : [];
			const current = roster.find((player) => player.steamId === params.steamId) ?? null;
			const metrics =
				recentKills.length <= 1000
					? (liveInfantryMetrics(recentKills, status, mappings).get(params.steamId) ?? null)
					: null;
			integrity = {
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
					!!live?.feedAt &&
					Date.now() - live.feedAt.getTime() < 5 * 60_000 &&
					recentKills.length <= 1000
			};
		}
		return { dossier: d, career, multiServer: visible.length > 1, integrity };
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
