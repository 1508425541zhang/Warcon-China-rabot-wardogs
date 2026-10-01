use crate::http::ApiQuery;
use crate::{
    auth::{authenticate, server_scope},
    config::AppState,
    error::Result,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
#[derive(Default, Deserialize)]
pub struct MatchQuery {
    page: Option<String>,
}
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiQuery(query): ApiQuery<MatchQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    server_scope(&state, &actor, &id, "server.view").await?;
    Ok(Json(
        list_view(&state, &id, query.page.as_deref(), 50).await?,
    ))
}
pub async fn list_view(
    state: &AppState,
    id: &str,
    raw_page: Option<&str>,
    page_size: i64,
) -> Result<Value> {
    let page = raw_page
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite() && n.fract() == 0. && *n >= 1.)
        .map(|n| n.min(100_000.) as i64)
        .unwrap_or(1);
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM matches WHERE server_id=$1")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    let rows=sqlx::query("SELECT jsonb_build_object('id',m.id,'startedAt',to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'endedAt',to_char(m.ended_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'map',m.map,'experiences',m.experiences,'lighting',m.lighting,'peakPlayers',m.peak_players,'players',(SELECT count(*) FROM match_players p WHERE p.match_id=m.id),'finalScores',m.final_scores,'winner',m.winner) AS data FROM matches m WHERE m.server_id=$1 ORDER BY m.id DESC LIMIT $2 OFFSET $3")
        .bind(&id).bind(page_size).bind((page as i64-1)*page_size).fetch_all(&state.db).await?;
    let mut matches: Vec<Value> = rows
        .iter()
        .map(|r| r.try_get("data"))
        .collect::<std::result::Result<_, _>>()?;
    for row in &mut matches {
        row["finalScores"] = normalize_scores(&row["finalScores"]);
    }
    let status: Option<Value> =
        sqlx::query_scalar("SELECT status FROM server_live WHERE server_id=$1")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?
            .flatten();
    let live:Vec<Value>=status.as_ref().and_then(|s|s.get("scores")).and_then(Value::as_array).map(|scores|scores.iter().map(|f|json!({"name":f.get("name"),"colorHex":f.get("colorHex").filter(|v|v.as_str()!=Some("")),"score":f.get("score").and_then(Value::as_f64).unwrap_or(0.0)})).collect()).unwrap_or_default();
    Ok(
        json!({"ok":true,"matches":matches,"live":live,"page":page,"pageSize":page_size,"total":total,"pages":((total+page_size-1)/page_size).max(1)}),
    )
}

pub fn normalize_scores(value: &Value) -> Value {
    let Some(rows) = value.as_array() else {
        return Value::Null;
    };
    let result: Vec<Value> = rows
        .iter()
        .filter_map(|r| {
            let name = r["name"].as_str().filter(|s| !s.is_empty())?;
            let score = r["score"]
                .as_f64()
                .or_else(|| {
                    r["score"]
                        .as_str()
                        .filter(|s| {
                            let s = s.strip_prefix('-').unwrap_or(s);
                            let mut parts = s.split('.');
                            let first = parts.next().unwrap_or("");
                            let second = parts.next();
                            !first.is_empty()
                                && first.bytes().all(|b| b.is_ascii_digit())
                                && second.is_none_or(|p| {
                                    !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())
                                })
                                && parts.next().is_none()
                        })
                        .and_then(|s| s.parse::<f64>().ok())
                })
                .filter(|n| n.is_finite())?;
            Some(json!({"name":name,"score":score}))
        })
        .collect();
    if result.is_empty() {
        Value::Null
    } else {
        json!(result)
    }
}

pub async fn detail(
    State(state): State<AppState>,
    Path((server, raw)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    server_scope(&state, &actor, &server, "server.view").await?;
    if raw.is_empty() || raw.len() > 18 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(crate::error::ApiError::missing());
    }
    let id = raw
        .parse::<i64>()
        .map_err(|_| crate::error::ApiError::missing())?;
    let view = load_match(&state.db, &server, id)
        .await?
        .ok_or_else(crate::error::ApiError::missing)?;
    Ok(Json(view))
}
pub async fn load_match(db: &sqlx::PgPool, server: &str, id: i64) -> Result<Option<Value>> {
    let row=sqlx::query("SELECT started_at,ended_at,winner,final_scores,jsonb_build_object('id',id,'startedAt',to_char(started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'endedAt',to_char(ended_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'map',map,'experiences',experiences,'lighting',lighting,'peakPlayers',peak_players,'finalScores',final_scores,'winner',winner) AS data FROM matches WHERE server_id=$1 AND id=$2 AND ended_at IS NOT NULL").bind(server).bind(id).fetch_optional(db).await?;
    let Some(row) = row else { return Ok(None) };
    let start: chrono::DateTime<chrono::Utc> = row.try_get("started_at")?;
    let end: chrono::DateTime<chrono::Utc> = row.try_get("ended_at")?;
    let winner: Option<String> = row.try_get("winner")?;
    let raw_scores: Value = row
        .try_get::<Option<Value>, _>("final_scores")?
        .unwrap_or(Value::Null);
    let final_scores = normalize_scores(&raw_scores);
    let mut summary: Value = row.try_get("data")?;
    summary["finalScores"] = final_scores.clone();
    let mut lines:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',steam_id,'name',name,'faction',faction,'seconds',seconds,'kills',kills,'deaths',deaths,'cashDelta',cash_delta,'headshots',headshots,'teamKills',team_kills,'suicides',suicides,'vehicleKills',vehicle_kills,'longestM',longest_m,'killStreak',kill_streak,'deathStreak',death_streak) FROM match_players WHERE match_id=$1 ORDER BY kills DESC,seconds DESC,steam_id").bind(id).fetch_all(db).await?;
    for line in &mut lines {
        let faction = line["faction"].as_str().filter(|s| !s.is_empty());
        let scoreboard = raw_scores.as_array();
        let on = faction.is_some_and(|f| {
            scoreboard.is_none_or(|s| s.is_empty() || s.iter().any(|s| s["name"] == f))
        });
        let result = if !on {
            None
        } else if let Some(w) = winner.as_deref().filter(|s| !s.is_empty()) {
            Some(if Some(w) == faction { "win" } else { "loss" })
        } else if scoreboard.is_some_and(|s| {
            s.iter()
                .any(|f| crate::http::js_number(&f["score"]).is_some_and(|n| n > 0.))
        }) {
            Some("draw")
        } else {
            None
        };
        line["result"] = json!(result);
    }
    let points=sqlx::query("SELECT ts,scores FROM samples WHERE server_id=$1 AND ok=true AND ts>=$2 AND ts<$3 ORDER BY ts").bind(server).bind(start).bind(end).fetch_all(db).await?;
    let live: Option<Value> =
        sqlx::query_scalar("SELECT status FROM server_live WHERE server_id=$1")
            .bind(server)
            .fetch_optional(db)
            .await?
            .flatten();
    let mut names: Vec<String> = final_scores
        .as_array()
        .map(|s| {
            s.iter()
                .filter_map(|f| f["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut normalized = Vec::new();
    for point in points {
        let ts: chrono::DateTime<chrono::Utc> = point.try_get("ts")?;
        let scores = normalize_scores(
            &point
                .try_get::<Option<Value>, _>("scores")?
                .unwrap_or(Value::Null),
        );
        if let Some(s) = scores.as_array() {
            for f in s {
                let name = f["name"].as_str().unwrap().to_owned();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        normalized.push((ts, scores));
    }
    for line in &lines {
        if let Some(f) = line["faction"].as_str().filter(|s| !s.is_empty()) {
            if !names.iter().any(|n| n == f) {
                names.push(f.to_owned());
            }
        }
    }
    let factions: Vec<_> = names
        .iter()
        .map(|name| {
            let color = live
                .as_ref()
                .and_then(|v| v["scores"].as_array())
                .and_then(|s| s.iter().find(|s| s["name"] == *name))
                .and_then(|s| s["colorHex"].as_str())
                .filter(|s| !s.is_empty());
            json!({"name":name,"colorHex":color})
        })
        .collect();
    let timeline: Vec<Value> = normalized
        .iter()
        .filter_map(|(ts, scores)| {
            let s = scores.as_array()?;
            let mut point = vec![json!(
                ((ts.timestamp_millis() - start.timestamp_millis()) as f64 / 1000.).round()
            )];
            for n in &names {
                point.push(
                    s.iter()
                        .find(|f| f["name"] == *n)
                        .map(|f| f["score"].clone())
                        .unwrap_or(json!(0)),
                );
            }
            Some(json!(point))
        })
        .collect();
    let kills: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM kills WHERE server_id=$1 AND ts>=$2 AND match_row=$3",
    )
    .bind(server)
    .bind(start - chrono::Duration::seconds(120))
    .bind(id)
    .fetch_one(db)
    .await?;
    let has_feed: bool =
        sqlx::query_scalar("SELECT feed_token_hash IS NOT NULL FROM servers WHERE id=$1")
            .bind(server)
            .fetch_one(db)
            .await?;
    summary["players"] = json!(lines.len());
    let duration = ((end.timestamp_millis() - start.timestamp_millis()) as f64 / 1000.)
        .round()
        .max(0.);
    let awards = awards(&lines, duration);
    Ok(Some(
        json!({"ok":true,"match":summary,"factions":factions,"lines":lines,"timeline":timeline,"awards":awards,"kills":kills,"hasFeed":has_feed}),
    ))
}
fn comma(n: f64) -> String {
    let s = format!("{:.0}", n.round());
    let digits = s.strip_prefix('-').unwrap_or(&s);
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',')
        }
        out.push(c)
    }
    if s.starts_with('-') {
        out.insert(0, '-')
    }
    out
}
fn fixed2(value: f64) -> String {
    if value == 0. {
        return "0.00".into();
    }
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023 - 52;
    let numerator = (((bits & ((1u64 << 52) - 1)) | (1u64 << 52)) as u128) * 100;
    let cents = if exponent >= 0 {
        numerator << exponent
    } else {
        let shift = (-exponent) as u32;
        if shift >= 128 {
            0
        } else {
            let divisor = 1u128 << shift;
            let q = numerator / divisor;
            let rem = numerator % divisor;
            q + u128::from(rem >= divisor / 2)
        }
    };
    format!("{}.{:02}", cents / 100, cents % 100)
}
pub fn awards(lines: &[Value], duration: f64) -> Vec<Value> {
    if duration < 1200. || lines.is_empty() {
        return vec![];
    }
    let mut result = Vec::new();
    for (key, label, field) in [
        ("kills", "Most kills", "kills"),
        ("kd", "Best K/D", ""),
        ("longest", "Longest shot", "longestM"),
        ("streak", "Best streak", "killStreak"),
        ("cash", "Richest match", "cashDelta"),
    ] {
        let mut top: Option<(&Value, f64)> = None;
        for l in lines {
            let value = if key == "kd" {
                let kills = l["kills"].as_f64().unwrap_or(0.);
                if kills < 10. {
                    None
                } else {
                    let deaths = l["deaths"].as_f64().unwrap_or(0.);
                    Some(if deaths > 0. { kills / deaths } else { kills })
                }
            } else {
                l[field].as_f64()
            };
            if let Some(v) = value {
                if top.is_none_or(|(_, n)| v > n) {
                    top = Some((l, v));
                }
            }
        }
        if let Some((line, value)) = top {
            let valid = match key {
                "kd" => true,
                "streak" => value >= 3.,
                _ => value > 0.,
            };
            if valid {
                let formatted = match key {
                    "kd" => fixed2(value),
                    "longest" => format!("{} m", value.round()),
                    "cash" => format!("+${}", comma(value)),
                    _ => value.to_string(),
                };
                result.push(json!({"key":key,"label":label,"steamId":line["steamId"],"name":line["name"],"value":formatted}));
            }
        }
    }
    result
}
