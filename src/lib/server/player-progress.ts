import { and, desc, eq, isNull, sql } from 'drizzle-orm';
import type { DbOrTx } from './db';
import type { Env } from './env';
import type { Player } from '$lib/types';
import { matches, playerProgressSamples } from './db/schema';
import { cashProgress, type CashObservation } from '$lib/player-progress';
export async function recordPlayerProgress(
	db: DbOrTx,
	serverId: string,
	players: Player[],
	observedAt: Date
) {
	const [match] = await db
		.select({ id: matches.id })
		.from(matches)
		.where(and(eq(matches.serverId, serverId), isNull(matches.endedAt)))
		.orderBy(desc(matches.id))
		.limit(1);
	if (!match || !players.length) return;
	await db
		.insert(playerProgressSamples)
		.values({
			serverId,
			matchId: match.id,
			bucket: Math.floor(observedAt.getTime() / 30000),
			observedAt,
			players: players
				.filter((p) => /^\d{17}$/.test(p.steamId) && Number.isFinite(p.cash))
				.map((p) => ({ steamId: p.steamId, cash: p.cash }))
		})
		.onConflictDoNothing();
}
export async function loadPlayerProgress(env: Env, serverId: string, steamId: string) {
	const rounds = await env.db.execute(sql`
 SELECT m.id,m.map,m.started_at AS "startedAt",m.ended_at AS "endedAt"
 FROM matches m WHERE m.server_id=${serverId} AND m.started_at>now()-interval '30 days'
 AND EXISTS(SELECT 1 FROM player_progress_samples p WHERE p.match_id=m.id AND p.players @> jsonb_build_array(jsonb_build_object('steamId', ${steamId}::text)))
 ORDER BY m.id DESC LIMIT 10`);
	const result = [];
	for (const round of rounds as unknown as {
		id: number;
		map: string;
		startedAt: Date;
		endedAt: Date | null;
	}[]) {
		const samples = await env.db
			.select()
			.from(playerProgressSamples)
			.where(eq(playerProgressSamples.matchId, Number(round.id)))
			.orderBy(playerProgressSamples.observedAt)
			.limit(6000);
		const observations: CashObservation[] = samples.map((s) => ({
			observedAt: s.observedAt.toISOString(),
			players: Array.isArray(s.players) ? (s.players as CashObservation['players']) : []
		}));
		result.push({
			id: Number(round.id),
			map: round.map,
			startedAt: new Date(round.startedAt).toISOString(),
			endedAt: round.endedAt ? new Date(round.endedAt).toISOString() : null,
			truncated: samples.length === 6000,
			points: cashProgress(observations, steamId, new Date(round.startedAt).toISOString())
		});
	}
	return result;
}
