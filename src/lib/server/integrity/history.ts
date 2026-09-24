import { and, eq, gte, inArray, lt, ne } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import { integrityScores } from '../db/schema';

/** Previous execution-level *recorded* result, never reinterpreted with today's thresholds. */
export async function hadRecentAutoKo(
	db: DbOrTx,
	orgId: string,
	steamId: string,
	before: Date,
	windowHours: number,
	currentWindowId: number | null
): Promise<boolean> {
	const conditions = [
		eq(integrityScores.orgId, orgId),
		eq(integrityScores.steamId, steamId),
		eq(integrityScores.source, 'window'),
		inArray(integrityScores.level, [
			'AUTO_KO',
			'AUTO_QUARANTINE_ELIGIBLE',
			'AUTO_QUARANTINE_24H',
			'AUTO_QUARANTINE_7D'
		]),
		gte(integrityScores.scoredAt, new Date(before.getTime() - windowHours * 3600_000)),
		lt(integrityScores.scoredAt, before)
	];
	if (currentWindowId !== null) conditions.push(ne(integrityScores.windowId, currentWindowId));
	const [row] = await db
		.select({ id: integrityScores.id })
		.from(integrityScores)
		.where(and(...conditions))
		.limit(1);
	return !!row;
}
