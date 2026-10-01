sess AS (
 SELECT steam_id,sum(extract(epoch FROM(coalesce(left_at,now())-greatest(joined_at,$2::timestamptz))))/60 AS minutes,
 sum(seed_seconds)/60.0 AS seed_minutes,max(last_seen) AS last_seen,sum(cash) AS cash
 FROM player_sessions WHERE server_id=ANY($1) AND last_seen>=$2 GROUP BY steam_id
),
__LINES__,
mt AS (
 SELECT steam_id,count(*) AS matches,sum(kills) AS kills,sum(deaths) AS deaths,sum(headshots) AS headshots,
 sum(team_kills) AS team_kills,sum(suicides) AS suicides,sum(vehicle_kills) AS vehicle_kills,
 max(kill_streak) AS kill_streak,max(death_streak) AS death_streak,
 count(*) FILTER(WHERE result='win') AS wins,count(*) FILTER(WHERE result='loss') AS losses,count(*) FILTER(WHERE result='draw') AS draws
 FROM lines GROUP BY steam_id
),
base AS (
 SELECT steam_id,coalesce(sess.minutes,0) AS minutes,coalesce(sess.seed_minutes,0) AS seed_minutes,
 coalesce(sess.cash,0) AS cash,sess.last_seen,
 coalesce(mt.kills,0) AS kills,coalesce(mt.deaths,0) AS deaths,coalesce(mt.headshots,0) AS headshots,
 coalesce(mt.team_kills,0) AS team_kills,coalesce(mt.suicides,0) AS suicides,coalesce(mt.vehicle_kills,0) AS vehicle_kills,
 coalesce(mt.kill_streak,0) AS kill_streak,coalesce(mt.death_streak,0) AS death_streak,
 coalesce(mt.matches,0) AS matches,coalesce(mt.wins,0) AS wins,coalesce(mt.losses,0) AS losses,coalesce(mt.draws,0) AS draws
 FROM sess FULL JOIN mt USING(steam_id)
)
