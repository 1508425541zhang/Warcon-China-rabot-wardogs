import { and, desc, eq, gte, isNull, lt, lte } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import { kills, playerSessions } from '../db/schema';

export const CHANGE_BUCKET_SECONDS = 15;
export const CHANGE_MINIMUM_BUCKETS = 20;

/** Completed, non-overlapping same-round buckets. No career or previous round is included. */
export function roundKillSeries(clocks: readonly number[], from: number, clock: number): number[] {
	const start = Math.ceil(from / CHANGE_BUCKET_SECONDS) * CHANGE_BUCKET_SECONDS;
	const end = Math.floor(clock / CHANGE_BUCKET_SECONDS) * CHANGE_BUCKET_SECONDS;
	const count = Math.max(0, Math.floor((end - start) / CHANGE_BUCKET_SECONDS));
	if (!Number.isFinite(count) || count < CHANGE_MINIMUM_BUCKETS || count > 5760) return [];
	const series = Array<number>(count).fill(0);
	for (const t of clocks) {
		if (Number.isFinite(t) && t >= start && t < end)
			series[Math.floor((t - start) / CHANGE_BUCKET_SECONDS)] += 60 / CHANGE_BUCKET_SECONDS;
	}
	return series;
}

/** All recorded enemy kills, irrespective of weapon/vehicle classification; duplicates excluded. */
export async function loadRoundChangeSeries(
	db: DbOrTx,
	input: {
		serverId: string;
		steamId: string;
		instanceId: string;
		matchRow: number;
		clock: number;
		at: Date;
	}
): Promise<number[]> {
	const [session] = await db
		.select()
		.from(playerSessions)
		.where(
			and(
				eq(playerSessions.serverId, input.serverId),
				eq(playerSessions.steamId, input.steamId),
				isNull(playerSessions.leftAt),
				lte(playerSessions.joinedAt, input.at),
				gte(playerSessions.lastSeen, new Date(input.at.getTime() - 65000))
			)
		)
		.orderBy(desc(playerSessions.joinedAt))
		.limit(1);
	if (!session) return [];
	const from = Math.max(0, input.clock - (input.at.getTime() - session.joinedAt.getTime()) / 1000);
	const rows = await db
		.select()
		.from(kills)
		.where(
			and(
				eq(kills.serverId, input.serverId),
				eq(kills.instanceId, input.instanceId),
				eq(kills.matchRow, input.matchRow),
				eq(kills.killerSteamId, input.steamId),
				gte(kills.eventTime, from),
				lt(kills.eventTime, input.clock),
				lte(kills.ts, input.at)
			)
		)
		.limit(20001);
	if (rows.length > 20000) return [];
	const seen = new Set<string>();
	const clocks: number[] = [];
	for (const row of rows) {
		if (seen.has(row.eventId)) continue;
		seen.add(row.eventId);
		if (!row.suicide && !row.teamKill && row.victimSteamId !== input.steamId)
			clocks.push(row.eventTime);
	}
	return roundKillSeries(clocks, from, input.clock);
}
