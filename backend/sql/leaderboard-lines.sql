lines AS (
 SELECT p.*,m.started_at,m.ended_at,m.map,
 CASE WHEN p.faction IS NULL THEN NULL
 WHEN jsonb_array_length(CASE WHEN jsonb_typeof(m.final_scores)='array' THEN m.final_scores ELSE '[]'::jsonb END)>0
 AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(CASE WHEN jsonb_typeof(m.final_scores)='array' THEN m.final_scores ELSE '[]'::jsonb END) e WHERE e->>'name'=p.faction) THEN NULL
 WHEN m.winner IS NOT NULL THEN CASE WHEN m.winner=p.faction THEN 'win' ELSE 'loss' END
 WHEN (SELECT max((e->>'score')::numeric) FROM jsonb_array_elements(CASE WHEN jsonb_typeof(m.final_scores)='array' THEN m.final_scores ELSE '[]'::jsonb END) e WHERE jsonb_typeof(e)='object' AND e->>'score' ~ '^-?[0-9]+(\.[0-9]+)?$')>0 THEN 'draw'
 ELSE NULL END AS result
 FROM matches m JOIN match_players p ON p.match_id=m.id AND p.server_id=m.server_id
 WHERE m.server_id=ANY($1) AND m.ended_at IS NOT NULL AND m.ended_at>=$2
 AND ($3::text IS NULL OR p.steam_id=$3)
)
