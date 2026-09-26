import { sql } from 'drizzle-orm';
import type { Env } from './env';

export interface MatchRetention {
	fromId: number;
	toId: number;
	fromMap: string | null;
	toMap: string | null;
	startedAt: string;
	provisional: boolean;
	total: number;
	retained: number | null;
	lost: number | null;
	percent: number | null;
}

/** Adjacent observed rounds, including the predecessor outside the selected time range.
 * A missing roster is unknown, never fabricated as 100% churn. */
export async function loadMatchRetention(
	env: Env,
	serverId: string,
	from: Date
): Promise<MatchRetention[]> {
	const rows = await env.db.execute<{
		from_id: number;
		to_id: number;
		from_map: string | null;
		to_map: string | null;
		started_at: Date;
		ended_at: Date | null;
		total: number;
		next_total: number;
		retained: number;
	}>(sql`
		WITH ordered AS (
			SELECT id AS from_id, map AS from_map,
			 LEAD(id) OVER w AS to_id, LEAD(map) OVER w AS to_map,
			 LEAD(started_at) OVER w AS started_at, LEAD(ended_at) OVER w AS ended_at
			FROM matches WHERE server_id = ${serverId}
			WINDOW w AS (ORDER BY started_at, id)
		), pairs AS (
			SELECT * FROM ordered WHERE to_id IS NOT NULL AND started_at >= ${from}
			ORDER BY started_at DESC, to_id DESC LIMIT 100
		)
		SELECT p.*,
		 (SELECT COUNT(DISTINCT steam_id)::int FROM match_players WHERE server_id=${serverId} AND match_id=p.from_id) AS total,
		 (SELECT COUNT(DISTINCT steam_id)::int FROM match_players WHERE server_id=${serverId} AND match_id=p.to_id) AS next_total,
		 (SELECT COUNT(DISTINCT a.steam_id)::int FROM match_players a
		  JOIN match_players b ON a.steam_id=b.steam_id AND b.match_id=p.to_id AND b.server_id=${serverId}
		  WHERE a.match_id=p.from_id AND a.server_id=${serverId}) AS retained
		FROM pairs p ORDER BY started_at, to_id
	`);
	return rows.map((r) => {
		const total = Number(r.total);
		const retained = total > 0 && Number(r.next_total) > 0 ? Number(r.retained) : null;
		return {
			fromId: Number(r.from_id),
			toId: Number(r.to_id),
			fromMap: r.from_map,
			toMap: r.to_map,
			startedAt: new Date(r.started_at).toISOString(),
			provisional: r.ended_at === null,
			total,
			retained,
			lost: retained === null ? null : total - retained,
			percent: retained === null ? null : Math.round((retained / total) * 1000) / 10
		};
	});
}
