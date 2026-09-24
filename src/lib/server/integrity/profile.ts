import { sql } from 'drizzle-orm';
import type { DbOrTx } from '../db';

const STEAM_ID = /^\d{17}$/;

/** Update one org's identities in one statement, on the observation's fenced transaction. */
export async function recordProfiles(
	db: DbOrTx,
	orgId: string,
	players: { steamId: string; name: string }[],
	seenAt: Date
): Promise<void> {
	const unique = new Map<string, string>();
	for (const player of players) {
		if (STEAM_ID.test(player.steamId) && player.name.trim())
			unique.set(player.steamId, player.name.slice(0, 200));
	}
	if (!unique.size) return;
	const input = JSON.stringify(
		[...unique].map(([steamId, name]) => ({ steam_id: steamId, current_name: name }))
	);
	await db.execute(sql`
		INSERT INTO integrity_profiles (org_id, steam_id, current_name, aliases, first_seen, last_seen)
		SELECT ${orgId}, p.steam_id, p.current_name, jsonb_build_array(p.current_name), ${seenAt}, ${seenAt}
		FROM jsonb_to_recordset((${input}::text)::jsonb) AS p(steam_id text, current_name text)
		ON CONFLICT (org_id, steam_id) DO UPDATE SET
			current_name = CASE WHEN excluded.last_seen >= integrity_profiles.last_seen
				THEN excluded.current_name ELSE integrity_profiles.current_name END,
			aliases = CASE WHEN integrity_profiles.aliases ? excluded.current_name
				THEN integrity_profiles.aliases
				ELSE integrity_profiles.aliases || jsonb_build_array(excluded.current_name) END,
			first_seen = LEAST(integrity_profiles.first_seen, excluded.first_seen),
			last_seen = GREATEST(integrity_profiles.last_seen, excluded.last_seen)
	`);
}
