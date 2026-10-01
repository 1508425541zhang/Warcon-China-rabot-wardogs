use crate::{
    auth::{authenticate, server_scope},
    config::AppState,
    error::{ApiError, Result},
    http::ApiQuery,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn number(v: &Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|v| v.is_finite())
        .unwrap_or(0.)
}
#[derive(Deserialize, Default)]
pub struct Query {
    range: Option<String>,
    since: Option<String>,
}
pub async fn cash(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<Query>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    server_scope(&state, &actor, &id, "server.view").await?;
    let now = Utc::now();
    let since = match query.since.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s
            .parse::<DateTime<Utc>>()
            .map_err(|_| ApiError::bad("since must be an ISO timestamp."))?,
        None => now - chrono::Duration::hours(1),
    }
    .max(now - chrono::Duration::hours(24));
    let rows=sqlx::query("SELECT ts,cash FROM samples WHERE server_id=$1 AND ts>=$2 AND ok AND cash IS NOT NULL ORDER BY ts DESC LIMIT 3000").bind(&id).bind(since).fetch_all(&state.db).await?;
    let mut points = vec![];
    for r in rows.iter().rev() {
        let values: Value = r.try_get("cash")?;
        let mut factions = BTreeMap::new();
        let mut total = 0.;
        if let Some(values) = values.as_array() {
            for v in values {
                let n = (number(&v["cash"]) + 0.5).floor();
                *factions
                    .entry(v["name"].as_str().unwrap_or("").to_owned())
                    .or_insert(0.) += n;
                total += n;
            }
        }
        points.push(json!({"ts":iso(r.try_get("ts")?),"factions":factions,"total":total}));
    }
    Ok(Json(json!({"ok":true,"since":iso(since),"points":points})))
}
const RAW: &str = "SELECT ts,ok,player_count::float8 player_count,player_count peak,max_players,map,least(600,extract(epoch FROM(coalesce(lead(ts) OVER(ORDER BY ts),now())-ts))) dur,1 n FROM samples WHERE server_id=$1 AND ts>=$2";
fn covered(rolled: bool) -> String {
    if !rolled {
        return RAW.into();
    };
    format!(
        "WITH cut AS (SELECT coalesce(max(bucket)+interval '1 hour',$2::timestamptz) t FROM sample_rollups WHERE server_id=$1 AND bucket>=$2) SELECT r.bucket ts,true ok,r.player_s/nullif(r.up_s,0) player_count,r.max_players peak,r.max_cap max_players,NULL::text map,r.up_s dur,r.ok_samples n FROM sample_rollups r,cut WHERE r.server_id=$1 AND r.bucket>=$2 AND r.bucket<cut.t AND r.up_s>0 UNION ALL SELECT r.bucket,false,NULL,NULL,NULL,NULL,r.down_s,r.samples-r.ok_samples FROM sample_rollups r,cut WHERE r.server_id=$1 AND r.bucket>=$2 AND r.bucket<cut.t AND r.down_s>0 UNION ALL SELECT x.ts,x.ok,x.player_count,x.peak,x.max_players,x.map,x.dur,x.n FROM ({RAW}) x,cut WHERE x.ts>=cut.t"
    )
}
async fn rows(
    db: &PgPool,
    sql: &str,
    id: &str,
    from: DateTime<Utc>,
    bucket: i32,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(sql)
        .bind(id)
        .bind(from)
        .bind(bucket)
        .fetch_all(db)
        .await?)
}
pub async fn retention(db: &PgPool, id: &str, from: DateTime<Utc>) -> Result<Vec<Value>> {
    let sql = "WITH ordered AS (SELECT id from_id,map from_map,lead(id) OVER w to_id,lead(map) OVER w to_map,lead(started_at) OVER w started_at,lead(ended_at) OVER w ended_at FROM matches WHERE server_id=$1 WINDOW w AS(ORDER BY started_at,id)),pairs AS(SELECT * FROM ordered WHERE to_id IS NOT NULL AND started_at>=$2 ORDER BY started_at DESC,to_id DESC LIMIT 100),counted AS(SELECT p.*,(SELECT count(DISTINCT steam_id) FROM match_players WHERE server_id=$1 AND match_id=p.from_id) total,(SELECT count(DISTINCT steam_id) FROM match_players WHERE server_id=$1 AND match_id=p.to_id) next_total,(SELECT count(DISTINCT a.steam_id) FROM match_players a JOIN match_players b ON a.steam_id=b.steam_id AND b.match_id=p.to_id AND b.server_id=$1 WHERE a.match_id=p.from_id AND a.server_id=$1) kept FROM pairs p),r AS(SELECT *,CASE WHEN total>0 AND next_total>0 THEN kept ELSE NULL END retained FROM counted) SELECT jsonb_build_object('fromId',from_id,'toId',to_id,'fromMap',from_map,'toMap',to_map,'startedAt',to_char(started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'provisional',ended_at IS NULL,'total',total,'retained',retained,'lost',total-retained,'percent',round(retained::numeric/nullif(total,0)*1000)/10) FROM r ORDER BY started_at,to_id";
    Ok(sqlx::query_scalar(sql)
        .bind(id)
        .bind(from)
        .fetch_all(db)
        .await?)
}
pub async fn analytics(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<Query>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    server_scope(&state, &actor, &id, "server.view").await?;
    let (range, days, bucket) = match query.range.as_deref() {
        Some("7d") => ("7d", 7, 1800),
        Some("30d") => ("30d", 30, 7200),
        _ => ("24h", 1, 300),
    };
    let to = Utc::now();
    let from = to - chrono::Duration::days(days);
    let cover = covered(days > 14);
    let db = &state.db;
    let population=rows(db,&format!("WITH s AS({cover}),r AS(SELECT to_timestamp(floor(extract(epoch FROM ts)/$3)*$3) b,sum(player_count*dur) FILTER(WHERE ok)/nullif(sum(dur) FILTER(WHERE ok),0) avg,max(peak) max,max(max_players) cap,coalesce(sum(n) FILTER(WHERE ok),0) ok,sum(n) total,coalesce(sum(dur) FILTER(WHERE ok),0) up,coalesce(sum(dur) FILTER(WHERE NOT ok),0) down FROM s GROUP BY b) SELECT jsonb_build_object('ts',to_char(b AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'avg',avg,'max',max,'cap',cap,'ok',ok,'total',total,'up',up,'down',down) FROM r ORDER BY b"),&id,from,bucket).await?;
    let totals:Value=sqlx::query_scalar(&format!("WITH s AS({cover}) SELECT jsonb_build_object('samples',coalesce(sum(n),0),'peak',coalesce(max(peak),0),'avg',coalesce(sum(player_count*dur) FILTER(WHERE ok)/nullif(sum(dur) FILTER(WHERE ok),0),0),'up',coalesce(sum(dur) FILTER(WHERE ok),0),'down',coalesce(sum(dur) FILTER(WHERE NOT ok),0)) FROM s")).bind(&id).bind(from).fetch_one(db).await?;
    let hourly:Vec<Value>=sqlx::query_scalar(&format!("WITH s AS({cover}),r AS(SELECT extract(hour FROM ts AT TIME ZONE 'UTC')::int h,sum(player_count*dur)/nullif(sum(dur),0) avg FROM s WHERE ok GROUP BY h) SELECT jsonb_build_object('hour',h,'avg',coalesce(avg,0)) FROM r ORDER BY h")).bind(&id).bind(from).fetch_all(db).await?;
    let map_source = if days > 14 {
        format!(
            "WITH cut AS(SELECT coalesce(max(bucket)+interval '1 hour',$2::timestamptz) t FROM sample_rollups WHERE server_id=$1 AND bucket>=$2) SELECT map,secs FROM sample_map_rollups,cut WHERE server_id=$1 AND bucket>=$2 AND bucket<cut.t UNION ALL SELECT x.map,x.dur FROM({RAW}) x,cut WHERE x.ts>=cut.t AND x.ok AND x.map IS NOT NULL AND x.map<>''"
        )
    } else {
        format!("SELECT map,dur secs FROM({RAW}) x WHERE ok AND map IS NOT NULL AND map<>''")
    };
    let maps:Vec<Value>=sqlx::query_scalar(&format!("WITH s AS({map_source}),r AS(SELECT map,sum(secs) secs FROM s GROUP BY map) SELECT jsonb_build_object('map',r.map,'minutes',round(secs/60),'matches',(SELECT count(*) FROM matches m WHERE m.server_id=$1 AND m.map=r.map AND m.started_at>=$2)) FROM r ORDER BY secs DESC")).bind(&id).bind(from).fetch_all(db).await?;
    let players:Vec<Value>=sqlx::query_scalar("WITH seen AS(SELECT steam_id,(SELECT name FROM player_sessions p2 WHERE p2.steam_id=p.steam_id AND p2.server_id=p.server_id ORDER BY last_seen DESC LIMIT 1) name,sum(extract(epoch FROM(coalesce(left_at,now())-greatest(joined_at,$2::timestamptz))))/60 minutes,count(*) sessions,max(last_seen) last_seen,count(*) FILTER(WHERE left_at IS NULL)>0 online FROM player_sessions p WHERE server_id=$1 AND last_seen>=$2 GROUP BY server_id,steam_id ORDER BY minutes DESC LIMIT 50),recorded AS(SELECT p.steam_id,sum(kills) kills,sum(deaths) deaths FROM match_players p JOIN matches m ON m.id=p.match_id WHERE p.server_id=$1 AND p.steam_id IN(SELECT steam_id FROM seen) AND m.ended_at IS NOT NULL AND m.ended_at>=$2 GROUP BY p.steam_id) SELECT jsonb_build_object('steamId',s.steam_id,'name',s.name,'minutes',round(s.minutes),'sessions',s.sessions,'kills',coalesce(r.kills,0),'deaths',coalesce(r.deaths,0),'lastSeen',to_char(s.last_seen AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'online',s.online) FROM seen s LEFT JOIN recorded r USING(steam_id) ORDER BY s.minutes DESC").bind(&id).bind(from).fetch_all(db).await?;
    let counts:Value=sqlx::query_scalar("SELECT jsonb_build_object('uniquePlayers',(SELECT count(DISTINCT steam_id) FROM player_sessions WHERE server_id=$1 AND last_seen>=$2),'onlineNow',(SELECT count(*) FROM player_sessions WHERE server_id=$1 AND left_at IS NULL),'matches',(SELECT count(*) FROM matches WHERE server_id=$1 AND started_at>=$2))").bind(&id).bind(from).fetch_one(db).await?;
    let matches:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'startedAt',to_char(started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'endedAt',to_char(ended_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'map',map,'experiences',experiences,'lighting',lighting,'peakPlayers',peak_players,'finalScores',final_scores,'winner',winner) FROM matches WHERE server_id=$1 AND started_at>=$2 ORDER BY ended_at IS NULL DESC,started_at DESC LIMIT 30").bind(&id).bind(from).fetch_all(db).await?;
    let cash_rows=sqlx::query("SELECT to_timestamp(floor(extract(epoch FROM s.ts)/$3)*$3) b,e->>'name' name,avg((e->>'cash')::numeric)::float8 cash FROM samples s CROSS JOIN LATERAL jsonb_array_elements(CASE WHEN jsonb_typeof(s.cash)='array' THEN s.cash ELSE '[]'::jsonb END) e WHERE s.server_id=$1 AND s.ts>=$2 AND s.ok AND jsonb_typeof(e)='object' AND e->>'cash' ~ '^-?[0-9]+(\\.[0-9]+)?$' GROUP BY b,name ORDER BY b").bind(&id).bind(from).bind(bucket).fetch_all(db).await?;
    let mut cash: BTreeMap<DateTime<Utc>, BTreeMap<String, f64>> = BTreeMap::new();
    for r in cash_rows {
        let n = (r.try_get::<f64, _>("cash")? + 0.5).floor();
        *cash
            .entry(r.try_get("b")?)
            .or_default()
            .entry(r.try_get::<Option<String>, _>("name")?.unwrap_or_default())
            .or_insert(0.) += n;
    }
    let cash=cash.into_iter().map(|(t,factions)|json!({"ts":iso(t),"total":factions.values().sum::<f64>(),"factions":factions})).collect::<Vec<_>>();
    let up = number(&totals["up"]);
    let down = number(&totals["down"]);
    let settings = crate::settings::load(db).await?;
    let sample_seconds = number(&settings["sampleMs"]) / 1000.;
    let playtime = crate::playtime::distribution(
        db,
        &id,
        from,
        std::env::var("STEAM_API_KEY").is_ok_and(|s| !s.is_empty()),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "database",
            "Playtime query failed.",
        )
    })?;
    let retention = retention(db, &id, from).await?;
    let combat = combat(db, &id, from, bucket).await?;
    Ok(Json(
        json!({"ok":true,"range":range,"from":iso(from),"to":iso(to),"sampleSeconds":sample_seconds.round(),"bucketSeconds":bucket,
        "summary":{"uniquePlayers":counts["uniquePlayers"],"onlineNow":counts["onlineNow"],"matches":counts["matches"],"samples":totals["samples"],"peakPlayers":totals["peak"],"avgPlayers":(number(&totals["avg"])*10.).round()/10.,"uptimePct":if up+down>0. {Some((up/(up+down)*1000.).round()/10.)}else{None},"coveredHours":((up+down)/3600.*10.).round()/10.},"population":population,"maps":maps,"players":players,"matches":matches,"cash":cash,"hourly":hourly,"retention":retention,"playtime":playtime,"combat":combat}),
    ))
}
async fn combat(db: &PgPool, id: &str, from: DateTime<Utc>, bucket: i32) -> Result<Value> {
    let mut totals:Value=sqlx::query_scalar("SELECT jsonb_build_object('kills',count(*),'headshots',count(*) FILTER(WHERE headshot),'teamKills',count(*) FILTER(WHERE team_kill),'suicides',count(*) FILTER(WHERE suicide),'vehicleKills',count(*) FILTER(WHERE cause LIKE 'Vehicle.%' OR cause LIKE 'Id.Vehicle.%')) FROM kills WHERE server_id=$1 AND ts>=$2").bind(id).bind(from).fetch_one(db).await?;
    let configured: bool =
        sqlx::query_scalar("SELECT feed_token_hash IS NOT NULL FROM servers WHERE id=$1")
            .bind(id)
            .fetch_one(db)
            .await?;
    if !configured && totals["kills"].as_i64() == Some(0) {
        return Ok(Value::Null);
    }
    totals["perBucket"]=json!(rows(db,"WITH r AS(SELECT to_timestamp(floor(extract(epoch FROM ts)/$3)*$3) b,count(*) kills FROM kills WHERE server_id=$1 AND ts>=$2 GROUP BY b) SELECT jsonb_build_object('ts',to_char(b AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'kills',kills) FROM r ORDER BY b",id,from,bucket).await?);
    totals["causes"]=json!(sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('cause',cause,'kills',count(*),'headshots',count(*) FILTER(WHERE headshot)) FROM kills WHERE server_id=$1 AND ts>=$2 AND cause IS NOT NULL AND NOT suicide GROUP BY cause ORDER BY count(*) DESC LIMIT 12").bind(id).bind(from).fetch_all(db).await?);
    totals["players"]=json!(sqlx::query_scalar::<_,Value>("WITH k AS(SELECT killer_steam_id steam_id,max(killer_name) name,count(*) kills,count(*) FILTER(WHERE headshot) headshots,count(*) FILTER(WHERE team_kill) team_kills,avg(distance_m) avg FROM kills WHERE server_id=$1 AND ts>=$2 AND killer_steam_id IS NOT NULL AND NOT suicide GROUP BY killer_steam_id),d AS(SELECT victim_steam_id steam_id,count(*) deaths FROM kills WHERE server_id=$1 AND ts>=$2 GROUP BY victim_steam_id) SELECT jsonb_build_object('steamId',k.steam_id,'name',k.name,'kills',k.kills,'deaths',coalesce(d.deaths,0),'headshots',k.headshots,'teamKills',k.team_kills,'avgDistanceM',floor(k.avg+0.5)) FROM k LEFT JOIN d USING(steam_id) ORDER BY k.kills DESC LIMIT 25").bind(id).bind(from).fetch_all(db).await?);
    totals["longest"]=json!(sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('ts',to_char(ts AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'killer',killer_name,'victim',victim_name,'cause',cause,'distanceM',floor(distance_m+0.5)) FROM kills WHERE server_id=$1 AND ts>=$2 AND distance_m IS NOT NULL AND NOT suicide AND NOT team_kill ORDER BY distance_m DESC LIMIT 5").bind(id).bind(from).fetch_all(db).await?);
    Ok(totals)
}
