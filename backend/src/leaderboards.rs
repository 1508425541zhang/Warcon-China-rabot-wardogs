use crate::{config::AppState, error::Result, http::js_number};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
const LINES: &str = include_str!("../sql/leaderboard-lines.sql");
fn base() -> String {
    include_str!("../sql/leaderboard-base.sql").replace("__LINES__", LINES)
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub scope: String,
    pub range: String,
    pub sort: String,
    pub dir: String,
    pub page: i64,
    pub min_minutes: i64,
}
pub fn parse(raw: Option<&str>, max_page: i64) -> Query {
    let mut p = HashMap::new();
    for (k, v) in url::form_urlencoded::parse(raw.unwrap_or("").as_bytes()) {
        p.entry(k.into_owned()).or_insert(v.into_owned());
    }
    let str = |k: &str| p.get(k).map(String::as_str).unwrap_or("");
    let int = |k: &str, default: i64, min: i64, max: i64| {
        let s = str(k);
        if s.is_empty() {
            default
        } else {
            js_number(&json!(s))
                .map(|n| n.trunc().clamp(min as f64, max as f64) as i64)
                .unwrap_or(default)
        }
    };
    Query {
        scope: if str("scope") == "org" {
            "org"
        } else {
            "server"
        }
        .into(),
        range: if ["7d", "30d", "90d", "all"].contains(&str("range")) {
            str("range")
        } else {
            "30d"
        }
        .into(),
        sort: if [
            "kills", "deaths", "kd", "perHour", "playtime", "seeded", "matches", "wins", "winRate",
            "cash",
        ]
        .contains(&str("sort"))
        {
            str("sort")
        } else {
            "kills"
        }
        .into(),
        dir: if str("dir") == "asc" { "asc" } else { "desc" }.into(),
        page: int("page", 1, 1, max_page),
        min_minutes: int("minMinutes", 60, 0, 100000),
    }
}
fn epoch() -> DateTime<Utc> {
    DateTime::from_timestamp(0, 0).unwrap()
}
pub async fn board(state: &AppState, ids: &[String], q: &Query) -> Result<Value> {
    let from = match q.range.as_str() {
        "7d" => Utc::now() - chrono::Duration::days(7),
        "30d" => Utc::now() - chrono::Duration::days(30),
        "90d" => Utc::now() - chrono::Duration::days(90),
        _ => epoch(),
    };
    let order = match q.sort.as_str() {
        "deaths" => "deaths",
        "kd" => {
            "CASE WHEN deaths>0 THEN kills::float8/deaths WHEN kills>0 THEN kills::float8 ELSE NULL END"
        }
        "perHour" => {
            "CASE WHEN minutes-seed_minutes>0 THEN kills::float8/((minutes-seed_minutes)/60) ELSE NULL END"
        }
        "playtime" => "minutes",
        "seeded" => "seed_minutes",
        "matches" => "matches",
        "wins" => "wins",
        "winRate" => {
            "CASE WHEN wins+losses+draws>0 THEN wins::float8/(wins+losses+draws) ELSE NULL END"
        }
        "cash" => "cash",
        _ => "kills",
    };
    let direction = if q.dir == "asc" { "ASC" } else { "DESC" };
    let offset = (q.page - 1) * 50;
    let sql = format!(
        "WITH {},page AS(SELECT *,count(*) OVER() AS total FROM base WHERE minutes>=$4 ORDER BY {order} {direction} NULLS LAST,kills DESC,steam_id LIMIT 50 OFFSET $5) SELECT jsonb_build_object('steamId',r.steam_id,'name',coalesce(nullif((SELECT name FROM player_sessions ps WHERE ps.steam_id=r.steam_id AND ps.server_id=ANY($1) ORDER BY ps.last_seen DESC LIMIT 1),''),r.steam_id),'minutes',round(r.minutes),'seedMinutes',round(r.seed_minutes),'cash',r.cash,'lastSeen',r.last_seen,'kills',r.kills,'deaths',r.deaths,'headshots',r.headshots,'teamKills',r.team_kills,'suicides',r.suicides,'vehicleKills',r.vehicle_kills,'killStreak',r.kill_streak,'deathStreak',r.death_streak,'matches',r.matches,'wins',r.wins,'losses',r.losses,'draws',r.draws,'total',r.total) FROM page r",
        base()
    );
    let mut rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(ids)
        .bind(from)
        .bind(None::<String>)
        .bind(q.min_minutes)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;
    let total = rows.first().map(|r| r["total"].clone()).unwrap_or(json!(0));
    for (i, row) in rows.iter_mut().enumerate() {
        row.as_object_mut().unwrap().remove("total");
        row["rank"] = json!(offset + i as i64 + 1)
    }
    let feed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM servers WHERE id=ANY($1) AND feed_token_hash IS NOT NULL)",
    )
    .bind(ids)
    .fetch_one(&state.db)
    .await?;
    Ok(json!({"query":q,"rows":rows,"total":total,"pageSize":50,"hasFeed":feed}))
}
pub async fn rank(state: &AppState, ids: &[String], steam: &str) -> Result<Option<i64>> {
    let sql = format!(
        "WITH {},me AS(SELECT kills,minutes FROM base WHERE steam_id=$4) SELECT CASE WHEN (SELECT minutes>=60 FROM me) THEN (SELECT count(*)+1 FROM base,me WHERE base.minutes>=60 AND base.kills>me.kills) ELSE NULL END",
        base()
    );
    Ok(sqlx::query_scalar(&sql)
        .bind(ids)
        .bind(epoch())
        .bind(None::<String>)
        .bind(steam)
        .fetch_one(&state.db)
        .await?)
}
pub fn group(rows: &[Value], key: &str) -> Value {
    let mut groups: Vec<Value> = vec![];
    for r in rows {
        let Some(k) = r[key].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        let index = groups
            .iter()
            .position(|g| g["key"] == k)
            .unwrap_or_else(|| {
                groups.push(
                    json!({"key":k,"matches":0,"wins":0,"losses":0,"draws":0,"kills":0,"deaths":0}),
                );
                groups.len() - 1
            });
        let g = &mut groups[index];
        for (field, n) in [
            ("matches", 1.),
            ("kills", r["kills"].as_f64().unwrap_or(0.)),
            ("deaths", r["deaths"].as_f64().unwrap_or(0.)),
        ] {
            g[field] = json!(g[field].as_f64().unwrap() + n)
        }
        if let Some(k) = r["result"].as_str() {
            let field = match k {
                "win" => "wins",
                "loss" => "losses",
                "draw" => "draws",
                _ => continue,
            };
            g[field] = json!(g[field].as_f64().unwrap() + 1.)
        }
    }
    groups.sort_by(|a, b| {
        b["matches"]
            .as_f64()
            .partial_cmp(&a["matches"].as_f64())
            .unwrap()
            .then_with(|| {
                b["kills"]
                    .as_f64()
                    .partial_cmp(&a["kills"].as_f64())
                    .unwrap()
            })
    });
    json!(groups)
}
pub async fn career(
    state: &AppState,
    server: &str,
    ids: &[String],
    names: &HashMap<String, String>,
    steam: &str,
) -> Result<Value> {
    let own = if ids.iter().any(|id| id == server) {
        vec![server.to_owned()]
    } else {
        vec![]
    };
    let sr = rank(state, &own, steam).await?;
    let org = rank(state, ids, steam).await?;
    let sql = format!(
        "WITH {LINES} SELECT jsonb_build_object('matchId',match_id,'serverId',server_id,'startedAt',started_at,'endedAt',ended_at,'map',map,'faction',faction,'result',result,'seconds',seconds,'kills',kills,'deaths',deaths,'cashDelta',cash_delta,'headshots',headshots,'vehicleKills',vehicle_kills,'longestM',longest_m,'killStreak',kill_streak,'deathStreak',death_streak) FROM lines ORDER BY started_at DESC,match_id DESC"
    );
    let mut rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(ids)
        .bind(epoch())
        .bind(steam)
        .fetch_all(&state.db)
        .await?;
    for r in &mut rows {
        let id = r["serverId"].as_str().unwrap();
        r["serverName"] = json!(names.get(id).map(String::as_str).unwrap_or(id));
    }
    let count = |k: &str| rows.iter().filter(|r| r["result"] == k).count();
    let sum = |k: &str| rows.iter().filter_map(|r| r[k].as_f64()).sum::<f64>();
    let max = |k: &str| rows.iter().filter_map(|r| r[k].as_f64()).fold(0., f64::max);
    let longest = rows
        .iter()
        .filter_map(|r| r["longestM"].as_f64())
        .reduce(f64::max);
    let mut streak_kind = None;
    let mut streak_n = 0;
    for r in &rows {
        let Some(result) = r["result"].as_str() else {
            continue;
        };
        if result == "draw" {
            break;
        }
        if streak_kind.is_none() {
            streak_kind = Some(result)
        }
        if streak_kind != Some(result) {
            break;
        }
        streak_n += 1;
    }
    let streak = streak_kind.map(|kind| json!({"kind":kind,"n":streak_n}));
    let last = rows
        .iter()
        .take(10)
        .map(|r| {
            let mut r = r.clone();
            for k in ["vehicleKills", "longestM", "deathStreak"] {
                r.as_object_mut().unwrap().remove(k);
            }
            r
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"rank":{"server":sr,"org":org,"floorMinutes":60},"streak":streak,"matches":rows.len(),"wins":count("win"),"losses":count("loss"),"draws":count("draw"),"kills":sum("kills"),"deaths":sum("deaths"),"minutes":(sum("seconds")/60.+0.5).floor(),"headshots":sum("headshots"),"vehicleKills":sum("vehicleKills"),"longestM":longest,"killStreak":max("killStreak"),"deathStreak":max("deathStreak"),"maps":group(&rows,"map"),"factions":group(&rows,"faction"),"last":last}),
    )
}
