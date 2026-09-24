import { and, asc, eq, gt, gte, inArray, lte } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import { integrityCases, kills } from '../db/schema';
import type { InfantryFinding } from './windows';
import type { IntegrityScore } from './score';

export type EvidenceConfidence = 'A' | 'B' | 'C' | 'D';

/** A requires a future continuous-feed proof. Complete stored trigger events qualify as B. */
export function evidenceConfidence(
	expected: readonly string[],
	found: readonly string[]
): EvidenceConfidence {
	if (!expected.length || !found.length) return 'D';
	return expected.length === found.length &&
		expected.length === new Set(found).size &&
		expected.every((id) => found.includes(id))
		? 'B'
		: 'C';
}

export interface FreezeInput {
	orgId: string;
	serverId: string;
	steamId: string;
	finding: InfantryFinding;
	score: IntegrityScore;
	ruleVersion: number;
	rulesSnapshot: unknown;
	createdAt: Date;
}

/** Freeze the accepted source events inside the same transaction as the score and finding. */
export async function freezeFindingEvidence(db: DbOrTx, input: FreezeInput): Promise<string> {
	const eventIds = input.finding.eventIds.slice(0, 200);
	const triggerRows = await db
		.select()
		.from(kills)
		.where(
			and(
				eq(kills.serverId, input.serverId),
				eq(kills.instanceId, input.finding.instanceId),
				inArray(kills.eventId, eventIds)
			)
		);
	const firstReceived = triggerRows.reduce(
		(earliest, row) => Math.min(earliest, row.ts.getTime()),
		input.createdAt.getTime()
	);
	const events = await db
		.select()
		.from(kills)
		.where(
			and(
				eq(kills.serverId, input.serverId),
				eq(kills.instanceId, input.finding.instanceId),
				eq(kills.map, input.finding.map),
				gt(kills.eventTime, input.finding.clockTo - 180),
				lte(kills.eventTime, input.finding.clockTo),
				gte(kills.ts, new Date(firstReceived - 180_000)),
				lte(kills.ts, input.createdAt)
			)
		)
		.orderBy(asc(kills.ts), asc(kills.eventTime))
		.limit(1001);
	const foundIds = events.filter((row) => eventIds.includes(row.eventId)).map((row) => row.eventId);
	const confidence =
		input.finding.eventIds.length > eventIds.length || events.length > 1000
			? 'C'
			: evidenceConfidence(input.finding.eventIds, foundIds);
	const id = `CASE-${crypto.randomUUID().replaceAll('-', '').slice(0, 16).toUpperCase()}`;
	await db.insert(integrityCases).values({
		id,
		orgId: input.orgId,
		serverId: input.serverId,
		steamId: input.steamId,
		createdAt: input.createdAt,
		confidence,
		trigger: 'ABNORMAL_INFANTRY_WINDOW',
		ruleVersion: input.ruleVersion,
		riskScore: input.score.score,
		riskBreakdown: input.score.breakdown,
		snapshot: {
			instanceId: input.finding.instanceId,
			map: input.finding.map,
			clockFrom: input.finding.clockFrom,
			clockTo: input.finding.clockTo,
			infantryKills: input.finding.infantryKills,
			kpm180: input.finding.kpm180,
			uniqueVictims: input.finding.uniqueVictims,
			expectedEventIds: input.finding.eventIds,
			rules: input.rulesSnapshot,
			events: events.slice(0, 1000).map((row) => ({
				eventId: row.eventId,
				ts: row.ts.toISOString(),
				eventTime: row.eventTime,
				map: row.map,
				killerSteamId: row.killerSteamId,
				killerName: row.killerName,
				killerFaction: row.killerFaction,
				victimSteamId: row.victimSteamId,
				victimName: row.victimName,
				victimFaction: row.victimFaction,
				cause: row.cause,
				distanceM: row.distanceM,
				headshot: row.headshot,
				teamKill: row.teamKill,
				suicide: row.suicide,
				tags: row.tags
			}))
		}
	});
	return id;
}
