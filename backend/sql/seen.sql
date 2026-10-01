WITH agg AS (
 SELECT steam_id,(array_agg(name ORDER BY last_seen DESC))[1] AS name,
 array_agg(DISTINCT name) AS names,(array_agg(server_id ORDER BY last_seen DESC))[1] AS last_server_id,
 min(joined_at) AS first_seen,max(last_seen) AS last_seen,count(*)::int AS sessions,
 (sum(extract(epoch FROM(coalesce(left_at,now())-joined_at)))/60)::int AS minutes,
 sum(kills)::int AS kills,sum(deaths)::int AS deaths,count(DISTINCT server_id)::int AS servers,
 bool_or(left_at IS NULL) AS online,bool_or(server_id=$4) AS on_server
 FROM player_sessions WHERE server_id=ANY($1)
 AND ($3 !~ '^[0-9]{17}$' OR steam_id=$3)
 AND ($5::int=0 OR steam_id IN(SELECT steam_id FROM player_sessions WHERE server_id=ANY($1) AND last_seen>=now()-($5*interval '1 day')))
 GROUP BY steam_id
), flagged AS (
 SELECT a.*,s.name AS last_server_name,
 EXISTS(SELECT 1 FROM list_entries e JOIN lists l ON l.id=e.list_id WHERE l.org_id=$2 AND l.server_id IS NULL AND l.kind='ban' AND e.removed_at IS NULL AND(e.expires_at IS NULL OR e.expires_at>now())AND e.steam_id=a.steam_id)AS org_banned,
 (EXISTS(SELECT 1 FROM server_bans b WHERE b.server_id=ANY($1)AND b.steam_id=a.steam_id)
 OR EXISTS(SELECT 1 FROM list_entries e JOIN lists l ON l.id=e.list_id WHERE l.server_id=ANY($1)AND l.kind='ban' AND e.removed_at IS NULL AND(e.expires_at IS NULL OR e.expires_at>now())AND e.steam_id=a.steam_id))AS server_banned,
 EXISTS(SELECT 1 FROM player_marks m WHERE m.org_id=$2 AND m.watched AND m.steam_id=a.steam_id)AS watched
 FROM agg a LEFT JOIN servers s ON s.id=a.last_server_id
)
SELECT to_jsonb(v)FROM(
 SELECT *,count(*)OVER()::int AS total FROM flagged
 WHERE($3='' OR($3~'^[0-9]+$' AND steam_id LIKE $3||'%')OR EXISTS(SELECT 1 FROM unnest(names)n WHERE n ILIKE $6))
 AND($5::int=0 OR last_seen>=now()-($5*interval '1 day'))
 AND(NOT $7 OR on_server)
 AND(CASE $8 WHEN 'banned' THEN(org_banned OR server_banned)WHEN 'watched' THEN watched WHEN 'online' THEN online ELSE true END)
 ORDER BY __ORDER__,steam_id LIMIT $9 OFFSET $10
)v
