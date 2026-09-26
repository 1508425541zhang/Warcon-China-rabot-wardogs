import { and, asc, eq, gte, lte, inArray, or } from 'drizzle-orm';
import type { Env } from '../env';
import {
	kills,
	matches,
	integrityReports,
	playerSessions,
	playerProgressSamples,
	type integrityCases
} from '../db/schema';

/** Lossless column table: dictionary indices apply only to named columns; null stays null. */
export function compactTable(input: object[]) {
	const records = JSON.parse(JSON.stringify(input)) as Record<string, unknown>[];
	const columns = [...new Set(records.flatMap(Object.keys))].sort();
	const dictionaries: Record<string, string[]> = {};
	for (const column of columns) {
		const values = records.map((r) => r[column]);
		if (
			values.some((v) => typeof v === 'string') &&
			values.every((v) => v === null || v === undefined || typeof v === 'string')
		)
			dictionaries[column] = [...new Set(values.filter((v): v is string => typeof v === 'string'))];
	}
	const lookup = Object.fromEntries(
		Object.entries(dictionaries).map(([k, v]) => [k, new Map(v.map((s, i) => [s, i]))])
	);
	return {
		columns,
		dictionaries,
		rows: records.map((r) =>
			columns.map((c) =>
				r[c] === undefined
					? { $missing: true }
					: typeof r[c] === 'string' && lookup[c]
						? lookup[c].get(r[c] as string)
						: r[c]
			)
		),
		count: records.length
	};
}

export async function expandedAiEvidence(
	env: Env,
	c: typeof integrityCases.$inferSelect,
	saved: { instanceId: string; eventId: string; event: unknown }[]
) {
	const snapshot = (c.snapshot ?? {}) as Record<string, unknown>;
	const instance = typeof snapshot.instanceId === 'string' ? snapshot.instanceId : null;
	const clock =
		typeof snapshot.clockTo === 'number' && Number.isFinite(snapshot.clockTo)
			? snapshot.clockTo
			: null;
	const ids = [
		...new Set(saved.filter((e) => !instance || e.instanceId === instance).map((e) => e.eventId))
	];
	// Resolve the actual round from frozen event identities, never from a map name alone.
	const anchors =
		instance && ids.length
			? await env.db
					.select()
					.from(kills)
					.where(
						and(
							eq(kills.serverId, c.serverId),
							eq(kills.instanceId, instance),
							inArray(kills.eventId, ids)
						)
					)
			: [];
	const roundIds = [
		...new Set(anchors.map((r) => r.matchRow).filter((id): id is number => id !== null))
	];
	const roundId = roundIds.length === 1 ? roundIds[0] : null;
	const [round] =
		roundId !== null
			? await env.db
					.select()
					.from(matches)
					.where(and(eq(matches.id, roundId), eq(matches.serverId, c.serverId)))
			: [];
	const fallbackStart = new Date(c.createdAt.getTime() - 86400000);
	const scope =
		round && instance
			? and(eq(kills.matchRow, round.id), eq(kills.instanceId, instance))
			: and(gte(kills.ts, fallbackStart), lte(kills.ts, c.createdAt));
	const rows = await env.db
		.select()
		.from(kills)
		.where(
			and(
				eq(kills.serverId, c.serverId),
				scope,
				or(
					eq(kills.killerSteamId, c.steamId),
					eq(kills.victimSteamId, c.steamId),
					...(round && clock !== null
						? [and(gte(kills.eventTime, Math.max(0, clock - 180)), lte(kills.eventTime, clock))]
						: [])
				)
			)
		)
		.orderBy(asc(kills.ts), asc(kills.eventId));
	const [reports, sessions, progress] = await Promise.all([
		env.db
			.select()
			.from(integrityReports)
			.where(
				and(
					eq(integrityReports.orgId, c.orgId),
					eq(integrityReports.serverId, c.serverId),
					eq(integrityReports.targetSteamId, c.steamId),
					or(
						eq(integrityReports.caseId, c.id),
						and(
							gte(integrityReports.createdAt, round?.startedAt ?? fallbackStart),
							lte(integrityReports.createdAt, round?.endedAt ?? c.createdAt)
						)
					)
				)
			),
		env.db
			.select()
			.from(playerSessions)
			.where(
				and(
					eq(playerSessions.serverId, c.serverId),
					eq(playerSessions.steamId, c.steamId),
					gte(playerSessions.lastSeen, round?.startedAt ?? fallbackStart),
					lte(playerSessions.joinedAt, round?.endedAt ?? c.createdAt)
				)
			),
		round
			? env.db
					.select()
					.from(playerProgressSamples)
					.where(
						and(
							eq(playerProgressSamples.serverId, c.serverId),
							eq(playerProgressSamples.matchId, round.id)
						)
					)
					.orderBy(asc(playerProgressSamples.observedAt))
			: Promise.resolve([])
	]);
	const playerRows = rows.filter(
		(r) => r.killerSteamId === c.steamId || r.victimSteamId === c.steamId
	);
	return {
		encoding:
			'column-table-v1: rows[i][j] uses columns[j]; non-null values in dictionary columns are zero-based dictionary indices; {$missing:true} means absent, not null. Decode before calculating. All stored columns and rows preserved.',
		scope: round
			? {
					kind: 'resolved_round',
					round,
					instanceId: instance,
					caseAt: c.createdAt,
					contextClockFrom: clock === null ? null : Math.max(0, clock - 180),
					contextClockTo: clock,
					includes:
						'all recorded kills AND deaths of the subject in this round, including post-case events already received; plus every server kill in the case 180-second window'
				}
			: {
					kind: 'unresolved_round',
					from: fallbackStart,
					to: c.createdAt,
					includes:
						'all recorded kills AND deaths involving the subject in preceding 24 hours; round cannot be safely resolved, do not combine rounds'
				},
		frozenEvents: {
			identities: compactTable(
				saved.map((r) => ({ instanceId: r.instanceId, eventId: r.eventId }))
			),
			data: compactTable(
				saved.map((r) =>
					r.event && typeof r.event === 'object' && !Array.isArray(r.event)
						? (r.event as object)
						: { value: r.event }
				)
			),
			join: 'identities.rows[i] corresponds to data.rows[i]'
		},
		reports: compactTable(reports),
		sessions: {
			scope: 'session totals may span multiple rounds; not a substitute for round KPM',
			data: compactTable(sessions)
		},
		cashObservations: compactTable(
			progress.flatMap((p) =>
				Array.isArray(p.players)
					? p.players
							.filter(
								(v: unknown) =>
									v && typeof v === 'object' && (v as { steamId?: string }).steamId === c.steamId
							)
							.map((v: object) => ({ observedAt: p.observedAt, matchId: p.matchId, ...v }))
					: []
			)
		),
		combatEvents: compactTable(rows),
		counts: {
			records: rows.length,
			subjectKills: playerRows.filter((r) => r.killerSteamId === c.steamId && !r.suicide).length,
			subjectDeaths: playerRows.filter((r) => r.victimSteamId === c.steamId).length,
			subjectSuicides: playerRows.filter((r) => r.victimSteamId === c.steamId && r.suicide).length,
			postCaseRecords: rows.filter((r) => r.ts > c.createdAt).length
		},
		coverage: {
			databaseRowsComplete: true,
			gameFeedComplete: 'unknown',
			duplicates:
				'records are preserved; deduplicate only on serverId+instanceId+eventId; differing versions must be flagged',
			latestReceivedAt: rows.at(-1)?.ts ?? null,
			nonFatalDamage: {
				available: false,
				events: null,
				reason:
					'Current verified feed stores killed events only; no damage amounts, shots fired, hits or non-fatal injury events are collected. Kill distance is not damage. Do not infer accuracy or total damage.'
			}
		}
	};
}
