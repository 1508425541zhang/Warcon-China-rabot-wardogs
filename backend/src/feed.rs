//! Original feed envelope, distance validation, roster attribution and atomic dual-consumer writes.
use crate::error::{ApiError, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Kill {
    pub event_id: String,
    pub match_id: String,
    pub event_time: f64,
    pub map: String,
    pub killer_steam_id: Option<String>,
    pub killer_name: Option<String>,
    pub victim_steam_id: String,
    pub victim_name: String,
    pub cause: Option<String>,
    pub distance_m: Option<f64>,
    pub distance_invalid: bool,
    pub raw_distance_cm: Option<f64>,
    pub headshot: bool,
    pub suicide: bool,
    pub tags: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub instance_id: String,
    pub server_name: String,
    pub kills: Vec<Kill>,
    pub skipped: usize,
}
pub fn truncate(value: &str, max: usize) -> String {
    String::from_utf16_lossy(&value.encode_utf16().take(max).collect::<Vec<_>>())
}
fn string(value: &Value, max: usize) -> String {
    truncate(value.as_str().unwrap_or(""), max)
}
fn valid_id(id: &str) -> bool {
    id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit())
}
pub fn map_id(name: &str) -> String {
    let pairs = [
        ("Kavkazi", "Bakurani"),
        ("Europe", "Ozeti"),
        ("NorthAmerica", "Zestafona"),
    ];
    if pairs.iter().any(|(id, _)| *id == name) {
        return name.into();
    }
    let mut shown = String::new();
    let mut previous = None;
    let mut separator = false;
    for c in name.chars() {
        if c == '_' || c == '-' {
            if !separator {
                shown.push(' ')
            }
            separator = true;
            previous = Some(c);
            continue;
        }
        separator = false;
        if c.is_ascii_uppercase()
            && previous.is_some_and(|p: char| p.is_ascii_lowercase() || p.is_ascii_digit())
        {
            shown.push(' ')
        }
        shown.push(c);
        previous = Some(c)
    }
    pairs
        .iter()
        .find(|(_, display)| display.eq_ignore_ascii_case(shown.trim()))
        .map(|(id, _)| id.to_string())
        .unwrap_or_else(|| name.into())
}
pub fn bearer(header: &str) -> Option<&str> {
    let mut fields = header.split_whitespace();
    let scheme = fields.next()?;
    let token = fields.next()?;
    if header.starts_with(char::is_whitespace)
        || !scheme.eq_ignore_ascii_case("Bearer")
        || fields.next().is_some()
        || token.len() != 47
        || !token.starts_with("wkf_")
    {
        return None;
    }
    token[4..]
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        .then_some(token)
}
pub fn parse_kill(value: &Value) -> Option<Kill> {
    if value["type"] != "killed" {
        return None;
    }
    let event_id = string(&value["eventId"], 64);
    let victim = string(&value["victimSteamId"], 17);
    let event_time = value["eventTime"].as_f64().filter(|n| n.is_finite())?;
    if event_id.is_empty() || !valid_id(&victim) {
        return None;
    }
    let killer = string(&value["killerSteamId"], 17);
    let mut tags = Vec::new();
    for tag in value["contextTags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let mut tag = tag;
        for prefix in [
            "Meta.Progression.Context.Player.KillContext.",
            "Meta.PlayerKillFlag.Player.",
        ] {
            if let Some(short) = tag.strip_prefix(prefix) {
                tag = short
            }
        }
        if ["Local.Kill", "Local.Death"].contains(&tag) || tags.iter().any(|s| s == tag) {
            continue;
        }
        tags.push(truncate(tag, 60));
    }
    let headshot = tags.iter().any(|s| s == "Headshot");
    let suicide = tags.iter().any(|s| s == "Suicide") || (!killer.is_empty() && killer == victim);
    tags.retain(|s| s != "Headshot" && s != "Suicide");
    let raw = value["distance"].as_f64().filter(|n| n.is_finite());
    let invalid = raw.is_some_and(|n| n <= 0. || n >= 500_000.);
    let cause = string(&value["cause"], 200);
    Some(Kill {
        event_id,
        match_id: string(&value["matchId"], 64),
        event_time,
        map: map_id(&string(&value["mapName"], 64)),
        killer_steam_id: valid_id(&killer).then_some(killer.clone()),
        killer_name: valid_id(&killer).then(|| string(&value["killerName"], 200)),
        victim_steam_id: victim,
        victim_name: string(&value["victimName"], 200),
        cause: (!cause.is_empty()).then_some(cause),
        distance_m: raw.filter(|_| !invalid).map(|n| (n + 0.5).floor() / 100.),
        distance_invalid: invalid,
        raw_distance_cm: raw,
        headshot,
        suicide,
        tags,
    })
}
pub fn parse_batch(body: &Value) -> Result<Batch> {
    let events = body["events"]
        .as_array()
        .ok_or_else(|| ApiError::bad("Expected { serverId, serverName, events: [] }."))?;
    if events.len() > 200 {
        return Err(ApiError::bad(format!(
            "Too many events in one batch ({}).",
            events.len()
        )));
    }
    let kills: Vec<_> = events.iter().filter_map(parse_kill).collect();
    let skipped = events.len() - kills.len();
    Ok(Batch {
        instance_id: string(&body["serverId"], 64),
        server_name: string(&body["serverName"], 200),
        kills,
        skipped,
    })
}
pub fn roster(
    players: &Value,
    players_at: Option<DateTime<Utc>>,
    status: &Value,
    status_at: Option<DateTime<Utc>>,
    map: &str,
) -> Option<HashMap<String, String>> {
    let (players_at, status_at) = (players_at?, status_at?);
    if status_at > players_at
        || (players_at - status_at).num_milliseconds() > 10000
        || map_id(status["map"].as_str()?) != map_id(map)
    {
        return None;
    }
    let teams: HashSet<_> = status["scores"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["name"].as_str())
        .collect();
    let players = players.as_array()?;
    let mut factions = HashMap::new();
    for player in players {
        if let (Some(id), Some(faction)) = (player["steamId"].as_str(), player["faction"].as_str())
        {
            if !faction.is_empty()
                && faction != "White"
                && (teams.is_empty() || teams.contains(faction))
            {
                factions.insert(id.into(), faction.into());
            }
        }
    }
    Some(factions)
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub accepted: usize,
    pub skipped: usize,
    pub duplicates: usize,
}
pub async fn ingest(db: &PgPool, server: &str, body: Value, now: DateTime<Utc>) -> Result<Receipt> {
    let batch = parse_batch(&body)?;
    let mut tx = db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("feed:{server}"))
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO training_feed_batches(server_id,received_at,instance_id,payload) VALUES($1,$2,$3,$4)").bind(server).bind(now).bind(&batch.instance_id).bind(body).execute(&mut *tx).await?;
    let ids: Vec<_> = batch.kills.iter().map(|k| k.event_id.clone()).collect();
    let mut seen: HashSet<String> = sqlx::query_scalar::<_, String>(
        "SELECT event_id FROM kills WHERE server_id=$1 AND event_id=ANY($2) AND ts>$3",
    )
    .bind(server)
    .bind(&ids)
    .bind(now - chrono::Duration::hours(24))
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let live = sqlx::query(
        "SELECT players,players_at,status,status_at FROM server_live WHERE server_id=$1",
    )
    .bind(server)
    .fetch_optional(&mut *tx)
    .await?;
    let match_row: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM matches WHERE server_id=$1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1",
    )
    .bind(server)
    .fetch_optional(&mut *tx)
    .await?;
    let mut factions = HashMap::new();
    let mut observed_at = None;
    if let Some(live) = live {
        let players_at: Option<DateTime<Utc>> = live.try_get("players_at")?;
        let status_at: Option<DateTime<Utc>> = live.try_get("status_at")?;
        if players_at.is_some_and(|at| at <= now && (now - at).num_milliseconds() <= 10000) {
            let players: Option<Value> = live.try_get("players")?;
            let status: Option<Value> = live.try_get("status")?;
            for k in &batch.kills {
                factions.insert(
                    k.map.clone(),
                    roster(
                        players.as_ref().unwrap_or(&Value::Null),
                        players_at,
                        status.as_ref().unwrap_or(&Value::Null),
                        status_at,
                        &k.map,
                    ),
                );
            }
            observed_at = players_at;
        }
    }
    let mut accepted = Vec::new();
    let mut duplicates = 0;
    for k in &batch.kills {
        if !seen.insert(k.event_id.clone()) {
            duplicates += 1;
            continue;
        }
        let roster = factions.get(&k.map).and_then(Option::as_ref);
        let killer = k
            .killer_steam_id
            .as_ref()
            .and_then(|id| roster.and_then(|r| r.get(id)));
        let victim = roster.and_then(|r| r.get(&k.victim_steam_id));
        let team_kill = killer.is_some()
            && killer == victim
            && k.killer_steam_id.as_deref() != Some(k.victim_steam_id.as_str());
        // Preserve an out-of-real-range number in payload; derived float4 cannot represent it.
        let raw_distance = k.raw_distance_cm.filter(|n| n.abs() <= f32::MAX as f64);
        sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,match_row,event_time,map,killer_steam_id,killer_name,killer_faction,faction_observed_at,victim_steam_id,victim_name,victim_faction,cause,distance_m,distance_invalid,raw_distance_cm,headshot,suicide,team_kill,tags) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)")
            .bind(now).bind(server).bind(&k.event_id).bind(&batch.instance_id).bind(&k.match_id).bind(match_row).bind(k.event_time).bind(&k.map).bind(&k.killer_steam_id).bind(&k.killer_name).bind(killer).bind(if killer.is_some() && victim.is_some(){observed_at}else{None}).bind(&k.victim_steam_id).bind(&k.victim_name).bind(victim).bind(&k.cause).bind(k.distance_m).bind(k.distance_invalid).bind(raw_distance).bind(k.headshot).bind(k.suicide).bind(team_kill).bind(serde_json::json!(k.tags)).execute(&mut *tx).await?;
        accepted.push(k.event_id.clone());
    }
    if !accepted.is_empty() {
        for consumer in ["legacy", "integrity"] {
            sqlx::query("INSERT INTO feed_processing_jobs(server_id,consumer,kill_ts,event_ids,created_at) VALUES($1,$2,$3,$4,$3)").bind(server).bind(consumer).bind(now).bind(serde_json::json!(accepted)).execute(&mut *tx).await?;
        }
    }
    sqlx::query("INSERT INTO server_live(server_id,feed_at) VALUES($1,$2) ON CONFLICT(server_id) DO UPDATE SET feed_at=excluded.feed_at WHERE server_live.feed_at IS NULL OR server_live.feed_at<excluded.feed_at-interval '10 seconds'").bind(server).bind(now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Receipt {
        accepted: accepted.len(),
        skipped: batch.skipped,
        duplicates,
    })
}
