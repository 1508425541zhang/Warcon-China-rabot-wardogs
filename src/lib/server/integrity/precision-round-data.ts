import { sql } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import { compareRoundPrecision, PRECISION_CLASSES, type PrecisionCount } from './precision-round';
import { classifyWeapon, type WeaponCategory } from './weapons';
/** Same-round aggregate, bounded by receipt and game clock to prevent future-event leakage. */
export async function loadRoundPrecision(
	db: DbOrTx,
	input: {
		serverId: string;
		steamId: string;
		instanceId: string;
		matchRow: number;
		clock: number;
		at: Date;
	},
	overrides: ReadonlyMap<string, WeaponCategory>
) {
	const rows = await db.execute(sql`
  WITH events AS (
   SELECT DISTINCT ON (k.event_id) k.event_id, k.killer_steam_id, k.cause, k.headshot, k.tags
   FROM kills k JOIN matches m ON m.id = k.match_row AND m.server_id = k.server_id
   WHERE k.server_id = ${input.serverId} AND k.instance_id = ${input.instanceId}
   AND k.match_row = ${input.matchRow} AND k.ts >= m.started_at AND k.ts <= ${input.at.toISOString()}
   AND k.event_time <= ${input.clock} AND NOT k.suicide AND NOT k.team_kill
   AND k.killer_steam_id IS NOT NULL AND k.killer_steam_id <> k.victim_steam_id
   AND (k.killer_faction IS NULL OR k.victim_faction IS NULL OR k.killer_faction <> k.victim_faction)
   ORDER BY k.event_id, k.ts DESC
  ) SELECT killer_steam_id, cause, tags, count(*)::int AS kills,
   count(*) FILTER (WHERE headshot)::int AS headshots FROM events GROUP BY killer_steam_id, cause, tags`);
	const counts: PrecisionCount[] = [];
	for (const row of rows) {
		const cause = String(row.cause ?? '');
		const category = PRECISION_CLASSES[cause];
		const tags = Array.isArray(row.tags) ? row.tags.map(String) : [];
		if (
			!category ||
			tags.includes('WeaponMelee') ||
			classifyWeapon({ cause, tags, suicide: false }, overrides) !== 'INFANTRY'
		)
			continue;
		counts.push({
			steamId: String(row.killer_steam_id),
			category,
			kills: Number(row.kills),
			headshots: Number(row.headshots)
		});
	}
	return compareRoundPrecision(counts, input.steamId);
}
