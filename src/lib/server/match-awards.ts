import { randomUUID } from 'node:crypto';
import { and, desc, eq, isNull, sql } from 'drizzle-orm';
import type { Env } from './env';
import { matches, matchPlayers, playerProgressSamples } from './db/schema';
import type { AwardLine } from '$lib/match-awards-policy';
import { feedRecord, type FeedKill } from './match-players';
/** Called at the old-round boundary, before reconcileMatch opens the new round. */
export async function loadAwardLines(
	env: Env,
	serverId: string,
	map: string,
	memory: AwardLine[]
): Promise<AwardLine[]> {
	const [round] = await env.db
		.select()
		.from(matches)
		.where(and(eq(matches.serverId, serverId), isNull(matches.endedAt)))
		.orderBy(desc(matches.id))
		.limit(1);
	if (!round || round.map !== map) return [];
	if (round.awardSnapshot) return round.awardSnapshot as AwardLine[];
	const [saved, progress, events] = await Promise.all([
		env.db.select().from(matchPlayers).where(eq(matchPlayers.matchId, round.id)),
		env.db
			.select()
			.from(playerProgressSamples)
			.where(
				and(
					eq(playerProgressSamples.serverId, serverId),
					eq(playerProgressSamples.matchId, round.id)
				)
			)
			.orderBy(desc(playerProgressSamples.observedAt))
			.limit(1),
		env.db.execute<
			FeedKill &
				Record<string, unknown> & { eventTime: number; instanceId: string; eventId: string }
		>(sql`
		 SELECT killer_steam_id AS "killerSteamId", victim_steam_id AS "victimSteamId", headshot, suicide,
		 team_kill AS "teamKill", cause, distance_m AS "distanceM", event_time AS "eventTime", instance_id AS "instanceId", event_id AS "eventId"
		 FROM kills WHERE server_id=${serverId} AND match_row=${round.id} AND ts>=${new Date(round.startedAt.getTime() - 120000)} ORDER BY event_time, ts, event_id`)
	]);
	const rows = new Map<string, AwardLine>();
	for (const p of saved) rows.set(p.steamId, p);
	for (const p of memory) if (p.steamId) rows.set(p.steamId, p);
	const feed = [
		...new Map(events.map((e) => [`${e.instanceId}:${e.eventId}`, e])).values()
	] as (FeedKill & { eventTime: number; instanceId: string; eventId: string })[];
	const records = feedRecord(feed);
	const multi = new Map<string, number>(),
		maxMulti = new Map<string, number>();
	for (const e of feed)
		if (e.killerSteamId && !e.suicide && !e.teamKill && e.killerSteamId !== e.victimSteamId) {
			const key = `${e.instanceId}:${e.killerSteamId}:${e.eventTime}`;
			const n = (multi.get(key) ?? 0) + 1;
			multi.set(key, n);
			maxMulti.set(e.killerSteamId, Math.max(n, maxMulti.get(e.killerSteamId) ?? 0));
		}
	const last = (progress[0]?.players ?? []) as { steamId: string; cash: number }[];
	const seed = randomUUID();
	const result = [...rows.values()].map((p) => ({
		...p,
		awardSeed: seed,
		multi: maxMulti.get(p.steamId!),
		streak: records.get(p.steamId!)?.killStreak,
		cashHeld: p.cashHeld ?? last.find((x) => x.steamId === p.steamId)?.cash
	}));
	const savedSnapshot = await env.db
		.update(matches)
		.set({ awardSnapshot: result })
		.where(and(eq(matches.id, round.id), isNull(matches.awardSnapshot)))
		.returning({ snapshot: matches.awardSnapshot });
	if (savedSnapshot.length) return result;
	const [concurrent] = await env.db
		.select({ snapshot: matches.awardSnapshot })
		.from(matches)
		.where(eq(matches.id, round.id));
	return (concurrent?.snapshot ?? []) as AwardLine[];
}
