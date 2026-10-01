//! Native game observations, raw archives and fenced presence/match bookkeeping.
//! Automation hooks are attached to this transaction by their migrated engines.
use crate::{
    actions,
    config::AppState,
    error::{ApiError, Result},
    game::{self, Client},
    observation_state::{self as state, Player, Session, Tally, TallyRow},
};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use sqlx::{Postgres, Transaction};
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};
#[derive(Clone)]
pub struct Memory {
    pub automation: crate::trigger_engine::RuleMemory,
    pub id: String,
    pub org: String,
    pub name: String,
    pub players_due: i64,
    pub status_due: i64,
    pub players_interval: i64,
    pub failures: u32,
    pub hold: i64,
    pub ok: bool,
    pub error: String,
    pub tier: String,
    pub status: Value,
    pub players: Vec<Player>,
    pub status_at: i64,
    pub players_at: i64,
    pub observed_at: i64,
    pub identity_at: i64,
    pub build: String,
    pub game_id: String,
    pub reserved: Option<i32>,
    pub started_at: i64,
    pub features: Value,
    pub health_unserved: bool,
    pub sessions: Option<HashMap<String, Session>>,
    pub tallies: HashMap<String, Tally>,
    pub last_match: Option<Value>,
    pub heartbeat_at: i64,
    pub sample_at: i64,
    pub sample_key: String,
    pub lists_at: i64,
    pub lists_loaded: bool,
    pub sync_at: i64,
    pub ban_retry: HashMap<String, Instant>,
}
impl Memory {
    pub fn new(id: String, org: String, name: String, now: i64, idle: i64) -> Self {
        let offset = state::phase(&id, idle.min(30000).max(1) as u64) as i64;
        Self {
            automation: Default::default(),
            id,
            org,
            name,
            players_due: now + offset,
            status_due: now + offset,
            players_interval: idle,
            failures: 0,
            hold: 0,
            ok: false,
            error: String::new(),
            tier: "idle".into(),
            status: Value::Null,
            players: vec![],
            status_at: 0,
            players_at: 0,
            observed_at: 0,
            identity_at: 0,
            build: String::new(),
            game_id: String::new(),
            reserved: None,
            started_at: 0,
            features: Value::Null,
            health_unserved: false,
            sessions: None,
            tallies: HashMap::new(),
            last_match: None,
            heartbeat_at: 0,
            sample_at: 0,
            sample_key: String::new(),
            lists_at: 0,
            lists_loaded: false,
            sync_at: 0,
            ban_retry: HashMap::new(),
        }
    }
    pub fn tier(&self, state: &AppState) -> &str {
        if self.failures >= 3 {
            "offline"
        } else if state.runtime.watched(&self.id) {
            "watched"
        } else if !self.players.is_empty() || self.status["playerCount"].as_f64().unwrap_or(0.) > 0.
        {
            "hot"
        } else {
            "idle"
        }
    }
    pub fn cadence(&self, settings: &Map<String, Value>, tier: &str) -> (i64, i64) {
        match tier {
            "watched" => (
                number(settings, "watchedPlayersMs", 1000),
                number(settings, "watchedStatusMs", 2000),
            ),
            "hot" => (
                number(settings, "hotPlayersMs", 2000),
                number(settings, "hotStatusMs", 5000),
            ),
            "offline" => {
                let n = number(settings, "offlineMs", 30000)
                    .saturating_mul(1i64 << self.failures.saturating_sub(3).min(20))
                    .min(number(settings, "offlineMaxMs", 120000));
                (n, n)
            }
            _ => {
                let n = number(settings, "idleMs", 30000);
                (n, n)
            }
        }
    }
}
pub fn number(settings: &Map<String, Value>, key: &str, default: i64) -> i64 {
    settings
        .get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|n| n as i64)))
        .unwrap_or(default)
}
pub fn now() -> i64 {
    Utc::now().timestamp_millis()
}
fn at(ms: i64) -> Option<DateTime<Utc>> {
    (ms > 0)
        .then(|| DateTime::from_timestamp_millis(ms))
        .flatten()
}
pub fn players(raw: &Value) -> game::Result<Vec<Player>> {
    let shaped = actions::players_shape(raw)?;
    let players: Vec<Player> =
        serde_json::from_value(shaped["players"].clone()).map_err(|_| bad_response())?;
    let mut seen = HashSet::new();
    if players.iter().any(|p| {
        crate::api::notes::steam_id(&p.steam_id).is_err()
            || p.name.len() > 4096
            || p.kills < 0
            || p.deaths < 0
            || p.kills > i32::MAX as i64
            || p.deaths > i32::MAX as i64
            || p.cash < i32::MIN as i64
            || p.cash > i32::MAX as i64
            || p.ping.is_some_and(|p| !p.is_finite())
    }) {
        return Err(bad_response());
    }
    Ok(players
        .into_iter()
        .filter(|p| seen.insert(p.steam_id.clone()))
        .collect())
}
fn bad_response() -> game::Error {
    game::Error::Game(game::GameError {
        status: 502,
        code: "bad_response".into(),
        message: "The server returned invalid player or status data.".into(),
        body: Value::Null,
        retry_after_ms: 0,
    })
}
async fn load_sessions(app: &AppState, id: &str) -> Result<HashMap<String, Session>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s) FROM player_sessions s WHERE server_id=$1 AND left_at IS NULL ORDER BY id").bind(id).fetch_all(&app.db).await?;
    let millis = |v: &Value| {
        v.as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.timestamp_millis())
            .unwrap_or(0)
    };
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let steam = r["steam_id"].as_str()?.to_owned();
            let faction = r["faction"].as_str().map(str::to_owned);
            Some((
                steam.clone(),
                Session {
                    id: r["id"].as_i64()?,
                    steam_id: steam,
                    name: r["name"].as_str()?.into(),
                    faction: faction.clone(),
                    kills: r["kills"].as_i64().unwrap_or(0),
                    deaths: r["deaths"].as_i64().unwrap_or(0),
                    cash: r["cash"].as_i64().unwrap_or(0),
                    game: None,
                    seed_ms: r["seed_seconds"].as_i64().unwrap_or(0) * 1000,
                    pending_seed_ms: 0,
                    joined_at: millis(&r["joined_at"]),
                    last_seen: millis(&r["last_seen"]),
                    written_at: millis(&r["last_seen"]),
                    first_visit: false,
                    last_faction: faction.clone(),
                    team: faction,
                },
            ))
        })
        .collect())
}
async fn archive(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    ts: i64,
    raw: &[(String, i64, Value)],
) -> Result<()> {
    for (endpoint, received, payload) in raw {
        sqlx::query("INSERT INTO training_observations(server_id,poll_started_at,received_at,endpoint,payload) VALUES($1,$2,$3,$4,$5)").bind(id).bind(at(ts)).bind(at(*received)).bind(endpoint).bind(payload).execute(&mut **tx).await?;
    }
    Ok(())
}
async fn write_live(tx: &mut Transaction<'_, Postgres>, m: &Memory, ts: i64) -> Result<()> {
    sqlx::query("INSERT INTO server_live(server_id,ok,error,tier,build,game_server_id,started_at,reserved_slots,status,players,player_count,status_at,players_at,observed_at,updated_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) ON CONFLICT(server_id) DO UPDATE SET ok=excluded.ok,error=excluded.error,tier=excluded.tier,build=excluded.build,game_server_id=excluded.game_server_id,started_at=excluded.started_at,reserved_slots=excluded.reserved_slots,status=excluded.status,players=excluded.players,player_count=excluded.player_count,status_at=excluded.status_at,players_at=excluded.players_at,observed_at=excluded.observed_at,updated_at=excluded.updated_at")
 .bind(&m.id).bind(m.ok).bind(&m.error).bind(&m.tier).bind(&m.build).bind(&m.game_id).bind(at(m.started_at)).bind(m.reserved).bind(&m.status).bind(json!(m.players)).bind(m.status["playerCount"].as_f64().unwrap_or(m.players.len() as f64) as i32).bind(at(m.status_at)).bind(at(m.players_at)).bind(at(m.observed_at)).bind(at(ts)).execute(&mut **tx).await?;
    sqlx::query("SELECT pg_notify('warcon_events',$1)")
        .bind(json!({"type":"live","serverId":m.id}).to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
fn session_rows(sessions: &[Session]) -> Value {
    json!(sessions.iter().map(|s|json!({"id":s.id,"last_seen":at(s.last_seen),"name":s.name,"faction":s.team.as_ref().or(s.faction.as_ref()),"kills":s.kills,"deaths":s.deaths,"cash":s.cash,"seed_seconds":((s.seed_ms as f64/1000.)+0.5).floor() as i64})).collect::<Vec<_>>())
}
async fn persist(
    tx: &mut Transaction<'_, Postgres>,
    m: &mut Memory,
    diff: &state::PresenceDiff,
    ts: i64,
    heartbeat: bool,
    teams: &[String],
) -> Result<()> {
    let open = m.sessions.as_mut().expect("presence loaded");
    if !diff.left.is_empty() {
        sqlx::query("UPDATE player_sessions s SET left_at=v.last_seen,last_seen=v.last_seen,name=v.name,faction=v.faction,kills=v.kills,deaths=v.deaths,cash=v.cash,seed_seconds=v.seed_seconds FROM jsonb_to_recordset($1) v(id bigint,last_seen timestamptz,name text,faction text,kills int,deaths int,cash int,seed_seconds int) WHERE s.id=v.id AND s.left_at IS NULL").bind(session_rows(&diff.left)).execute(&mut **tx).await?;
        for s in &diff.left {
            open.remove(&s.steam_id);
        }
    }
    if !diff.joined.is_empty() {
        let payload=json!(diff.joined.iter().map(|p|json!({"steam_id":p.steam_id,"name":p.name,"faction":p.faction,"kills":p.kills,"deaths":p.deaths,"cash":p.cash})).collect::<Vec<_>>());
        let first: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT steam_id FROM player_sessions WHERE server_id=$1 AND steam_id=ANY($2)",
        )
        .bind(&m.id)
        .bind(
            diff.joined
                .iter()
                .map(|p| p.steam_id.clone())
                .collect::<Vec<_>>(),
        )
        .fetch_all(&mut **tx)
        .await?;
        let ids:Vec<(i64,String)>=sqlx::query_as("INSERT INTO player_sessions(server_id,steam_id,name,faction,joined_at,last_seen,kills,deaths,cash) SELECT $1,p.steam_id,p.name,p.faction,$2,$2,p.kills,p.deaths,p.cash FROM jsonb_to_recordset($3) p(steam_id text,name text,faction text,kills int,deaths int,cash int) RETURNING id,steam_id").bind(&m.id).bind(at(ts)).bind(payload).fetch_all(&mut **tx).await?;
        for p in &diff.joined {
            let id = ids
                .iter()
                .find(|(_, steam)| steam == &p.steam_id)
                .map(|r| r.0)
                .ok_or_else(|| ApiError::bad("Missing new session."))?;
            open.insert(
                p.steam_id.clone(),
                state::joined(id, p, ts, Some(teams), !first.contains(&p.steam_id)),
            );
        }
    }
    for p in &diff.stayed {
        if let Some(s) = open.get_mut(&p.steam_id) {
            state::follow(s, p, ts, Some(teams));
        }
    }
    if heartbeat && !diff.stayed.is_empty() {
        let rows = diff
            .stayed
            .iter()
            .filter_map(|p| open.get(&p.steam_id).cloned())
            .collect::<Vec<_>>();
        sqlx::query("UPDATE player_sessions s SET last_seen=v.last_seen,name=v.name,faction=v.faction,kills=v.kills,deaths=v.deaths,cash=v.cash,seed_seconds=v.seed_seconds FROM jsonb_to_recordset($1) v(id bigint,last_seen timestamptz,name text,faction text,kills int,deaths int,cash int,seed_seconds int) WHERE s.id=v.id AND s.left_at IS NULL").bind(session_rows(&rows)).execute(&mut **tx).await?;
        for p in &diff.stayed {
            if let Some(s) = open.get_mut(&p.steam_id) {
                s.written_at = ts;
            }
        }
        m.heartbeat_at = ts;
    }
    Ok(())
}
pub async fn write_tallies(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    match_id: i64,
    rows: &[TallyRow],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let payload=json!(rows.iter().map(|r|json!({"steam_id":r.steam_id,"name":r.name,"faction":r.faction,"seconds":r.seconds,"kills":r.kills,"deaths":r.deaths,"cash_delta":r.cash_delta})).collect::<Vec<_>>());
    sqlx::query("INSERT INTO match_players(match_id,server_id,steam_id,name,faction,seconds,kills,deaths,cash_delta) SELECT $1,$2,p.steam_id,p.name,p.faction,p.seconds,p.kills,p.deaths,p.cash_delta FROM jsonb_to_recordset($3) p(steam_id text,name text,faction text,seconds int,kills int,deaths int,cash_delta int) ON CONFLICT(match_id,steam_id) DO UPDATE SET name=excluded.name,faction=excluded.faction,seconds=excluded.seconds,kills=excluded.kills,deaths=excluded.deaths,cash_delta=excluded.cash_delta").bind(match_id).bind(id).bind(payload).execute(&mut **tx).await?;
    Ok(())
}
async fn reconcile_match(
    tx: &mut Transaction<'_, Postgres>,
    m: &mut Memory,
    ts: i64,
    look: &Value,
    end: Option<&Value>,
    previous: i64,
) -> Result<i64> {
    let current:Option<(i64,Option<String>,i32)>=sqlx::query_as("SELECT id,map,peak_players FROM matches WHERE server_id=$1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1 FOR UPDATE").bind(&m.id).fetch_optional(&mut **tx).await?;
    let stale = end.is_none()
        && current
            .as_ref()
            .is_some_and(|r| r.1.as_deref() != look["map"].as_str());
    if let Some(end) = end {
        let (rows, carried) = state::close(&m.tallies, previous);
        if let Some((id, _, _)) = &current {
            write_tallies(tx, &m.id, *id, &rows).await?;
            crate::match_feed::enrich(tx, &m.id, *id).await?;
            sqlx::query("UPDATE matches SET ended_at=$2,final_scores=$3,winner=$4 WHERE id=$1")
                .bind(id)
                .bind(at(ts))
                .bind(&end["scores"])
                .bind(end["winner"].as_str())
                .execute(&mut **tx)
                .await?;
        }
        m.tallies = carried;
    } else if let Some((id, _, _)) = &current {
        if stale {
            sqlx::query("UPDATE matches SET ended_at=$2 WHERE id=$1")
                .bind(id)
                .bind(at(ts))
                .execute(&mut **tx)
                .await?;
        } else {
            let rows = m
                .tallies
                .values()
                .filter(|t| t.dirty)
                .map(state::row)
                .collect::<Vec<_>>();
            write_tallies(tx, &m.id, *id, &rows).await?;
            for tally in m.tallies.values_mut() {
                tally.dirty = false;
            }
        }
    }
    if current.is_none() || end.is_some() || stale {
        let elapsed = look["matchSeconds"]
            .as_f64()
            .filter(|n| *n >= 0. && *n <= 86400. * 365.)
            .unwrap_or(0.);
        let started = ts - (elapsed * 1000.) as i64;
        Ok(sqlx::query_scalar("INSERT INTO matches(server_id,started_at,map,experiences,lighting,peak_players) VALUES($1,$2,$3,$4,$5,$6) RETURNING id").bind(&m.id).bind(at(started)).bind(look["map"].as_str()).bind(experiences(&m.status)).bind(m.status["lighting"].as_str()).bind(m.status["playerCount"].as_f64().unwrap_or(0.) as i32).fetch_one(&mut **tx).await?)
    } else {
        let id = current.unwrap().0;
        sqlx::query("UPDATE matches SET peak_players=GREATEST(peak_players,$2) WHERE id=$1")
            .bind(id)
            .bind(m.status["playerCount"].as_f64().unwrap_or(0.) as i32)
            .execute(&mut **tx)
            .await?;
        Ok(id)
    }
}
fn experiences(status: &Value) -> String {
    status["experiences"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("+")
}
fn scores(status: &Value) -> Value {
    json!(
        status["scores"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| json!({"name":s["name"],"score":s["score"]}))
            .collect::<Vec<_>>()
    )
}
async fn record_profiles(tx: &mut Transaction<'_, Postgres>, m: &Memory, ts: i64) -> Result<()> {
    let payload = json!(
        m.players
            .iter()
            .filter(|p| !p.name.trim().is_empty())
            .map(|p| json!({"steam_id":p.steam_id,"name":crate::feed::truncate(&p.name,200)}))
            .collect::<Vec<_>>()
    );
    sqlx::query("INSERT INTO integrity_profiles(org_id,steam_id,current_name,aliases,first_seen,last_seen) SELECT $1,p.steam_id,p.name,jsonb_build_array(p.name),$2,$2 FROM jsonb_to_recordset($3) p(steam_id text,name text) ON CONFLICT(org_id,steam_id) DO UPDATE SET current_name=CASE WHEN excluded.last_seen>=integrity_profiles.last_seen THEN excluded.current_name ELSE integrity_profiles.current_name END,aliases=CASE WHEN integrity_profiles.aliases ? excluded.current_name THEN integrity_profiles.aliases ELSE integrity_profiles.aliases||jsonb_build_array(excluded.current_name) END,first_seen=LEAST(integrity_profiles.first_seen,excluded.first_seen),last_seen=GREATEST(integrity_profiles.last_seen,excluded.last_seen)").bind(&m.org).bind(at(ts)).bind(payload).execute(&mut **tx).await?;
    Ok(())
}
fn hold(m: &mut Memory, e: &game::Error) -> bool {
    if let game::Error::Game(e) = e {
        if e.status == 429 {
            m.hold = m.hold.max(now() + e.retry_after_ms.max(1000) as i64);
            return true;
        }
    }
    false
}
async fn identity(client: &Client, m: &mut Memory, ts: i64) {
    m.identity_at = ts;
    m.health_unserved = false;
    match actions::run(client, "capabilities", &json!({})).await {
        Ok(caps) => {
            m.features = caps["features"].clone();
            m.build = crate::http::string(&caps["raw"]["build"], 200);
            if m.features["serverId"] != true {
                m.game_id.clear()
            }
        }
        Err(e) => {
            m.identity_at = ts - 3600000 + 300000;
            if hold(m, &e) {
                return;
            }
        }
    }
    if m.features["serverId"] == true {
        match actions::run(client, "serverId", &json!({})).await {
            Ok(v) => m.game_id = crate::http::string(&v["serverId"], 100),
            Err(e) => {
                m.identity_at = ts - 3600000 + 300000;
                hold(m, &e);
            }
        }
    }
    if m.hold > now() {
        return;
    }
    match actions::read_config(client).await {
        Ok(v) => {
            m.reserved = reserved_slots(v["text"].as_str().unwrap_or(""));
        }
        Err(e) => {
            m.identity_at = ts - 3600000 + 300000;
            hold(m, &e);
        }
    }
}
fn reserved_slots(text: &str) -> Option<i32> {
    crate::config_document::array(text, crate::config_document::SESSION, "MaxReservedSlots")
        .last()
        .and_then(|s| crate::http::js_number(&json!(s.trim())))
        .filter(|n| *n >= 0. && *n <= i32::MAX as f64 && n.fract() == 0.)
        .map(|n| n as i32)
}
async fn uptime(client: &Client, m: &mut Memory) {
    if m.health_unserved || m.hold > now() {
        return;
    }
    let ts = now();
    match actions::run(client, "health", &json!({})).await {
        Ok(v) => {
            if let Some(up) = crate::http::js_number(&v["uptimeSeconds"])
                .filter(|n| *n >= 0. && *n <= 86400. * 365. * 10.)
            {
                let started = ts - up.floor() as i64 * 1000;
                if (started - m.started_at).abs() > 5000 {
                    m.started_at = started
                }
            }
        }
        Err(game::Error::Game(e)) if e.code == "no_route" => m.health_unserved = true,
        Err(e) => {
            hold(m, &e);
        }
    }
}
pub async fn observe(
    app: &AppState,
    m: &mut Memory,
    status_due: bool,
    players_due: bool,
    settings: &Map<String, Value>,
) -> Result<()> {
    app.runtime.check().await?;
    if m.observed_at == 0 {
        let previous:Option<(String,String,Option<i32>,Option<DateTime<Utc>>)>=sqlx::query_as("SELECT build,game_server_id,reserved_slots,started_at FROM server_live WHERE server_id=$1").bind(&m.id).fetch_optional(&app.db).await?;
        if let Some((build, id, reserved, started)) = previous {
            if m.build.is_empty() {
                m.build = build
            }
            if m.game_id.is_empty() {
                m.game_id = id
            }
            if m.reserved.is_none() {
                m.reserved = reserved
            }
            if m.started_at == 0 {
                m.started_at = started.map(|t| t.timestamp_millis()).unwrap_or(0)
            }
        }
    }
    let ts = now();
    let started = Instant::now();
    let mut raw = vec![];
    let read = async {
        let client = Client::for_server(app, &m.id).await?;
        let status = if status_due || m.status.is_null() {
            let reply = client.json("GET", "/v1/status", None).await?;
            raw.push(("/v1/status".into(), now(), reply.clone()));
            let s = crate::live::status(&actions::status_shape(reply, false));
            if s.is_null() {
                return Err(bad_response());
            }
            Some(s)
        } else {
            None
        };
        let players = if players_due {
            let reply = client.json("GET", "/v1/players", None).await?;
            raw.push(("/v1/players".into(), now(), reply.clone()));
            Some(players(&reply)?)
        } else {
            None
        };
        Ok::<_, game::Error>((client, status, players))
    }
    .await;
    let (client, status, players) = match read {
        Ok(v) => v,
        Err(game::Error::Api(e)) => return Err(e),
        Err(e) => {
            return failed(
                app,
                m,
                ts,
                started.elapsed().as_millis() as i32,
                &e,
                &raw,
                settings,
            )
            .await;
        }
    };
    let previous_players = m.players_at;
    let previous_status = m.status_at;
    let was_offline = m.failures >= 3;
    let had_failed = m.failures > 0;
    let mut next = m.clone();
    next.failures = 0;
    next.hold = 0;
    next.ok = true;
    next.error.clear();
    next.observed_at = ts;
    if let Some(status) = &status {
        next.status = status.clone();
        next.status_at = ts
    }
    if let Some(players) = &players {
        next.players = players.clone();
        next.players_at = ts
    }
    next.tier = next.tier(app).into();
    if had_failed || ts - next.identity_at >= 3600000 {
        identity(&client, &mut next, ts).await;
    }
    if status.is_some() {
        uptime(&client, &mut next).await;
    }
    if next.sessions.is_none() {
        next.sessions = Some(load_sessions(app, &m.id).await?);
    }
    let teams = next.status["scores"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let gap = ts - previous_players;
    let trusted = players.is_some()
        && previous_players > 0
        && !was_offline
        && gap <= 2 * next.players_interval.max(1000) + 1000;
    let diff = if let Some(players) = &players {
        state::diff(
            next.sessions.as_ref().unwrap(),
            players,
            ts,
            60000,
            previous_players,
            Some(&teams),
        )
    } else {
        state::PresenceDiff::default()
    };
    let look = status
        .as_ref()
        .map(|s| json!({"map":s["map"],"scores":scores(s),"matchSeconds":s["matchSeconds"]}));
    let end = if was_offline {
        None
    } else {
        look.as_ref()
            .and_then(|look| state::boundary(next.last_match.as_ref(), look))
    };
    if let Some(look) = &look {
        next.last_match = Some(look.clone());
    }
    if let Some(players) = &players {
        state::tally(
            &mut next.tallies,
            players,
            ts,
            gap,
            &if trusted {
                diff.stayed.iter().map(|p| p.steam_id.clone()).collect()
            } else {
                HashSet::new()
            },
            Some(&teams),
        );
    }
    for s in &diff.left {
        if let Some(t) = next.tallies.get_mut(&s.steam_id) {
            t.dirty = true
        }
    }
    crate::trigger_engine::prepare(app, &mut next, &diff, trusted, players.is_some(), ts).await?;
    crate::trigger_engine::accrue(&mut next, &diff, trusted, players.is_some(), gap);
    let mut tx = app.worker_transaction().await?;
    archive(&mut tx, &next.id, ts, &raw).await?;
    if players.is_some() {
        let heartbeat = ts - next.heartbeat_at >= number(settings, "sessionHeartbeatMs", 30000);
        persist(&mut tx, &mut next, &diff, ts, heartbeat, &teams).await?;
        record_profiles(&mut tx, &next, ts).await?;
        crate::qq_economy::warmth(
            app,
            &mut tx,
            &next.id,
            at(ts)
                .ok_or_else(|| crate::error::ApiError::bad("Observation timestamp is invalid."))?,
            &diff
                .stayed
                .iter()
                .map(|p| p.steam_id.clone())
                .collect::<Vec<_>>(),
            gap,
            next.players.len(),
            trusted,
        )
        .await?;
        crate::steam::enqueue(
            &mut tx,
            &next
                .players
                .iter()
                .map(|p| p.steam_id.clone())
                .collect::<Vec<_>>(),
        )
        .await?;
    }
    write_live(&mut tx, &next, ts).await?;
    let sample_key = json!([
        next.status["map"],
        next.status["experiences"],
        next.status["lighting"],
        next.status["playerCount"],
        next.status["maxPlayers"]
    ])
    .to_string();
    if !next.status.is_null()
        && (sample_key != next.sample_key
            || ts - next.sample_at >= number(settings, "sampleMs", 30000))
    {
        let mut cash = teams
            .iter()
            .map(|name| (name.clone(), 0i64))
            .collect::<Vec<_>>();
        for p in &next.players {
            let faction = p.faction.as_deref().unwrap_or("");
            if let Some((_, n)) = cash.iter_mut().find(|r| r.0 == faction) {
                *n += p.cash
            } else {
                cash.push((faction.into(), p.cash));
            }
        }
        sqlx::query("INSERT INTO samples(ts,server_id,ok,player_count,max_players,map,experiences,lighting,match_seconds,scores,cash,latency_ms) VALUES($1,$2,true,$3,$4,$5,$6,$7,$8,$9,$10,$11)").bind(at(ts)).bind(&next.id).bind(next.status["playerCount"].as_f64().unwrap_or(0.) as i32).bind(next.status["maxPlayers"].as_f64().unwrap_or(0.) as i32).bind(next.status["map"].as_str()).bind(experiences(&next.status)).bind(next.status["lighting"].as_str()).bind(next.status["matchSeconds"].as_f64().map(|n|n as i32)).bind(scores(&next.status)).bind(json!(cash.iter().map(|(name,cash)|json!({"name":name,"cash":cash})).collect::<Vec<_>>())).bind(started.elapsed().as_millis().min(i32::MAX as u128) as i32).execute(&mut *tx).await?;
        next.sample_key = sample_key;
        next.sample_at = ts;
    }
    crate::trigger_engine::evaluate(
        &mut tx,
        &mut next,
        &diff,
        trusted,
        players.is_some(),
        end.as_ref(),
        previous_status,
        ts,
    )
    .await?;
    let match_id = if let Some(look) = &look {
        Some(reconcile_match(&mut tx, &mut next, ts, look, end.as_ref(), previous_status).await?)
    } else {
        sqlx::query_scalar::<_,i64>("SELECT id FROM matches WHERE server_id=$1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1").bind(&next.id).fetch_optional(&mut *tx).await?
    };
    if let Some(match_id) = match_id
        .filter(|_| players.is_some() && !next.players.is_empty() && ts - next.status_at < 30000)
    {
        sqlx::query("INSERT INTO player_progress_samples(server_id,match_id,bucket,observed_at,players) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING").bind(&next.id).bind(match_id).bind(ts/30000).bind(at(ts)).bind(json!(next.players.iter().map(|p|json!({"steamId":p.steam_id,"cash":p.cash,"kills":p.kills,"deaths":p.deaths})).collect::<Vec<_>>())).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    *m = next;
    if ts - m.lists_at >= number(settings, "listsSnapshotMs", 300000) {
        // An unsupported list endpoint must not undo healthy presence/kill observations.
        m.lists_at = ts;
        if let Err(e) = crate::list_sync::snapshot_held(app, &client, &m.id).await {
            if e.code == "worker_ownership_lost" {
                return Err(e);
            }
            tracing::warn!(server_id=%m.id, "Game list snapshot unavailable");
        } else {
            m.lists_loaded = true;
        }
    }
    if players.is_some() {
        // Game effects occur after observation commit, still within the held lane.
        for (name, result) in [
            (
                "faction_quota",
                crate::faction_quota::run(app, &client, m, trusted, end.is_some()).await,
            ),
            (
                "skill_balance",
                crate::skill_balance::select(app, m, trusted, end.is_some()).await,
            ),
            (
                "faction_lock",
                crate::faction_lock::run(app, &client, m, &diff, trusted, end.is_some()).await,
            ),
            (
                "numeric",
                crate::game_automation::numeric(app, &client, m, trusted, end.is_some()).await,
            ),
            (
                "weapons",
                crate::game_automation::weapons(app, &client, m, end.is_some()).await,
            ),
        ] {
            if let Err(error) = result {
                app.runtime.check().await?;
                tracing::warn!(server=%m.id,%name,%error,"automation failed");
            }
        }
        crate::ban_enforcement::enforce(
            app,
            &client,
            &m.id,
            &m.players
                .iter()
                .map(|p| p.steam_id.clone())
                .collect::<Vec<_>>(),
            &mut m.ban_retry,
        )
        .await?;
    }
    Ok(())
}
async fn failed(
    app: &AppState,
    m: &mut Memory,
    ts: i64,
    latency: i32,
    e: &game::Error,
    raw: &[(String, i64, Value)],
    settings: &Map<String, Value>,
) -> Result<()> {
    let mut next = m.clone();
    let limited = hold(&mut next, e);
    next.observed_at = ts;
    next.error = match e {
        game::Error::Api(e) => crate::feed::truncate(&e.message, 300),
        game::Error::Game(e) => crate::feed::truncate(&e.message, 300),
    };
    if !limited {
        next.failures += 1;
        next.ok = false;
        next.tier = next.tier(app).into();
    }
    let mut tx = app.worker_transaction().await?;
    archive(&mut tx, &m.id, ts, raw).await?;
    if !limited && next.failures >= 3 {
        if next.sessions.is_none() {
            next.sessions = Some(load_sessions(app, &m.id).await?)
        }
        let diff = state::PresenceDiff {
            left: next.sessions.as_ref().unwrap().values().cloned().collect(),
            ..Default::default()
        };
        persist(&mut tx, &mut next, &diff, ts, false, &[]).await?;
        let open:Option<i64>=sqlx::query_scalar("SELECT id FROM matches WHERE server_id=$1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1").bind(&m.id).fetch_optional(&mut *tx).await?;
        if let Some(match_id) = open {
            write_tallies(
                &mut tx,
                &m.id,
                match_id,
                &next.tallies.values().map(state::row).collect::<Vec<_>>(),
            )
            .await?;
        }
        next.tallies.clear();
        next.last_match = None;
    }
    if !limited
        && (next.failures == 1 || ts - next.sample_at >= number(settings, "sampleMs", 30000))
    {
        sqlx::query(
            "INSERT INTO samples(ts,server_id,ok,latency_ms,error) VALUES($1,$2,false,$3,$4)",
        )
        .bind(at(ts))
        .bind(&m.id)
        .bind(latency)
        .bind(&next.error)
        .execute(&mut *tx)
        .await?;
        next.sample_at = ts;
        next.sample_key = "failed".into();
    }
    write_live(&mut tx, &next, ts).await?;
    tx.commit().await?;
    *m = next;
    Ok(())
}
