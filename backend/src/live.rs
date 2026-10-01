//! Compatibility shaping for persisted status rows and authenticated realtime snapshots.
use crate::{config::AppState, error::Result};
use serde_json::{Value, json};
use std::collections::HashMap;
pub fn finite(v: &Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return n.is_finite().then_some(n);
    }
    let s = v.as_str()?;
    let s = s.strip_prefix('-').unwrap_or(s);
    let mut parts = s.split('.');
    let whole = parts.next()?;
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(frac) = parts.next() {
        if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    if parts.next().is_some() {
        return None;
    }
    v.as_str()?.parse::<f64>().ok().filter(|n| n.is_finite())
}
pub fn status(v: &Value) -> Value {
    if !v.is_object() || !v["serverName"].is_string() || !v["map"].is_string() {
        return Value::Null;
    }
    let raw = v["scores"].as_array().cloned().unwrap_or_default();
    let scores = raw
        .iter()
        .filter_map(|s| {
            let name = s["name"].as_str().filter(|s| !s.is_empty())?;
            let score = finite(&s["score"])?;
            // The first occurrence decides the colour, as in the original normalizer.
            let colour = raw
                .iter()
                .find(|s| s["name"] == name)
                .and_then(|s| s["colorHex"].as_str())
                .filter(|s| {
                    s.len() == 7
                        && s.starts_with('#')
                        && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
                })
                .unwrap_or("#888888");
            Some(json!({"name":name,"score":score,"colorHex":colour}))
        })
        .collect::<Vec<_>>();
    let mut out = json!({"serverName":v["serverName"],"map":v["map"],"experiences":v["experiences"].as_array().into_iter().flatten().filter(|s|s.is_string()).cloned().collect::<Vec<_>>(),"lighting":v["lighting"].as_str().unwrap_or(""),"alternator":v["alternator"].as_str().unwrap_or(""),"scores":scores,"playerCount":finite(&v["playerCount"]).unwrap_or(0.).max(0.),"maxPlayers":finite(&v["maxPlayers"]).unwrap_or(0.).max(0.),"rotationNow":finite(&v["rotationNow"]).unwrap_or(0.),"rotationNext":finite(&v["rotationNext"]).unwrap_or(0.)});
    for key in [
        "scoreTick",
        "scoreTickMin",
        "scoreTickMax",
        "scoreCap",
        "matchSeconds",
    ] {
        out[key] = json!(finite(&v[key]));
    }
    out
}
fn iso(v: &Value) -> Value {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|at| {
            json!(
                at.to_utc()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            )
        })
        .unwrap_or(Value::Null)
}
pub fn view(r: &Value) -> Value {
    json!({"serverId":r["server_id"],"ok":r["ok"],"error":r["error"].as_str().unwrap_or(""),"tier":r["tier"].as_str().unwrap_or("idle"),"build":r["build"].as_str().unwrap_or(""),"gameServerId":r["game_server_id"].as_str().unwrap_or(""),"startedAt":iso(&r["started_at"]),"reservedSlots":r["reserved_slots"],"throttledUntil":null,"status":status(&r["status"]),"players":r["players"].as_array().cloned().unwrap_or_default(),"statusAt":iso(&r["status_at"]),"playersAt":iso(&r["players_at"]),"observedAt":iso(&r["observed_at"])})
}
pub async fn read(state: &AppState, ids: &[String]) -> Result<HashMap<String, Value>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=ANY($1)")
            .bind(ids)
            .fetch_all(&state.db)
            .await?;
    Ok(rows
        .iter()
        .filter_map(|r| Some((r["server_id"].as_str()?.to_owned(), view(r))))
        .collect())
}
pub async fn kills(
    state: &AppState,
    id: &str,
    ids: &[String],
    ts: Option<&str>,
) -> Result<Vec<Value>> {
    let Some(at) = ts
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|at| at.to_utc())
    else {
        return Ok(vec![]);
    };
    if ids.is_empty() || ids.len() > 128 {
        return Ok(vec![]);
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND ts=$2 AND event_id=ANY($3) ORDER BY event_time DESC").bind(id).bind(at).bind(ids).fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(crate::api::kills::view).collect())
}
