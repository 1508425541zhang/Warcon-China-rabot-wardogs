//! Poller automation. Prepare reads outside the fenced observation transaction; commit carries state.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    observation_state::{self, PresenceDiff},
    observer::Memory,
    trigger_policy as p,
    trigger_store::{self, Intent},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use std::collections::{HashMap, HashSet};
#[derive(Clone, Default)]
pub struct RuleMemory {
    pub rows: Vec<Value>,
    pub loaded: i64,
    pub reserved: HashSet<String>,
    pub reserved_loaded: bool,
    pub inputs: HashMap<String, Value>,
    pub risk_due: i64,
    pub risk_ids: HashSet<String>,
    pub risk_attempts: HashMap<String, i64>,
    pub seed_state: HashMap<String, Value>,
    pub held: HashMap<String, Value>,
}
pub async fn prepare(
    app: &AppState,
    m: &mut Memory,
    diff: &PresenceDiff,
    trusted: bool,
    fresh: bool,
    ts: i64,
) -> Result<()> {
    if ts - m.automation.loaded >= 10000 {
        m.automation.rows=sqlx::query_scalar("SELECT to_jsonb(t) FROM triggers t WHERE server_id=$1 AND enabled ORDER BY created_at,id").bind(&m.id).fetch_all(&app.db).await?;
        m.automation.rows.retain_mut(|row| {
            match p::validate(row["kind"].as_str().unwrap_or(""), &row["config"]) {
                Ok(config) => {
                    row["config"] = config;
                    true
                }
                Err(_) => {
                    tracing::warn!(server=%m.id,trigger=?row["id"],"Invalid trigger ignored");
                    false
                }
            }
        });
        m.automation.loaded = ts;
        let active: HashSet<_> = m
            .automation
            .rows
            .iter()
            .filter_map(|r| r["id"].as_str())
            .map(str::to_owned)
            .collect();
        m.automation.held.retain(|k, _| active.contains(k));
        m.automation.seed_state.retain(|k, _| active.contains(k));
    }
    if m.automation.rows.is_empty() {
        m.automation.inputs.clear();
        return Ok(());
    }
    m.automation.reserved_loaded = m.lists_loaded;
    let reserved: Vec<String> =
        sqlx::query_scalar("SELECT steam_id FROM server_reserved WHERE server_id=$1")
            .bind(&m.id)
            .fetch_all(&app.db)
            .await?;
    m.automation.reserved = reserved.into_iter().collect();
    m.automation.risk_ids.clear();
    if fresh && m.automation.rows.iter().any(|r| r["kind"] == "risk_kick") {
        let sweep = ts >= m.automation.risk_due;
        if sweep {
            m.automation.risk_due = ts + 60000
        }
        for player in diff
            .joined
            .iter()
            .filter(|_| trusted)
            .chain(diff.returned.iter())
            .chain(diff.stayed.iter().filter(|_| sweep))
        {
            m.automation.risk_ids.insert(player.steam_id.clone());
        }
        let players: Vec<Value> = m
            .players
            .iter()
            .filter(|p| m.automation.risk_ids.contains(&p.steam_id))
            .map(|p| json!(p))
            .collect();
        if !players.is_empty() {
            let ids: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM servers WHERE org_id=$1 ORDER BY sort_order,name",
            )
            .bind(&m.org)
            .fetch_all(&app.db)
            .await?;
            let names = m
                .automation
                .rows
                .iter()
                .any(|r| r["kind"] == "risk_kick" && p::risk_score(&r["config"]).is_some());
            m.automation.inputs =
                crate::player_risk::inputs(app, &m.org, &ids, Some(&m.id), &players, names).await?;
        }
    }
    Ok(())
}
pub fn accrue(m: &mut Memory, diff: &PresenceDiff, trusted: bool, fresh: bool, gap: i64) {
    if !fresh {
        return;
    }
    let seed = m
        .automation
        .rows
        .iter()
        .filter(|r| r["kind"] == "seed_reward")
        .max_by_key(|r| r["config"]["lowAt"].as_i64().unwrap_or(0));
    if let Some(seed) = seed {
        let c = &seed["config"];
        let full = c["fullAt"]
            .as_i64()
            .or(m.status["maxPlayers"].as_i64().filter(|n| *n > 0));
        let count = m.players.len() as i64;
        for player in &diff.stayed {
            if let Some(session) = m
                .sessions
                .as_mut()
                .and_then(|s| s.get_mut(&player.steam_id))
            {
                if full.is_some_and(|f| count >= f) {
                    session.seed_ms += session.pending_seed_ms;
                    session.pending_seed_ms = 0
                } else if count <= c["lowAt"].as_i64().unwrap_or(20) && trusted {
                    if c["untilFull"] != false {
                        session.pending_seed_ms += gap
                    } else {
                        session.seed_ms += gap
                    }
                }
            }
        }
    }
}
fn vars(m: &Memory, player: Option<&Value>, previous: &str) -> Value {
    json!({"name":player.map(|p|p["name"].clone()).unwrap_or(json!("")),"faction":player.and_then(|p|p["faction"].as_str()).unwrap_or(""),"previous":previous,"server":m.status["serverName"].as_str().filter(|s|!s.is_empty()).unwrap_or(&m.name),"map":m.status["map"],"players":m.status["playerCount"],"max":m.status["maxPlayers"],"cap":m.status["scoreCap"].as_f64().unwrap_or(100.)})
}
fn intent(
    row: &Value,
    action: &str,
    params: Value,
    target: String,
    detail: Value,
    steam: Option<String>,
    suffix: &str,
) -> Intent {
    Intent {
        action: action.into(),
        params,
        target,
        detail,
        steam,
        key: format!("{}:{suffix}", row["id"].as_str().unwrap_or("")),
        ok: format!("{}：{}", row["name"].as_str().unwrap_or(""), action),
    }
}
fn whisper(
    row: &Value,
    m: &Memory,
    player: &Value,
    template: &str,
    previous: &str,
    ts: i64,
) -> Intent {
    let id = player["steamId"].as_str().unwrap_or("");
    intent(
        row,
        "whisper",
        json!({"steamId":id,"message":p::render(template,&vars(m,Some(player),previous))}),
        id.into(),
        json!({"name":player["name"]}),
        Some(id.into()),
        &format!("{id}:{ts}"),
    )
}
pub fn on_target(c: &Value, status: &Value) -> bool {
    let mut wanted: Vec<_> = c["experiences"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut actual: Vec<_> = status["experiences"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    wanted.sort();
    actual.sort();
    c["map"] == status["map"] && (wanted.is_empty() || wanted == actual)
}
pub fn uptime(ms: i64) -> String {
    let mins = ms.max(0) / 60000;
    let d = mins / 1440;
    let h = mins % 1440 / 60;
    let m = mins % 60;
    if mins < 1 {
        "<1m".into()
    } else if d > 0 {
        if h > 0 {
            format!("{d}d {h}h")
        } else {
            format!("{d}d")
        }
    } else if h > 0 {
        if m > 0 {
            format!("{h}h {m}m")
        } else {
            format!("{h}h")
        }
    } else {
        format!("{m}m")
    }
}
async fn award_lines(
    tx: &mut Transaction<'_, Postgres>,
    m: &Memory,
    end: &Value,
    previous: i64,
) -> Result<Vec<Value>> {
    let round:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(m) FROM matches m WHERE server_id=$1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1 FOR UPDATE").bind(&m.id).fetch_optional(&mut **tx).await?;
    let Some(round) = round.filter(|r| r["map"] == end["map"]) else {
        return Ok(vec![]);
    };
    if let Some(a) = round["award_snapshot"].as_array() {
        return Ok(a.clone());
    }
    let id = round["id"].as_i64().unwrap();
    let saved: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM match_players p WHERE match_id=$1")
            .bind(id)
            .fetch_all(&mut **tx)
            .await?;
    let mut rows: HashMap<String, Value> = saved
        .into_iter()
        .map(crate::ai_evidence::row)
        .filter_map(|v| v["steamId"].as_str().map(|s| (s.to_owned(), v.clone())))
        .collect();
    let (memory, _) = observation_state::close(&m.tallies, previous);
    for p in memory {
        let p = json!(p);
        if let Some(steam) = p["steamId"].as_str() {
            rows.insert(steam.into(), p);
        }
    }
    let raw:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND match_row=$2 AND ts >= $3::timestamptz-interval '120 seconds' ORDER BY event_time,ts,event_id").bind(&m.id).bind(id).bind(crate::integrity_enforcement::date(&round["started_at"])).fetch_all(&mut **tx).await?;
    let events: Vec<_> = raw.into_iter().map(crate::ai_evidence::row).collect();
    let record = crate::match_feed::record(&events);
    let mut multis: HashMap<String, i64> = HashMap::new();
    let mut maxima: HashMap<String, i64> = HashMap::new();
    for event in &events {
        if event["suicide"] == true
            || event["teamKill"] == true
            || event["killerSteamId"] == event["victimSteamId"]
        {
            continue;
        }
        if let Some(killer) = event["killerSteamId"].as_str() {
            let key = format!("{}:{killer}:{}", event["instanceId"], event["eventTime"]);
            let n = multis.entry(key).or_default();
            *n += 1;
            let max = maxima.entry(killer.into()).or_default();
            *max = (*max).max(*n)
        }
    }
    let progress:Option<Value>=sqlx::query_scalar("SELECT players FROM player_progress_samples WHERE server_id=$1 AND match_id=$2 ORDER BY observed_at DESC LIMIT 1").bind(&m.id).bind(id).fetch_optional(&mut **tx).await?;
    let seed = uuid::Uuid::new_v4().to_string();
    let mut result = vec![];
    for (steam, mut row) in rows {
        row["awardSeed"] = json!(seed);
        row["multi"] = json!(maxima.get(&steam));
        row["streak"] = record[&steam]["killStreak"].clone();
        if row["cashHeld"].is_null() {
            row["cashHeld"] = progress
                .as_ref()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|p| p["steamId"] == steam)
                .map(|p| p["cash"].clone())
                .unwrap_or(Value::Null)
        }
        result.push(row)
    }
    result.sort_by(|a, b| a["steamId"].as_str().cmp(&b["steamId"].as_str()));
    sqlx::query("UPDATE matches SET award_snapshot=$2 WHERE id=$1 AND award_snapshot IS NULL")
        .bind(id)
        .bind(json!(result))
        .execute(&mut **tx)
        .await?;
    Ok(result)
}
pub async fn evaluate(
    tx: &mut Transaction<'_, Postgres>,
    m: &mut Memory,
    diff: &PresenceDiff,
    trusted: bool,
    fresh: bool,
    end: Option<&Value>,
    previous_status: i64,
    ts: i64,
) -> Result<()> {
    let at = DateTime::from_timestamp_millis(ts).ok_or_else(|| ApiError::bad("观察时间无效。"))?;
    let players: Vec<Value> = m.players.iter().map(|p| json!(p)).collect();
    let mut rows = m.automation.rows.clone();
    let mut award = None;
    for row in &mut rows {
        let kind = row["kind"].as_str().unwrap_or("");
        let c = &row["config"];
        let id = row["id"].as_str().unwrap_or("").to_owned();
        let last = crate::integrity_enforcement::date(&row["last_fired_at"])
            .map(|t| t.timestamp_millis())
            .unwrap_or(0);
        let mut intents = vec![];
        let mut next_state = None;
        match kind {
            "welcome" | "faction_change" => {
                let mut picks: Vec<Value> = if trusted {
                    diff.factioned.clone()
                } else {
                    vec![]
                };
                if trusted {
                    picks.extend(
                        diff.joined
                            .iter()
                            .filter(|p| p.faction.as_ref().is_some_and(|f| !f.is_empty()))
                            .map(|p| json!({"player":p,"from":null})),
                    )
                }
                let targets: Vec<(Value, String)> = if kind == "faction_change" {
                    picks
                        .iter()
                        .filter_map(|f| {
                            f["from"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(|s| (f["player"].clone(), s.into()))
                        })
                        .collect()
                } else if c["afterFaction"] == true {
                    picks
                        .iter()
                        .filter(|f| f["from"].is_null() || f["from"] == "")
                        .map(|f| (f["player"].clone(), String::new()))
                        .collect()
                } else {
                    diff.joined
                        .iter()
                        .filter(|_| trusted)
                        .map(|p| (json!(p), String::new()))
                        .collect()
                };
                for (player, previous) in targets {
                    let steam = player["steamId"].as_str().unwrap_or("");
                    if kind == "welcome"
                        && c["onlyFirstVisit"] == true
                        && m.sessions
                            .as_ref()
                            .and_then(|s| s.get(steam))
                            .is_none_or(|s| !s.first_visit)
                    {
                        continue;
                    }
                    intents.push(whisper(
                        row,
                        m,
                        &player,
                        c["message"].as_str().unwrap_or(""),
                        &previous,
                        ts,
                    ));
                }
            }
            "broadcast" => {
                let count = m.status["playerCount"]
                    .as_i64()
                    .unwrap_or(players.len() as i64);
                if p::wanted(c, count)
                    && (last == 0 || ts - last >= c["everyMinutes"].as_i64().unwrap_or(1) * 60000)
                {
                    if let Some(messages) = c["messages"].as_array().filter(|a| !a.is_empty()) {
                        let index =
                            row["state"]["index"].as_u64().unwrap_or(0) % messages.len() as u64;
                        let message = p::render(
                            messages[index as usize].as_str().unwrap_or(""),
                            &vars(m, None, ""),
                        );
                        intents.push(intent(
                            row,
                            "broadcast",
                            json!({"message":message}),
                            message.clone(),
                            json!({"index":index}),
                            None,
                            &ts.to_string(),
                        ));
                        next_state = Some(json!({"index":index+1}));
                    }
                }
            }
            "empty_reset" => {
                if players.is_empty()
                    && m.status["playerCount"].as_i64().unwrap_or(0) == 0
                    && !on_target(c, &m.status)
                    && (last == 0
                        || ts - last >= c["cooldownMinutes"].as_i64().unwrap_or(30) * 60000)
                {
                    let since:Option<DateTime<Utc>>=sqlx::query_scalar("SELECT COALESCE((SELECT max(ts) FROM samples WHERE server_id=$1 AND(NOT ok OR player_count>0)),(SELECT min(ts) FROM samples WHERE server_id=$1))").bind(&m.id).fetch_one(&mut **tx).await?;
                    if since.is_some_and(|s| {
                        ts - s.timestamp_millis() >= c["afterMinutes"].as_i64().unwrap_or(1) * 60000
                    }) {
                        let mut params = json!({"map":c["map"],"experiences":c["experiences"]});
                        for key in ["lighting", "zoneAlternator"] {
                            if c[key].as_str().is_some_and(|s| !s.is_empty()) {
                                params[key] = c[key].clone()
                            }
                        }
                        intents.push(intent(row,"empty_reset",params,c["map"].as_str().unwrap_or("").into(),json!({"emptyMinutes":((ts-since.unwrap().timestamp_millis())as f64/60000.).round()}),None,&ts.to_string()));
                    }
                }
            }
            "risk_kick" => {
                if c["spareReserved"] == true && !m.automation.reserved_loaded {
                    continue;
                }
                for player in &players {
                    let steam = player["steamId"].as_str().unwrap_or("");
                    if !m.automation.risk_ids.contains(steam) {
                        continue;
                    }
                    let key = format!("{id}:{steam}");
                    if m.automation
                        .risk_attempts
                        .get(&key)
                        .is_some_and(|at| ts - at < 300000)
                        && !diff
                            .joined
                            .iter()
                            .chain(&diff.returned)
                            .any(|p| p.steam_id == steam)
                    {
                        continue;
                    }
                    let Some(s) = m.automation.inputs.get(steam) else {
                        continue;
                    };
                    if let Some(verdict) = crate::player_risk::verdict(c, s, ts) {
                        intents.push(intent(
                            row,
                            "kick",
                            json!({"steamId":steam,"reason":c["reason"]}),
                            steam.into(),
                            json!({"name":player["name"],"verdict":verdict}),
                            Some(steam.into()),
                            &format!("{steam}:{ts}"),
                        ));
                        m.automation.risk_attempts.insert(key, ts);
                    }
                }
            }
            "name_filter" => {
                let mut named: HashMap<String, Value> = HashMap::new();
                for player in diff
                    .joined
                    .iter()
                    .filter(|_| trusted)
                    .chain(diff.renamed.iter())
                    .chain(diff.returned.iter().filter(|_| c["action"] == "kick"))
                {
                    named.insert(player.steam_id.clone(), json!(player));
                }
                for (steam, player) in named {
                    if c["spareReserved"] == true && m.automation.reserved.contains(&steam) {
                        continue;
                    }
                    if let Some(verdict) =
                        crate::name_filter::verdict(c, player["name"].as_str().unwrap_or(""))
                    {
                        let kick = c["action"] == "kick";
                        let mut args = vars(m, Some(&player), "");
                        args["why"] = verdict["why"].clone();
                        intents.push(intent(row,if kick{"kick"}else{"name_flag"},if kick{json!({"steamId":steam,"reason":p::render(c["reason"].as_str().unwrap_or(""),&args)})}else{json!({})},steam.clone(),json!({"name":player["name"],"verdict":verdict["verdict"]}),Some(steam.clone()),&format!("{steam}:{ts}")));
                    }
                }
            }
            "ping_kick" if fresh => {
                let step = p::ping_step(
                    c,
                    &row["state"],
                    &players,
                    ts,
                    2 * m.players_interval.max(1000) + 1000,
                );
                for steam in step["kicks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    let player = players.iter().find(|p| p["steamId"] == steam).unwrap();
                    let since = step["state"]["players"][steam]["since"]
                        .as_i64()
                        .unwrap_or(ts);
                    intents.push(intent(
                        row,
                        "kick",
                        json!({"steamId":steam,"reason":c["reason"]}),
                        steam.into(),
                        json!({"name":player["name"],"pingMs":player["ping"]}),
                        Some(steam.into()),
                        &format!("{steam}:{since}"),
                    ));
                }
                if step["state"]["players"]
                    .as_object()
                    .is_some_and(|a| !a.is_empty())
                    || row["state"]["players"]
                        .as_object()
                        .is_some_and(|a| !a.is_empty())
                {
                    next_state = Some(step["state"].clone());
                }
            }
            "restart_notice" => {
                if let Some(stage) = p::restart_stage(
                    c,
                    &row["state"],
                    m.started_at,
                    m.status["playerCount"].as_i64().unwrap_or(0),
                    ts,
                ) {
                    let mut args = vars(m, None, "");
                    args["minutes"] = stage["minutes"].clone();
                    args["uptime"] = json!(uptime(ts - m.started_at));
                    let message = p::render(
                        if stage["stage"] == "lead" {
                            c["leadMessage"].as_str()
                        } else {
                            c["message"].as_str()
                        }
                        .unwrap_or(""),
                        &args,
                    );
                    intents.push(intent(
                        row,
                        "broadcast",
                        json!({"message":message}),
                        message,
                        json!({"stage":stage["stage"],"startedAt":m.started_at}),
                        None,
                        &format!(
                            "{}:{}:{}",
                            stage["stage"],
                            stage["state"]["startedAt"],
                            if stage["stage"] == "due" { ts } else { 0 }
                        ),
                    ));
                    next_state = Some(stage["state"].clone());
                }
            }
            "match_broadcast" => {
                let count = m.status["playerCount"].as_i64().unwrap_or(0);
                let held = m
                    .automation
                    .held
                    .get(&id)
                    .filter(|h| ts - h["at"].as_i64().unwrap_or(0) <= 180000)
                    .cloned();
                let held = if let Some(end) = end {
                    if held.is_some() && end["leaders"].as_array().is_none_or(|a| a.is_empty()) {
                        held
                    } else {
                        if award.is_none() {
                            award = Some(award_lines(tx, m, end, previous_status).await?)
                        }
                        Some(json!({"end":end,"lines":award,"at":ts}))
                    }
                } else {
                    held
                };
                if let Some(held) = held {
                    if count < c["minPlayers"].as_i64().unwrap_or(1) {
                        m.automation.held.insert(id.clone(), held);
                    } else {
                        let lines = held["lines"].as_array().cloned().unwrap_or_default();
                        for send in
                            p::match_messages(c, &held["end"], count, &vars(m, None, ""), &lines)
                        {
                            let message = send["message"].as_str().unwrap_or("");
                            intents.push(intent(row,"broadcast",json!({"message":message}),message.into(),json!({"stage":send["stage"],"map":held["end"]["map"],"winner":held["end"]["winner"],"scores":held["end"]["scores"]}),None,&format!("{}:{}",send["stage"].as_str().unwrap_or(""),held["at"])));
                        }
                        m.automation.held.remove(&id);
                    }
                } else {
                    m.automation.held.remove(&id);
                }
            }
            "seed_reward" => {
                if !m.automation.reserved_loaded {
                    continue;
                }
                let prior = m
                    .automation
                    .seed_state
                    .get(&id)
                    .cloned()
                    .unwrap_or(json!({"checkedAt":0,"low":false,"full":false}));
                let low = players.len() as i64 <= c["lowAt"].as_i64().unwrap_or(20);
                let full = c["fullAt"]
                    .as_i64()
                    .or(m.status["maxPlayers"].as_i64().filter(|n| *n > 0))
                    .is_some_and(|n| players.len() as i64 >= n);
                let due = if low {
                    ts - prior["checkedAt"].as_i64().unwrap_or(0) >= 60000
                } else if c["untilFull"] != false {
                    full && prior["full"] != true
                } else {
                    prior["low"] == true
                };
                m.automation.seed_state.insert(id.clone(),json!({"checkedAt":if due{ts}else{prior["checkedAt"].as_i64().unwrap_or(0)},"low":low,"full":full}));
                if !due {
                    continue;
                }
                let earlier:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',steam_id,'seconds',sum(seed_seconds)) FROM player_sessions WHERE server_id=$1 AND left_at IS NOT NULL AND last_seen>=$2 GROUP BY steam_id").bind(&m.id).bind(at-chrono::Duration::days(c["windowDays"].as_i64().unwrap_or(7))).fetch_all(&mut **tx).await?;
                for player in &players {
                    let steam = player["steamId"].as_str().unwrap_or("");
                    if m.automation.reserved.contains(steam) {
                        continue;
                    }
                    let seconds = earlier
                        .iter()
                        .find(|e| e["steamId"] == steam)
                        .and_then(|e| e["seconds"].as_i64())
                        .unwrap_or(0)
                        + m.sessions
                            .as_ref()
                            .and_then(|s| s.get(steam))
                            .map(|s| s.seed_ms / 1000)
                            .unwrap_or(0);
                    if seconds < c["minutes"].as_i64().unwrap_or(1) * 60 {
                        continue;
                    }
                    let minutes = seconds / 60;
                    let reason = format!(
                        "Seeded {}: {minutes} min with {} or fewer on",
                        m.name, c["lowAt"]
                    );
                    intents.push(intent(row,"seed_reward",json!({"steamId":steam,"name":player["name"],"reason":reason,"slotDays":c["slotDays"],"scope":if c["scope"]=="server"{"server"}else{"org"}}),steam.into(),json!({"name":player["name"],"minutes":minutes,"slotDays":c["slotDays"]}),None,&format!("{steam}:{ts}")));
                    if c["message"].as_str().is_some_and(|s| !s.is_empty()) {
                        let mut args = vars(m, Some(player), "");
                        args["minutes"] = json!(minutes);
                        args["days"] = c["slotDays"].clone();
                        args["until"] = json!(
                            (at + chrono::Duration::days(c["slotDays"].as_i64().unwrap_or(7)))
                                .format("%Y-%m-%d")
                                .to_string()
                        );
                        let message = p::render(c["message"].as_str().unwrap(), &args);
                        intents.push(intent(
                            row,
                            "whisper",
                            json!({"steamId":steam,"message":message}),
                            steam.into(),
                            json!({"name":player["name"]}),
                            Some(steam.into()),
                            &format!("{steam}:whisper:{ts}"),
                        ));
                    }
                }
            }
            _ => {}
        }
        trigger_store::enqueue(tx, &m.id, row, &intents, next_state.as_ref(), at).await?;
        if !intents.is_empty() {
            row["last_fired_at"] = json!(at)
        }
        if let Some(state) = next_state {
            row["state"] = state
        }
    }
    m.automation.rows = rows;
    m.automation.risk_attempts.retain(|_, at| ts - *at < 300000);
    Ok(())
}
