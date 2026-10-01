//! The original, explicitly selected `host=demo` WDRCON simulator, entirely native.
//! Fake state is process-local and isolated by server; real targets never enter this module.
use crate::{
    config::AppState, config_document as ini, error::Result, rcon::GameResponse, runtime::Runtime,
};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::{collections::HashMap, sync::OnceLock};

fn catalog() -> &'static Value {
    static SOURCE: OnceLock<Value> = OnceLock::new();
    SOURCE.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/demo.json")).expect("demo source fixture")
    })
}
pub fn enabled() -> bool {
    std::env::var("ALLOW_DEMO_SERVER").map_or(true, |v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
pub fn is_demo(host: &str) -> bool {
    enabled() && host.trim().eq_ignore_ascii_case("demo")
}
fn live_build() -> bool {
    std::env::var("MOCK_LIVE_BUILD")
        .is_ok_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}
fn utc(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .format("%Y-%m-%d %H:%M:%SZ")
        .to_string()
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Player {
    name: String,
    steam_id: String,
    faction: String,
    kills: i64,
    deaths: i64,
    cash: i64,
    ping_ms: i64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    map: String,
    experiences: Vec<String>,
    lighting: String,
    zone_alternator: String,
    denied: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rotation {
    enabled: bool,
    mode: String,
    now_index: usize,
    next_index: i64,
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoState {
    server_name: String,
    max_players: i64,
    score_tick: i64,
    score_cap: i64,
    factions: Vec<Value>,
    current: Value,
    players: Vec<Player>,
    bans: Vec<Value>,
    reserved: Vec<String>,
    sponsor_url: String,
    rotation: Rotation,
    audit: Vec<Value>,
    match_start: i64,
    last_score_at: i64,
    next_joiner: u64,
    config_text: String,
    config_revision: u64,
    feed: Vec<Value>,
    #[serde(skip)]
    requests: u64,
    #[serde(skip)]
    started: i64,
}
impl DemoState {
    fn new(key: &str, now: i64) -> Self {
        let mut state: Self = serde_json::from_value(catalog()["seed"].clone()).expect("demo seed");
        let delta = now - catalog()["fixed"].as_i64().unwrap();
        state.started = now;
        state.match_start += delta;
        state.last_score_at = now;
        for entry in &mut state.audit {
            if let Some(ms) = entry["timestampUtc"]
                .as_str()
                .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%SZ").ok())
                .map(|d| d.and_utc().timestamp_millis())
            {
                entry["timestampUtc"] = json!(utc(ms + delta));
            }
        }
        state.server_name = format!(
            "Warcon Demo Server [{}]",
            key.chars().take(6).collect::<String>()
        );
        state.config_text = state.config_text.replacen(
            "ServerName=fixture",
            &format!("ServerName={}", state.server_name),
            1,
        );
        state
    }
    fn log(&mut self, detail: String, now: i64) {
        self.audit.push(json!({"timestampUtc":utc(now),"peer":"warcon","sessionId":"warcon01","event":"COMMAND","detail":detail}));
        if self.audit.len() > 500 {
            self.audit.drain(..self.audit.len() - 500);
        }
    }
    fn reset(&mut self, now: i64) {
        for f in &mut self.factions {
            f["score"] = json!(0);
        }
        self.match_start = now;
        for p in &mut self.players {
            p.kills = 0;
            p.deaths = 0;
            p.cash = 0;
        }
    }
    fn apply(&mut self, e: &Entry) {
        self.current = json!({"map":e.map,"experiences":e.experiences,"lighting":e.lighting,"alternator":e.zone_alternator});
    }
    fn next_playable(&self, from: usize) -> usize {
        let n = self.rotation.entries.len();
        (1..=n)
            .map(|step| (from + step) % n)
            .find(|i| !self.rotation.entries[*i].denied)
            .unwrap_or(from)
    }
    fn end(&mut self, now: i64) {
        if self.rotation.enabled && !self.rotation.entries.is_empty() {
            let next = if self.rotation.next_index >= 0 {
                self.rotation.next_index as usize
            } else {
                self.next_playable(self.rotation.now_index)
            };
            if let Some(entry) = self.rotation.entries.get(next).cloned() {
                self.rotation.now_index = next;
                self.rotation.next_index = self.next_playable(next) as i64;
                self.apply(&entry);
            }
        }
        self.reset(now);
    }
    fn queue_kill(&mut self, killer: &Player, victim: &Player, now: i64, rng: &mut impl Rng) {
        let suicide = killer.steam_id == victim.steam_id;
        let causes = catalog()["FEED_CAUSES"].as_array().unwrap();
        let mut tags = vec![
            "Meta.PlayerKillFlag.Player.Local.Kill",
            "Meta.PlayerKillFlag.Player.Local.Death",
        ];
        if suicide {
            tags.insert(0, "Meta.PlayerKillFlag.Player.Suicide");
        } else if rng.gen_bool(0.25) {
            tags.insert(0, "Meta.Progression.Context.Player.KillContext.Headshot");
        }
        let mut event = json!({"eventId":uuid::Uuid::new_v4().to_string().to_ascii_uppercase(),"type":"killed","eventTime":(now-self.match_start) as f64/1000.,"matchId":"demo-match","mapName":self.current["map"],"killerName":killer.name,"killerId":"-demo","killerSteamId":killer.steam_id,"victimName":victim.name,"victimId":"-demo","victimSteamId":victim.steam_id,"contextTags":tags});
        if !suicide {
            event["cause"] = causes[rng.gen_range(0..causes.len())].clone();
            event["distance"] = json!(rng.gen_range(300..=30300));
        }
        self.feed.push(event);
        if self.feed.len() > 50 {
            self.feed.drain(..self.feed.len() - 50);
        }
    }
    fn tick(&mut self, now: i64) {
        let mut rng = rand::thread_rng();
        let dt = (now - self.last_score_at).max(0) as f64 / 1000.;
        self.last_score_at = now;
        for f in &mut self.factions {
            f["score"] = json!(
                f["score"].as_f64().unwrap_or(0.)
                    + f["rate"].as_f64().unwrap() * dt * rng.gen_range(0.8..1.2)
            );
        }
        if self
            .factions
            .iter()
            .any(|f| f["score"].as_f64().unwrap_or(0.) >= self.score_cap as f64)
        {
            self.end(now);
        }
        for i in 0..self.players.len() {
            if rng.gen_bool(0.05) {
                self.players[i].kills += 1;
                self.players[i].cash += 150;
                let v = rng.gen_range(0..self.players.len());
                if v != i {
                    self.players[v].deaths += 1;
                }
                self.queue_kill(
                    &self.players[i].clone(),
                    &self.players[v].clone(),
                    now,
                    &mut rng,
                );
            }
            self.players[i].ping_ms = (self.players[i].ping_ms + rng.gen_range(-4..=4)).max(5);
        }
        if self.players.len() > 8 && rng.gen_bool(0.03) {
            let drifters = self
                .players
                .iter()
                .enumerate()
                .filter(|(_, p)| p.steam_id.as_str() >= "76561198100000401")
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if !drifters.is_empty() {
                let index = drifters[rng.gen_range(0..drifters.len())];
                let gone = self.players.remove(index);
                self.log(format!("{} disconnected", gone.name), now);
                self.audit.last_mut().unwrap()["event"] = json!("CLOSE");
            }
        }
        if self.players.len() < 16 && rng.gen_bool(if self.players.len() < 12 { 0.2 } else { 0.04 })
        {
            let joiners = catalog()["JOINERS"].as_array().unwrap();
            self.players.push(Player {
                name: joiners[self.next_joiner as usize % joiners.len()]
                    .as_str()
                    .unwrap()
                    .into(),
                steam_id: (76561198100000401u64 + self.next_joiner).to_string(),
                faction: self.factions[rng.gen_range(0..3)]["name"]
                    .as_str()
                    .unwrap()
                    .into(),
                kills: 0,
                deaths: 0,
                cash: 0,
                ping_ms: rng.gen_range(20..110),
            });
            self.next_joiner += 1;
        }
    }
    fn revision(&self) -> String {
        format!("demo{:08}", self.config_revision)
    }
    fn seed_config(&mut self) {
        self.config_text = ini::set_scalar(
            &self.config_text,
            ini::SESSION,
            "ServerName",
            &self.server_name,
        );
        self.config_text = ini::set_array(
            &self.config_text,
            ini::SESSION,
            ini::RESERVED,
            &self.reserved,
        );
        self.config_text = ini::set_scalar(
            &self.config_text,
            "MatchState.Playing.KOTH",
            "ScorePeriod",
            &self.score_tick.to_string(),
        );
        self.config_text = ini::set_scalar(
            &self.config_text,
            ROTATION,
            "bEnabled",
            if self.rotation.enabled {
                "true"
            } else {
                "false"
            },
        );
        self.config_text = ini::set_scalar(
            &self.config_text,
            ROTATION,
            "RotationMode",
            if self.rotation.mode == "random" {
                "Random"
            } else {
                "Ordered"
            },
        );
        let entries = self
            .rotation
            .entries
            .iter()
            .map(|e| {
                format!(
                    "(Map=\"{}\",Experiences=\"{}\",Lighting=\"{}\"{})",
                    e.map,
                    e.experiences.join("+"),
                    e.lighting,
                    if e.zone_alternator.is_empty() {
                        String::new()
                    } else {
                        format!(",ZoneAlternator=\"{}\"", e.zone_alternator)
                    }
                )
            })
            .collect::<Vec<_>>();
        self.config_text = ini::set_array(&self.config_text, ROTATION, "RotationEntries", &entries);
    }
}
const ROTATION: &str = "/Script/WDGame.WDServerMapRotationSettings";
fn sections(live: bool) -> Value {
    let mut value = catalog()["CONFIG_SECTIONS"].clone();
    if live {
        for c in value.as_array_mut().unwrap() {
            let pin = match c["section"].as_str().unwrap_or("") {
                ini::SESSION => Some(("ServerName", "RCON_FixedServerName", "applied")),
                "/Script/WDRCON.WDRCONSettings" => Some(("Port", "RCONPort", "next-restart")),
                _ => None,
            };
            if let Some((key, locked, when)) = pin {
                c["keyOverrides"].as_array_mut().unwrap().push(json!({"key":key,"appliesWhen":when,"description":format!("Pinned by -{locked} on this server's command line. The value is shown but cannot be changed here."),"writable":false,"lockedBy":locked}));
            }
        }
    }
    value
}
fn ok(body: Value, status: u16) -> GameResponse {
    GameResponse {
        status,
        status_text: if status == 200 {
            "OK".into()
        } else {
            String::new()
        },
        headers: HashMap::from([
            ("content-type".into(), "application/json".into()),
            (
                "access-control-expose-headers".into(),
                "ETag, Retry-After".into(),
            ),
        ]),
        text: body.to_string(),
    }
}
fn fail(status: u16, message: impl Into<String>, code: &str) -> GameResponse {
    ok(
        json!({"ok":false,"error":{"code":code,"message":message.into()}}),
        status,
    )
}
fn string(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn valid_id(id: &str) -> bool {
    id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit())
}
fn valid_map(id: &str) -> bool {
    catalog()["MAPS"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == id)
}
fn entry(b: &Value, default_lighting: &str, default_exp: &[String]) -> Entry {
    Entry {
        map: string(&b["map"]).into(),
        experiences: b["experiences"]
            .as_array()
            .filter(|a| !a.is_empty())
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| default_exp.to_vec()),
        lighting: b["lighting"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(default_lighting)
            .into(),
        zone_alternator: string(&b["zoneAlternator"]).into(),
        denied: false,
    }
}
fn read_rotation(text: &str) -> Vec<Entry> {
    let re = regex::Regex::new(r#"(\w+)\s*=\s*(?:"([^"]*)"|([^,)]*))"#).unwrap();
    ini::array(text, ROTATION, "RotationEntries")
        .into_iter()
        .filter_map(|line| {
            let fields = re
                .captures_iter(line.trim().trim_start_matches('(').trim_end_matches(')'))
                .map(|c| {
                    (
                        c[1].to_ascii_lowercase(),
                        c.get(2)
                            .or_else(|| c.get(3))
                            .unwrap()
                            .as_str()
                            .trim()
                            .to_owned(),
                    )
                })
                .collect::<HashMap<_, _>>();
            let map = fields.get("map")?.clone();
            if map.is_empty() {
                return None;
            }
            let experiences = fields
                .get("experiences")
                .or_else(|| fields.get("experience"))
                .map(String::as_str)
                .unwrap_or("")
                .split('+')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let denied = experiences.iter().any(|id| {
                !catalog()["EXPERIENCES"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["id"] == *id)
            });
            Some(Entry {
                map,
                experiences,
                lighting: fields.get("lighting").cloned().unwrap_or_default(),
                zone_alternator: fields.get("zonealternator").cloned().unwrap_or_default(),
                denied,
            })
        })
        .collect()
}
fn handle(
    s: &mut DemoState,
    key: &str,
    method: &str,
    raw_path: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    live: bool,
    now: i64,
) -> GameResponse {
    let (path, query) = raw_path.split_once('?').unwrap_or((raw_path, ""));
    let qs = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect::<HashMap<_, _>>();
    let p = path
        .trim_matches('/')
        .split('/')
        .skip(1)
        .collect::<Vec<_>>();
    let b = body
        .and_then(|b| serde_json::from_str::<Value>(b).ok())
        .unwrap_or_else(|| json!({}));
    let route = format!("{method} {}", p.join("/"));
    if live {
        let pattern = format!(
            "{method} /v1/{}",
            p.iter()
                .enumerate()
                .map(|(i, s)| if i > 0 && valid_id(s) {
                    "{steamId}"
                } else if i > 0 && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
                    "{index}"
                } else {
                    s
                })
                .collect::<Vec<_>>()
                .join("/")
        );
        if catalog()["LIVE_BUILD_MISSING"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == &pattern)
        {
            return fail(404, "No such endpoint.", "not_found");
        }
    }
    match route.as_str() {
        "GET capabilities" => {
            return ok(
                json!({"apiVersion":"1","build":if live{"++Wardogs+Live-CL-501228"}else{"++Wardogs+Demo-CL-501228"},"auth":{"scheme":"bearer","header":"Authorization"},"limits":{"maxBodyBytes":65536,"maxRequestsPerMinutePerIp":600},"config":{"writable":true,"document":"/v1/config"},"routes":catalog()["CAPABILITY_ROUTES"].as_array().unwrap().iter().filter(|r|!live||!catalog()["LIVE_BUILD_MISSING"].as_array().unwrap().contains(r)).collect::<Vec<_>>()}),
                200,
            );
        }
        "GET health" => {
            return ok(
                json!({"status":"ok","uptimeSeconds":(now-s.started)/1000,"connections":{"active":1},"gameThreadQueue":{"inFlight":0,"depth":32,"rejectedTotal":0}}),
                200,
            );
        }
        "GET server-id" => {
            let h = hex::encode(Sha1::digest(format!("warcon-demo:{key}")));
            return ok(
                json!({"serverId":format!("{}-{}-4{}-a{}-{}",&h[..8],&h[8..12],&h[13..16],&h[17..20],&h[20..32])}),
                200,
            );
        }
        "GET status" => {
            let held = ini::scalar(&s.config_text, ini::SESSION, "MaxReservedSlots")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let mut status = json!({"serverName":s.server_name,"map":s.current["map"],"experiences":s.current["experiences"],"lighting":s.current["lighting"],"alternator":s.current["alternator"],"scoreTick":{"current":s.score_tick,"min":18,"max":30},"players":{"current":s.players.len(),"max":(s.max_players-held).max(0)},"factionScores":s.factions.iter().map(|f|json!({"name":f["name"],"colorHex":f["colorHex"],"score":f["score"].as_f64().unwrap_or(0.).floor()})).collect::<Vec<_>>(),"rotation":{"nowIndex":s.rotation.now_index,"nextIndex":if s.rotation.enabled {json!(s.rotation.next_index)}else{Value::Null}}});
            if !live {
                status["scoreCap"] = json!(s.score_cap);
                status["matchSeconds"] = json!((now - s.match_start) / 1000);
            }
            return ok(status, 200);
        }
        "GET players" => return ok(json!({"players":s.players}), 200),
        "GET bans" => return ok(json!({"bans":s.bans}), 200),
        "GET reserved-slots" => return ok(json!({"reservedSlots":s.reserved}), 200),
        "GET sponsor" => return ok(json!({"imageUrl":s.sponsor_url}), 200),
        "PUT sponsor" => {
            return fail(
                405,
                "PUT is not supported on this endpoint",
                "method_not_allowed",
            );
        }
        "GET catalog/maps" => return ok(json!({"maps":catalog()["MAPS"]}), 200),
        "GET catalog/lightings" => return ok(json!({"lightings":catalog()["LIGHTINGS"]}), 200),
        "GET catalog/experiences" => {
            return ok(json!({"experiences":catalog()["EXPERIENCES"]}), 200);
        }
        "GET audit" => {
            let limit = qs
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(50)
                .clamp(1, 500);
            return ok(
                json!({"entries":&s.audit[s.audit.len().saturating_sub(limit)..]}),
                200,
            );
        }
        "POST broadcast" => {
            if string(&b["message"]).is_empty() {
                return fail(400, "message is required.", "error");
            }
            s.log(format!("broadcast {}", b["message"].as_str().unwrap()), now);
            return ok(
                json!({"message":format!("Announcement sent to {} player(s).",s.players.len())}),
                200,
            );
        }
        "POST bans" => {
            let id = string(&b["steamId"]);
            if !valid_id(id) {
                return fail(400, "steamId must be a 17-digit SteamID64.", "error");
            }
            let player = s.players.iter().find(|p| p.steam_id == id).cloned();
            if live && player.is_none() {
                return fail(404, format!("Error: no player matching '{id}'."), "error");
            }
            s.players.retain(|p| p.steam_id != id);
            s.bans.retain(|v| v["steamId"] != id);
            s.bans.push(json!({"steamId":id,"bannedAtUtc":utc(now),"bannedBy":"rcon","reason":b["reason"].as_str().filter(|v|!v.is_empty())}));
            s.log(
                format!("ban {id} {}", string(&b["reason"])).trim().into(),
                now,
            );
            return ok(
                json!({"message":format!("Banned {}.",player.map(|p|p.name).unwrap_or_else(||id.into()))}),
                200,
            );
        }
        "POST reserved-slots" => {
            let id = string(&b["steamId"]);
            if !valid_id(id) {
                return fail(400, "steamId must be a 17-digit SteamID64.", "error");
            }
            if s.reserved.iter().any(|v| v == id) {
                return fail(
                    409,
                    format!("SteamId {id} is already reserved."),
                    "already_reserved",
                );
            }
            s.reserved.push(id.into());
            s.config_text =
                ini::set_array(&s.config_text, ini::SESSION, ini::RESERVED, &s.reserved);
            s.config_revision += 1;
            s.log(format!("reserved add {id}"), now);
            return ok(
                json!({"message":format!("Reserved slot added for {id}.")}),
                200,
            );
        }
        "POST match/map" => {
            let map = string(&b["map"]);
            if !valid_map(map) {
                return fail(400, format!("Unknown map '{map}'."), "error");
            }
            let e = entry(
                &b,
                string(&s.current["lighting"]),
                &s.current["experiences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            );
            s.apply(&e);
            s.reset(now);
            s.log(
                format!("changemap {map} {} {}", e.experiences.join("+"), e.lighting)
                    .trim()
                    .into(),
                now,
            );
            return ok(json!({"message":format!("Changing map to {map}...")}), 200);
        }
        "POST match/end" => {
            s.end(now);
            s.log("endmatch".into(), now);
            return ok(
                json!({"message":format!("Match ended — travelling to {}.",string(&s.current["map"]))}),
                200,
            );
        }
        "POST match/restart" => {
            s.reset(now);
            s.log("restartmatch".into(), now);
            return ok(
                json!({"message":format!("Restarting {}.",string(&s.current["map"]))}),
                200,
            );
        }
        "PUT world/lighting" => {
            let id = string(&b["lighting"]);
            if !catalog()["LIGHTINGS"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["id"] == id)
            {
                return fail(400, format!("Unknown lighting '{id}'."), "error");
            }
            s.current["lighting"] = json!(id);
            s.log(format!("weather {id}"), now);
            return ok(json!({"message":format!("Weather changed to {id}.")}), 200);
        }
        "GET rotation" => {
            return ok(
                json!({"enabled":s.rotation.enabled,"mode":s.rotation.mode,"entries":s.rotation.entries.iter().enumerate().map(|(i,e)|{let mut v=serde_json::to_value(e).unwrap();if e.zone_alternator.is_empty(){v.as_object_mut().unwrap().remove("zoneAlternator");}v["status"]=json!(if i==s.rotation.now_index{"now"}else if i as i64==s.rotation.next_index{"next"}else{""});v}).collect::<Vec<_>>()}),
                200,
            );
        }
        "POST rotation/entries" => {
            let id = string(&b["map"]);
            if !valid_map(id) {
                return fail(400, format!("Unknown map '{id}'."), "error");
            }
            s.rotation
                .entries
                .push(entry(&b, "DayClear", &["KOTH_InfantryOnly".into()]));
            if s.rotation.entries.len() == 2 {
                s.rotation.next_index = 1;
            }
            s.log(format!("addrotation {id}"), now);
            return ok(
                json!({"message":format!("Added rotation entry {} ({id}).",s.rotation.entries.len()-1)}),
                200,
            );
        }
        "POST rotation/save" => {
            s.seed_config();
            s.config_revision += 1;
            s.log("saverotation".into(), now);
            return ok(json!({"message":"Rotation saved."}), 200);
        }
        "PATCH settings" => {
            let mut msgs = vec![];
            if let Some(v) = b.get("scoreTick") {
                s.score_tick = v.as_i64().filter(|n| *n != 0).unwrap_or(24).clamp(18, 30);
                msgs.push(format!("ScoreTick set to {}.", s.score_tick));
                s.log(format!("set scoretick {}", s.score_tick), now);
            }
            if let Some(v) = b.get("rotationEnabled") {
                s.rotation.enabled = crate::http::truthy(v);
                msgs.push(format!(
                    "Rotation {}.",
                    if s.rotation.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ));
                s.log(
                    format!(
                        "set rotationenabled {}",
                        if s.rotation.enabled { "on" } else { "off" }
                    ),
                    now,
                );
            }
            if let Some(v) = b.get("rotationMode") {
                s.rotation.mode = if string(v).eq_ignore_ascii_case("random") {
                    "random"
                } else {
                    "ordered"
                }
                .into();
                msgs.push(format!("Rotation mode set to {}.", s.rotation.mode));
                s.log(format!("set rotationmode {}", s.rotation.mode), now);
            }
            return if msgs.is_empty() {
                fail(400, "No settings to apply.", "error")
            } else {
                ok(json!({"message":msgs.join(" ")}), 200)
            };
        }
        "GET config" => {
            let mut response = ok(
                json!({"revision":s.revision(),"writable":true,"text":s.config_text,"sections":sections(live),"warnings":[]}),
                200,
            );
            response
                .headers
                .insert("etag".into(), format!("\"{}\"", s.revision()));
            return response;
        }
        "POST config/validate" | "PUT config" => {
            let text = body.unwrap_or("");
            let errors = text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| {
                    let t = line.trim();
                    if t.is_empty()
                        || t.starts_with([';', '#'])
                        || (t.starts_with('[') && t.ends_with(']'))
                        || t.contains('=')
                    {
                        None
                    } else {
                        Some(json!({"line":i+1,"message":"Expected key=value or [section]."}))
                    }
                })
                .collect::<Vec<_>>();
            if !errors.is_empty() {
                return ok(
                    json!({"ok":false,"error":{"code":"invalid","message":format!("{} line(s) could not be parsed.",errors.len())},"errors":errors}),
                    400,
                );
            }
            let revision = headers
                .get("if-match")
                .or_else(|| headers.get("If-Match"))
                .map(|v| v.replace('"', ""))
                .unwrap_or_default();
            if method == "PUT"
                && !revision.is_empty()
                && revision != s.revision()
                && qs.get("force").map(String::as_str) != Some("true")
            {
                return ok(
                    json!({"ok":false,"error":{"code":"revision_mismatch","message":format!("The config changed since revision {revision} (now {}).",s.revision())},"revision":s.revision()}),
                    412,
                );
            }
            let outcomes = sections(live)
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| text.contains(&format!("[{}]", string(&c["section"]))))
                .map(|c| json!({"section":c["section"],"state":c["appliesWhen"],"detail":""}))
                .collect::<Vec<_>>();
            if method == "PUT" {
                let has_reserved = text.lines().any(|l| {
                    l.trim()
                        .trim_start_matches(['!', '+', '-', '.'])
                        .split_once('=')
                        .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(ini::RESERVED))
                });
                s.config_text = if has_reserved {
                    if !live {
                        s.reserved = ini::reserved(text)
                            .into_iter()
                            .filter(|id| valid_id(id))
                            .collect();
                    }
                    text.into()
                } else {
                    ini::set_array(text, ini::SESSION, ini::RESERVED, &s.reserved)
                };
                s.config_revision += 1;
                if !live {
                    if let Some(name) = ini::scalar(text, ini::SESSION, "ServerName") {
                        s.server_name = name.trim().into();
                    }
                }
                if let Some(image) = ini::scalar(text, ini::SESSION, "ServerImageURL") {
                    s.sponsor_url = image.trim().trim_matches('"').into();
                }
                let playing = s.rotation.entries.get(s.rotation.now_index).cloned();
                s.rotation.entries = read_rotation(text);
                s.rotation.enabled = ini::scalar(text, ROTATION, "bEnabled")
                    .is_none_or(|v| v.eq_ignore_ascii_case("true"));
                s.rotation.mode = if ini::scalar(text, ROTATION, "RotationMode")
                    .is_some_and(|v| v.to_ascii_lowercase().contains("random"))
                {
                    "random"
                } else {
                    "ordered"
                }
                .into();
                s.rotation.now_index = playing
                    .and_then(|old| {
                        s.rotation.entries.iter().position(|e| {
                            e.map == old.map
                                && e.lighting == old.lighting
                                && e.experiences == old.experiences
                        })
                    })
                    .unwrap_or(0);
                s.rotation.next_index = if s.rotation.entries.is_empty() {
                    -1
                } else {
                    ((s.rotation.now_index + 1) % s.rotation.entries.len()) as i64
                };
                s.log(format!("config -> {}", s.revision()), now);
            }
            return ok(
                json!({"ok":true,"revision":s.revision(),"outcomes":outcomes,"shadowed":[],"stripped":[],"errors":[],"changed":[],"warnings":[],"timingsMs":{"total":3.2}}),
                200,
            );
        }
        _ => {}
    }
    match p.as_slice() {
        ["players", id, tail @ ..] => {
            let Some(index) = s.players.iter().position(|p| p.steam_id == *id) else {
                return fail(404, format!("Player not found: {id}"), "player_not_found");
            };
            let name = s.players[index].name.clone();
            match (method, tail) {
                ("POST", ["kick"]) => {
                    s.players.remove(index);
                    s.log(
                        format!("kick {id} {}", string(&b["reason"])).trim().into(),
                        now,
                    );
                    return ok(json!({"message":format!("Kicked {name}.")}), 200);
                }
                ("POST", ["kill"]) => {
                    s.players[index].deaths += 1;
                    s.log(format!("kill {id}"), now);
                    return ok(json!({"message":format!("Killed {name}.")}), 200);
                }
                ("POST", ["message"]) => {
                    let text = string(&b["message"]);
                    if text.is_empty() {
                        return fail(400, "message is required.", "error");
                    }
                    s.log(format!("msg {id} {text}"), now);
                    return ok(json!({"message":format!("Message sent to {name}.")}), 200);
                }
                ("PATCH", []) => {
                    let f = string(&b["faction"]);
                    if !s.factions.iter().any(|v| v["name"] == f) {
                        return fail(400, format!("Unknown faction '{f}'."), "error");
                    }
                    s.players[index].faction = f.into();
                    s.log(format!("changeteam {id} {f}"), now);
                    return ok(json!({"message":format!("Moved {name} to {f}.")}), 200);
                }
                _ => {}
            }
        }
        ["bans", id] if method == "DELETE" => {
            let old = s.bans.len();
            s.bans.retain(|b| b["steamId"] != *id);
            if old == s.bans.len() {
                return fail(
                    404,
                    format!("Error: SteamId {id} is not currently banned."),
                    "ban_not_found",
                );
            }
            s.log(format!("unban {id}"), now);
            return ok(json!({"message":format!("Unbanned {id}.")}), 200);
        }
        ["reserved-slots", id] if method == "DELETE" => {
            let old = s.reserved.len();
            s.reserved.retain(|v| v != id);
            if old == s.reserved.len() {
                return fail(
                    404,
                    format!("{id} has no reserved slot."),
                    "reserved_slot_not_found",
                );
            }
            s.config_text =
                ini::set_array(&s.config_text, ini::SESSION, ini::RESERVED, &s.reserved);
            s.config_revision += 1;
            s.log(format!("reserved remove {id}"), now);
            return ok(
                json!({"message":format!("Reserved slot removed for {id}.")}),
                200,
            );
        }
        ["catalog", "maps", map, "experiences"] if method == "GET" => {
            return ok(
                json!({"experiences":catalog()["EXPERIENCES_BY_MAP"].get(*map).cloned().unwrap_or(json!([]))}),
                200,
            );
        }
        ["catalog", "maps", map, "alternators"] if method == "GET" => {
            return ok(
                json!({"alternators":catalog()["ALTERNATORS_BY_MAP"].get(*map).cloned().unwrap_or(json!([]))}),
                200,
            );
        }
        ["rotation", "entries", index, tail @ ..] => {
            let Some(index) = index
                .parse::<usize>()
                .ok()
                .filter(|i| *i < s.rotation.entries.len())
            else {
                return fail(
                    400,
                    format!("Rotation index {index} out of range."),
                    "error",
                );
            };
            if method == "DELETE" {
                s.rotation.entries.remove(index);
                s.rotation.now_index = s
                    .rotation
                    .now_index
                    .min(s.rotation.entries.len().saturating_sub(1));
                s.rotation.next_index = if s.rotation.entries.len() > 1 {
                    ((s.rotation.now_index + 1) % s.rotation.entries.len()) as i64
                } else {
                    s.rotation.now_index as i64
                };
                s.log(format!("removerotation {index}"), now);
                return ok(
                    json!({"message":format!("Removed rotation entry {index}.")}),
                    200,
                );
            }
            if method == "POST" && tail == ["move"] {
                let direction = string(&b["direction"]);
                let to = if direction == "up" {
                    index.checked_sub(1)
                } else {
                    Some(index + 1)
                };
                let Some(to) = to.filter(|i| *i < s.rotation.entries.len()) else {
                    return fail(
                        400,
                        format!("Cannot move rotation entry {index} {direction}."),
                        "error",
                    );
                };
                s.rotation.entries.swap(index, to);
                let swap = |i| {
                    if i == index as i64 {
                        to as i64
                    } else if i == to as i64 {
                        index as i64
                    } else {
                        i
                    }
                };
                s.rotation.now_index = swap(s.rotation.now_index as i64) as usize;
                s.rotation.next_index = swap(s.rotation.next_index);
                s.log(format!("moverotation {index} {direction}"), now);
                return ok(
                    json!({"message":format!("Moved rotation entry {index} {direction}.")}),
                    200,
                );
            }
        }
        _ => {}
    }
    fail(404, "No such endpoint.", "not_found")
}
pub fn request(
    runtime: &Runtime,
    server: &str,
    password: &str,
    method: &str,
    path: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> GameResponse {
    if password != "demo" {
        return fail(
            401,
            "Bad RCON password (the demo server's password is 'demo').",
            "unauthorized",
        );
    }
    let now = Utc::now().timestamp_millis();
    let mut states = runtime.demo.lock().unwrap_or_else(|e| e.into_inner());
    let state = states
        .entry(server.into())
        .or_insert_with(|| DemoState::new(server, now));
    let every = std::env::var("MOCK_RATE_LIMIT_EVERY")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    state.requests += 1;
    if every > 0 && state.requests % every == 0 {
        let mut response = fail(429, "Too many requests from this address.", "rate_limited");
        response.headers.insert("retry-after".into(), "2".into());
        return response;
    }
    state.tick(now);
    handle(
        state,
        server,
        &method.to_ascii_uppercase(),
        path,
        headers,
        body,
        live_build(),
        now,
    )
}
/// Drain only explicit demos after a committed roster, and only when their feed is enabled.
pub async fn ingest_queued(app: &AppState, server: &str) -> Result<()> {
    if !app
        .runtime
        .demo
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(server)
    {
        return Ok(());
    }
    let configured:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM servers WHERE id=$1 AND lower(trim(host))='demo' AND feed_token_hash IS NOT NULL)").bind(server).fetch_one(&app.db).await?;
    let batch = {
        let mut states = app.runtime.demo.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = states.get_mut(server) else {
            return Ok(());
        };
        let events = std::mem::take(&mut s.feed);
        if !configured || events.is_empty() {
            return Ok(());
        }
        json!({"serverId":format!("demo-{server}"),"serverName":s.server_name,"events":events})
    };
    app.runtime.check().await?;
    crate::feed::ingest(&app.db, server, batch, Utc::now()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(s: &mut DemoState, m: &str, p: &str, b: Value, live: bool) -> GameResponse {
        handle(
            s,
            "fixture",
            m,
            p,
            &HashMap::new(),
            Some(&b.to_string()),
            live,
            1780000000000,
        )
    }
    fn value(r: GameResponse) -> Value {
        assert_eq!(r.status, 200, "{}", r.text);
        serde_json::from_str(&r.text).unwrap()
    }
    #[test]
    fn original_demo_routes_and_mutations_are_native() {
        let mut s = DemoState::new("fixture", 1780000000000);
        for (route, key, source) in [
            ("maps", "maps", "MAPS"),
            ("lightings", "lightings", "LIGHTINGS"),
            ("experiences", "experiences", "EXPERIENCES"),
        ] {
            assert_eq!(
                value(call(
                    &mut s,
                    "GET",
                    &format!("/v1/catalog/{route}"),
                    Value::Null,
                    false
                ))[key],
                catalog()[source]
            );
        }
        assert_eq!(
            value(call(&mut s, "GET", "/v1/players", Value::Null, false))["players"]
                .as_array()
                .unwrap()
                .len(),
            9
        );
        let id = "76561198100000101";
        value(call(
            &mut s,
            "PATCH",
            &format!("/v1/players/{id}"),
            json!({"faction":"Lonestar"}),
            false,
        ));
        assert_eq!(s.players[0].faction, "Lonestar");
        value(call(
            &mut s,
            "POST",
            &format!("/v1/players/{id}/kill"),
            Value::Null,
            false,
        ));
        assert_eq!(s.players[0].deaths, 7);
        assert_eq!(
            call(
                &mut s,
                "POST",
                &format!("/v1/players/{id}/message"),
                Value::Null,
                false
            )
            .status,
            400
        );
        value(call(
            &mut s,
            "POST",
            &format!("/v1/players/{id}/message"),
            json!({"message":"你好"}),
            false,
        ));
        value(call(
            &mut s,
            "POST",
            "/v1/reserved-slots",
            json!({"steamId":id}),
            false,
        ));
        assert!(ini::reserved(&s.config_text).contains(&id.to_owned()));
        assert_eq!(
            call(
                &mut s,
                "POST",
                "/v1/reserved-slots",
                json!({"steamId":id}),
                false
            )
            .status,
            409
        );
        value(call(
            &mut s,
            "DELETE",
            &format!("/v1/reserved-slots/{id}"),
            Value::Null,
            false,
        ));
        assert_eq!(s.reserved.len(), 2);
        value(call(
            &mut s,
            "POST",
            "/v1/bans",
            json!({"steamId":id,"reason":"test"}),
            false,
        ));
        assert_eq!(s.players.len(), 8);
        assert!(s.bans.iter().any(|b| b["steamId"] == id));
        value(call(
            &mut s,
            "DELETE",
            &format!("/v1/bans/{id}"),
            Value::Null,
            false,
        ));
        assert_eq!(
            call(&mut s, "POST", "/v1/bans", json!({"steamId":id}), true).status,
            404
        );
        value(call(
            &mut s,
            "POST",
            "/v1/rotation/entries",
            json!({"map":"Europe"}),
            false,
        ));
        assert_eq!(s.rotation.entries.len(), 4);
        value(call(
            &mut s,
            "POST",
            "/v1/rotation/entries/3/move",
            json!({"direction":"up"}),
            false,
        ));
        value(call(
            &mut s,
            "DELETE",
            "/v1/rotation/entries/2",
            Value::Null,
            false,
        ));
        value(call(
            &mut s,
            "PATCH",
            "/v1/settings",
            json!({"scoreTick":99,"rotationMode":"random"}),
            false,
        ));
        assert_eq!(s.score_tick, 30);
        assert_eq!(s.rotation.mode, "random");
        value(call(
            &mut s,
            "POST",
            "/v1/rotation/save",
            Value::Null,
            false,
        ));
        value(call(&mut s, "POST", "/v1/match/end", Value::Null, false));
        assert_eq!(s.current["map"], "Europe");
        assert!(s.players.iter().all(|p| p.kills == 0 && p.cash == 0));
        assert_eq!(
            call(&mut s, "PUT", "/v1/sponsor", Value::Null, false).status,
            405
        );
        value(call(
            &mut s,
            "POST",
            "/v1/broadcast",
            json!({"message":"测试"}),
            false,
        ));
        assert!(
            s.audit.last().unwrap()["detail"]
                .as_str()
                .unwrap()
                .contains("测试")
        );
    }
    #[test]
    fn demo_config_etag_live_build_locks_and_feed_shape() {
        let mut s = DemoState::new("fixture", 1780000000000);
        let config = value(call(&mut s, "GET", "/v1/config", Value::Null, true));
        assert_eq!(
            config["sections"][6]["keyOverrides"][0]["lockedBy"],
            "RCONPort"
        );
        let stale = handle(
            &mut s,
            "fixture",
            "PUT",
            "/v1/config",
            &HashMap::from([("if-match".into(), "\"stale\"".into())]),
            Some("[x]\na=1"),
            false,
            1780000000000,
        );
        assert_eq!(stale.status, 412);
        assert_eq!(
            handle(
                &mut s,
                "fixture",
                "POST",
                "/v1/config/validate",
                &HashMap::new(),
                Some("invalid"),
                false,
                1780000000000
            )
            .status,
            400
        );
        let original = s.config_text.clone();
        let modified = original.replace(
            "ServerName=Warcon Demo Server [fixtur]",
            "ServerName=changed",
        );
        assert_eq!(
            handle(
                &mut s,
                "fixture",
                "PUT",
                "/v1/config",
                &HashMap::new(),
                Some(&modified),
                false,
                1780000000000
            )
            .status,
            200
        );
        assert_eq!(s.server_name, "changed");
        assert_eq!(s.rotation.entries.len(), 3);
        assert_eq!(
            call(
                &mut s,
                "PATCH",
                "/v1/settings",
                json!({"scoreTick":30}),
                true
            )
            .status,
            404
        );
        let k = s.players[0].clone();
        let v = s.players[3].clone();
        s.queue_kill(&k, &v, 1780000000000, &mut rand::thread_rng());
        let parsed =
            crate::feed::parse_batch(&json!({"serverId":"demo-fixture","events":s.feed})).unwrap();
        assert_eq!(parsed.kills.len(), 1);
        assert_eq!(
            parsed.kills[0].killer_steam_id.as_deref(),
            Some(k.steam_id.as_str())
        );
        assert!(!parsed.kills[0].distance_invalid);
        let runtime = Runtime::default();
        assert_eq!(
            request(
                &runtime,
                "x",
                "bad",
                "GET",
                "/v1/status",
                &HashMap::new(),
                None
            )
            .status,
            401
        );
    }
}
