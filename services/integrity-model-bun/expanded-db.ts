import { SQL } from 'bun';
/** Only a registered queued observation may select a player. DB transactions are read-only. */
export function expandedSourceReader(url: string) {
	if (!url) throw Error('MODEL_DATABASE_URL is required');
	const db = new SQL(url, { max: 2, idleTimeout: 20, connectionTimeout: 10 });
	return async (request: any) => {
		if (!/^[0-9a-f-]{36}$/i.test(request?.requestId ?? ''))
			throw Error('Registered model job required');
		return await db.begin(async (tx: any) => {
			await tx`SET TRANSACTION READ ONLY`;
			await tx`SET LOCAL statement_timeout='10s'`;
			const [job] =
				await tx`SELECT server_id,steam_id,match_id,created_at FROM integrity_model_runs WHERE id=${request.requestId}`;
			if (!job || String(request.sources?.matches?.[0]?.id) !== String(job.match_id))
				throw Error('Observation scope mismatch');
			const [match] =
				await tx`SELECT id,server_id,started_at,map FROM matches WHERE id=${job.match_id} AND server_id=${job.server_id}`;
			if (!match) throw Error('Match missing');
			const end = new Date(Math.floor(new Date(job.created_at).getTime() / 30000) * 30000),
				since = new Date(Math.max(end.getTime() - 2100000, new Date(match.started_at).getTime()));
			const kills =
				await tx`SELECT ts,server_id,event_id,instance_id,match_row,event_time,map,killer_steam_id,victim_steam_id,cause,distance_m,distance_invalid,headshot,suicide,team_kill,tags
    FROM kills WHERE server_id=${job.server_id} AND match_row=${job.match_id} AND ts>=${since} AND ts<=${end} ORDER BY ts,event_id LIMIT 20001`;
			const batches =
				await tx`SELECT received_at,instance_id,payload FROM training_feed_batches WHERE server_id=${job.server_id} AND received_at>=${since} AND received_at<=${end} ORDER BY received_at,id LIMIT 10001`;
			const observations =
				await tx`SELECT received_at,endpoint,CASE WHEN endpoint='/v1/players' THEN jsonb_build_object('roster_size',jsonb_array_length(CASE WHEN jsonb_typeof(payload)='array' THEN payload ELSE COALESCE(payload->'players','[]'::jsonb) END),
    'players',(SELECT COALESCE(jsonb_agg(p-'name'),'[]'::jsonb) FROM jsonb_array_elements(CASE WHEN jsonb_typeof(payload)='array' THEN payload ELSE COALESCE(payload->'players','[]'::jsonb) END) p WHERE p->>'steamId'=${job.steam_id})) ELSE payload END AS payload
    FROM training_observations WHERE server_id=${job.server_id} AND received_at>=${since} AND received_at<=${end} AND endpoint IN ('/v1/players','/v1/status') ORDER BY received_at,id LIMIT 20001`;
			const progress =
				await tx`SELECT observed_at,jsonb_array_length(players) AS roster_size,(SELECT COALESCE(jsonb_agg(p-'name'),'[]'::jsonb) FROM jsonb_array_elements(players) p WHERE p->>'steamId'=${job.steam_id}) AS players
    FROM player_progress_samples WHERE server_id=${job.server_id} AND match_id=${job.match_id} AND observed_at>=${since} AND observed_at<=${end} ORDER BY observed_at LIMIT 10001`;
			if (
				kills.length > 20000 ||
				batches.length > 10000 ||
				observations.length > 20000 ||
				progress.length > 10000
			)
				throw Error('Observation limit exceeded; no silent truncation');
			return {
				match,
				player: job.steam_id,
				end: end.toISOString(),
				kills,
				batches,
				observations,
				progress
			};
		});
	};
}
