-- Match rows for what was played before match_players existed, from the sessions and the feed.
-- A closed session gives a row to every match it overlapped (the rule the boards used until now:
-- from the match open when the session began, or the first to start after that, to the last to
-- start before it ended). The time on is the overlap. The session's counters go to the last match
-- it overlapped and the earlier ones keep zero: rows written before 2026-09-20 hold only that
-- match's counters anyway, and a later session over several matches cannot be split without the
-- feed. Two sessions of one player in one match keep the larger counters, not the sum, in case the
-- game kept them across the rejoin. The change in cash is unknown (a session holds the balance)
-- and stays zero. Rows the worker has already written are left alone; sessions still open are
-- the worker's. One pass over the sessions with three index probes each, once.
INSERT INTO match_players (match_id, server_id, steam_id, name, faction, seconds, kills, deaths)
WITH s AS (
	SELECT id, server_id, steam_id, name, faction, joined_at, left_at, last_seen, kills, deaths
	  FROM player_sessions WHERE left_at IS NOT NULL
),
bounds AS (
	SELECT s.*, a.id AS last_id,
	       CASE WHEN p.id IS NOT NULL AND COALESCE(p.ended_at, now()) > s.joined_at THEN p.id ELSE n.id END AS first_id
	  FROM s
	  LEFT JOIN LATERAL (SELECT id FROM matches m WHERE m.server_id = s.server_id AND m.started_at < s.left_at
	                      ORDER BY m.started_at DESC LIMIT 1) a ON true
	  LEFT JOIN LATERAL (SELECT id, ended_at FROM matches m WHERE m.server_id = s.server_id AND m.started_at <= s.joined_at
	                      ORDER BY m.started_at DESC LIMIT 1) p ON true
	  LEFT JOIN LATERAL (SELECT id FROM matches m WHERE m.server_id = s.server_id AND m.started_at > s.joined_at
	                      ORDER BY m.started_at LIMIT 1) n ON true
),
pairs AS (
	SELECT m.id AS match_id, m.server_id, b.steam_id, b.name, b.faction, b.last_seen,
	       GREATEST(0, EXTRACT(EPOCH FROM (LEAST(b.left_at, COALESCE(m.ended_at, b.left_at)) - GREATEST(b.joined_at, m.started_at))))::int AS seconds,
	       CASE WHEN m.id = b.last_id THEN b.kills ELSE 0 END AS kills,
	       CASE WHEN m.id = b.last_id THEN b.deaths ELSE 0 END AS deaths
	  FROM bounds b
	  JOIN matches m ON m.server_id = b.server_id AND m.id BETWEEN b.first_id AND b.last_id
	 WHERE b.first_id <= b.last_id
)
SELECT match_id, server_id, steam_id,
       (array_agg(name ORDER BY last_seen DESC))[1],
       (array_agg(faction ORDER BY last_seen DESC))[1],
       SUM(seconds), MAX(kills), MAX(deaths)
  FROM pairs GROUP BY match_id, server_id, steam_id
ON CONFLICT (match_id, steam_id) DO NOTHING;--> statement-breakpoint
-- The feed's columns of every row whose match has kills carrying its row: headshots, team kills,
-- suicides, vehicle kills and the longest shot as sums; the best kill and death streaks as the
-- longest runs of a player's ordered events (feedRecord in match-players.ts, in SQL: a kill is
-- a killer's, not a suicide, not a team kill; every death counts against its victim; a team kill
-- touches no streak). One pass over the kills that carry a match, once.
WITH agg AS (
	SELECT match_row, killer_steam_id AS steam_id,
	       COUNT(*) FILTER (WHERE headshot AND NOT team_kill) AS headshots,
	       COUNT(*) FILTER (WHERE team_kill) AS team_kills,
	       COUNT(*) FILTER (WHERE NOT team_kill AND (cause ~* '^Vehicle\.' OR cause ~* '^Id\.Vehicle\.WeaponExtension\.')) AS vehicle_kills,
	       MAX(distance_m) FILTER (WHERE NOT team_kill) AS longest_m
	  FROM kills WHERE match_row IS NOT NULL AND killer_steam_id IS NOT NULL AND NOT suicide
	 GROUP BY match_row, killer_steam_id
),
sui AS (
	SELECT match_row, victim_steam_id AS steam_id, COUNT(*) AS suicides
	  FROM kills WHERE match_row IS NOT NULL AND suicide
	 GROUP BY match_row, victim_steam_id
),
ev AS (
	SELECT match_row, killer_steam_id AS steam_id, event_time, ts, 1 AS kind
	  FROM kills WHERE match_row IS NOT NULL AND killer_steam_id IS NOT NULL AND NOT suicide AND NOT team_kill
	UNION ALL
	SELECT match_row, victim_steam_id, event_time, ts, 0
	  FROM kills WHERE match_row IS NOT NULL
),
runs AS (
	SELECT match_row, steam_id, kind,
	       ROW_NUMBER() OVER (PARTITION BY match_row, steam_id ORDER BY event_time, ts, kind)
	       - ROW_NUMBER() OVER (PARTITION BY match_row, steam_id, kind ORDER BY event_time, ts, kind) AS grp
	  FROM ev
),
streaks AS (
	SELECT match_row, steam_id,
	       MAX(n) FILTER (WHERE kind = 1) AS kill_streak,
	       MAX(n) FILTER (WHERE kind = 0) AS death_streak
	  FROM (SELECT match_row, steam_id, kind, COUNT(*) AS n FROM runs GROUP BY match_row, steam_id, kind, grp) x
	 GROUP BY match_row, steam_id
),
feed AS (
	SELECT COALESCE(a.match_row, u.match_row, k.match_row) AS match_row,
	       COALESCE(a.steam_id, u.steam_id, k.steam_id) AS steam_id,
	       a.headshots, a.team_kills, a.vehicle_kills, a.longest_m, u.suicides, k.kill_streak, k.death_streak
	  FROM agg a
	  FULL JOIN sui u ON u.match_row = a.match_row AND u.steam_id = a.steam_id
	  FULL JOIN streaks k ON k.match_row = COALESCE(a.match_row, u.match_row) AND k.steam_id = COALESCE(a.steam_id, u.steam_id)
)
UPDATE match_players p
   SET headshots = COALESCE(f.headshots, 0), team_kills = COALESCE(f.team_kills, 0),
       suicides = COALESCE(f.suicides, 0), vehicle_kills = COALESCE(f.vehicle_kills, 0),
       longest_m = f.longest_m,
       kill_streak = COALESCE(f.kill_streak, 0), death_streak = COALESCE(f.death_streak, 0)
  FROM feed f
 WHERE p.match_id = f.match_row AND p.steam_id = f.steam_id;
