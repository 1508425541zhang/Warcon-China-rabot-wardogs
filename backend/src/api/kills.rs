use crate::{
    auth::{authenticate, server_scope},
    config::AppState,
    error::{ApiError, Result},
};
use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, Method},
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Postgres, QueryBuilder, Row};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillQuery {
    pub before: Option<DateTime<Utc>>,
    pub before_time: Option<f64>,
    pub limit: Option<i64>,
    pub count: Option<String>,
    pub player: Option<String>,
    pub killer: Option<String>,
    pub victim: Option<String>,
    pub cause: Option<String>,
    pub kind: Option<String>,
    pub min_m: Option<f64>,
    #[serde(rename = "match")]
    pub match_id: Option<i64>,
}
fn parse_query(raw: Option<String>) -> Result<KillQuery> {
    let mut fields = std::collections::HashMap::new();
    for (key, value) in url::form_urlencoded::parse(raw.as_deref().unwrap_or("").as_bytes()) {
        fields.entry(key.into_owned()).or_insert(value.into_owned());
    }
    let number = |key: &str| {
        fields
            .get(key)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|n| n.is_finite())
    };
    let before = if let Some(value) = fields.get("before").filter(|v| !v.is_empty()) {
        Some(
            DateTime::parse_from_rfc3339(value)
                .map_err(|_| ApiError::bad("before must be an ISO timestamp."))?
                .to_utc(),
        )
    } else {
        None
    };
    let before_time = if fields.get("beforeTime").is_some_and(|v| !v.is_empty()) {
        Some(number("beforeTime").ok_or_else(|| ApiError::bad("beforeTime must be a number."))?)
    } else {
        None
    };
    let match_id = if fields.get("match").is_some_and(|v| !v.is_empty()) {
        let n = number("match")
            .filter(|n| *n > 0. && *n <= 9_007_199_254_740_991. && n.fract() == 0.)
            .ok_or_else(|| ApiError::bad("match must be a match id."))?;
        Some(n as i64)
    } else {
        None
    };
    Ok(KillQuery {
        before,
        before_time,
        limit: Some(number("limit").unwrap_or(50.).trunc().clamp(1., 200.) as i64),
        count: fields.get("count").cloned(),
        player: fields.get("player").cloned(),
        killer: fields.get("killer").cloned(),
        victim: fields.get("victim").cloned(),
        cause: fields.get("cause").cloned(),
        kind: fields.get("kind").cloned(),
        min_m: number("minM").filter(|n| *n > 0.).map(f64::round),
        match_id,
    })
}
type Window = (i64, DateTime<Utc>, Option<DateTime<Utc>>);
fn text(value: &Option<String>, max: usize) -> String {
    value
        .as_deref()
        .unwrap_or("")
        .trim()
        .chars()
        .take(max)
        .collect()
}
fn side(q: &mut QueryBuilder<'_, Postgres>, needle: &str, killer: bool) {
    let (id, name) = if killer {
        ("killer_steam_id", "killer_name")
    } else {
        ("victim_steam_id", "victim_name")
    };
    if needle.len() == 17 && needle.bytes().all(|b| b.is_ascii_digit()) {
        q.push(id).push(" = ").push_bind(needle.to_owned());
    } else {
        q.push(name).push(" ILIKE ").push_bind(format!(
            "%{}%",
            needle
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        ));
    }
}
fn conditions(
    q: &mut QueryBuilder<'_, Postgres>,
    id: &str,
    f: &KillQuery,
    cursor: bool,
    window: &Option<Window>,
) {
    q.push(" WHERE server_id = ").push_bind(id.to_owned());
    if let Some((mid, from, to)) = window {
        q.push(" AND match_row = ")
            .push_bind(*mid)
            .push(" AND ts >= ")
            .push_bind(*from);
        if let Some(to) = to {
            q.push(" AND ts <= ").push_bind(*to);
        }
    }
    if cursor {
        if let Some(before) = f.before {
            if let Some(t) = f.before_time {
                q.push(" AND (ts,event_time) < (")
                    .push_bind(before)
                    .push(",")
                    .push_bind(t)
                    .push(")");
            } else {
                q.push(" AND ts < ").push_bind(before);
            }
        }
    }
    for (needle, killer) in [(text(&f.killer, 100), true), (text(&f.victim, 100), false)] {
        if !needle.is_empty() {
            q.push(" AND ");
            side(q, &needle, killer);
        }
    }
    let player = text(&f.player, 100);
    if !player.is_empty() {
        q.push(" AND (");
        side(q, &player, true);
        q.push(" OR ");
        side(q, &player, false);
        q.push(")");
    }
    let cause = text(&f.cause, 200);
    if !cause.is_empty() {
        q.push(" AND cause = ").push_bind(cause);
    }
    if let Some(min) = f.min_m {
        q.push(" AND distance_m >= ").push_bind(min.round());
    }
    match f.kind.as_deref() {
        Some("headshot") => {
            q.push(" AND headshot=true");
        }
        Some("teamKill") => {
            q.push(" AND team_kill=true");
        }
        Some("suicide") => {
            q.push(" AND suicide=true");
        }
        Some("environment") => {
            q.push(" AND killer_steam_id IS NULL");
        }
        Some("vehicle") => {
            q.push(" AND (cause ILIKE 'Vehicle.%' OR cause ILIKE 'Id.Vehicle.%' OR tags ?| ARRAY['VehicleExplosion','RoadKill'])");
        }
        _ => {}
    }
}
fn view(r: Value) -> Value {
    let date = |name: &str| {
        r.get(name)
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|s| s.to_utc().to_rfc3339_opts(SecondsFormat::Millis, true))
    };
    json!({"eventId":r["event_id"],"instanceId":r["instance_id"],"matchId":r["match_id"],"matchRow":r["match_row"],"ts":date("ts"),"map":r["map"],"eventTime":r["event_time"],
    "killer":if r["killer_steam_id"].is_null(){Value::Null}else{json!({"steamId":r["killer_steam_id"],"name":r["killer_name"].as_str().unwrap_or(""),"faction":r["killer_faction"]})},
    "victim":{"steamId":r["victim_steam_id"],"name":r["victim_name"],"faction":r["victim_faction"]},"factionBracketed":r["faction_bracketed"],"factionObservedAt":date("faction_observed_at"),"cause":r["cause"],"distanceM":r["distance_m"],"distanceInvalid":r["distance_invalid"],"rawDistanceCm":r["raw_distance_cm"],"headshot":r["headshot"],"suicide":r["suicide"],"teamKill":r["team_kill"],"tags":r.get("tags").filter(|v|v.is_array()).cloned().unwrap_or(json!([]))})
}
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let f = parse_query(raw)?;
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    server_scope(&state, &actor, &id, "server.view").await?;
    if f.before_time.is_some_and(|n| !n.is_finite()) {
        return Err(ApiError::bad("beforeTime must be a number."));
    }
    let window = if let Some(mid) = f.match_id {
        if mid <= 0 {
            return Err(ApiError::bad("match must be a match id."));
        }
        let row =
            sqlx::query("SELECT started_at,ended_at FROM matches WHERE server_id=$1 AND id=$2")
                .bind(&id)
                .bind(mid)
                .fetch_optional(&state.db)
                .await?
                .ok_or_else(ApiError::missing)?;
        Some((mid, row.try_get("started_at")?, row.try_get("ended_at")?))
    } else {
        None
    };
    let mut q = QueryBuilder::new("SELECT to_jsonb(kills) AS data FROM kills");
    conditions(&mut q, &id, &f, true, &window);
    q.push(" ORDER BY ts DESC,event_time DESC LIMIT ")
        .push_bind(f.limit.unwrap_or(50).clamp(1, 200));
    let rows = q.build().fetch_all(&state.db).await?;
    let kills: Vec<Value> = rows
        .iter()
        .map(|r| r.try_get::<Value, _>("data").map(view))
        .collect::<std::result::Result<_, _>>()?;
    let total = if f.count.as_deref() == Some("1") {
        let mut count = QueryBuilder::new("SELECT count(*) FROM kills");
        conditions(&mut count, &id, &f, false, &window);
        Some(
            count
                .build_query_scalar::<i64>()
                .fetch_one(&state.db)
                .await?,
        )
    } else {
        None
    };
    let row=sqlx::query("SELECT s.feed_token_hash,l.feed_at FROM servers s LEFT JOIN server_live l ON l.server_id=s.id WHERE s.id=$1").bind(&id).fetch_one(&state.db).await?;
    let feed: Option<DateTime<Utc>> = row.try_get("feed_at")?;
    Ok(Json(
        json!({"ok":true,"configured":row.try_get::<Option<String>,_>("feed_token_hash")?.is_some(),"feedAt":feed.map(|t|t.to_rfc3339_opts(SecondsFormat::Millis,true)),"kills":kills,"total":total}),
    ))
}
