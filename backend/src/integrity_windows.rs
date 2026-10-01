//! Game-clock infantry episodes shared by live evaluation and historical replay.
use crate::{integrity_score::num, integrity_weapons};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
pub const WINDOW_SECONDS: f64 = 180.;
#[derive(Clone, Debug)]
pub struct Entry {
    pub clock: f64,
    pub event_id: String,
    pub victim: String,
    pub cause: Option<String>,
    pub distance: Option<f64>,
    pub headshot: bool,
    pub penetration: bool,
}
#[derive(Clone, Default)]
struct Severity {
    kpm: f64,
    victims: f64,
    headshot: bool,
    penetration: bool,
    burst: f64,
}
#[derive(Clone, Default)]
struct Player {
    entries: Vec<Entry>,
    episode: HashSet<String>,
    anchor: Option<f64>,
    window_id: Option<i64>,
    best: Option<Severity>,
    reasons: Vec<String>,
    peak: f64,
}
#[derive(Clone)]
struct Server {
    instance: String,
    match_row: Option<i64>,
    sequence: u64,
    map: String,
    clock: f64,
    players: HashMap<String, Player>,
}
#[derive(Clone, Default)]
pub struct Windows {
    servers: HashMap<String, Server>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub steam_id: String,
    pub instance_id: String,
    pub round_id: String,
    pub map: String,
    pub anchor_clock: f64,
    pub window_id: Option<i64>,
    pub clock_from: f64,
    pub clock_to: f64,
    pub infantry_kills: usize,
    pub kpm180: f64,
    pub unique_victims: usize,
    pub headshots: usize,
    pub headshot_pct: f64,
    pub penetrations: usize,
    pub penetration_pct: f64,
    pub burst_points: f64,
    pub max_kills15s: usize,
    pub median_kill_interval: Option<f64>,
    pub weapon_metrics: Vec<Value>,
    pub reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_only: Option<bool>,
    pub event_ids: Vec<String>,
}
fn tier(value: f64, bands: &Value) -> f64 {
    bands
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, b)| value >= num(b, "min"))
        .map(|(i, _)| (i + 1) as f64)
        .last()
        .unwrap_or(0.)
}
pub fn burst(clocks: &[f64]) -> f64 {
    let mut clocks = clocks.to_vec();
    clocks.sort_by(f64::total_cmp);
    for (count, seconds, points) in [
        (8, 15., 12.),
        (6, 12., 10.),
        (5, 10., 7.),
        (4, 8., 4.),
        (3, 5., 2.),
    ] {
        if clocks
            .windows(count)
            .any(|w| w[count - 1] - w[0] <= seconds)
        {
            return points;
        }
    }
    0.
}
pub fn max_within(clocks: &[f64], seconds: f64) -> usize {
    let mut clocks = clocks.to_vec();
    clocks.sort_by(f64::total_cmp);
    let mut start = 0;
    let mut max = 0;
    for end in 0..clocks.len() {
        while clocks[start] < clocks[end] - seconds {
            start += 1;
        }
        max = max.max(end - start + 1);
    }
    max
}
pub fn median_interval(clocks: &[f64]) -> Option<f64> {
    if clocks.len() < 2 {
        return None;
    }
    let mut clocks = clocks.to_vec();
    clocks.sort_by(f64::total_cmp);
    let mut gaps: Vec<_> = clocks.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_by(f64::total_cmp);
    Some(if gaps.len() % 2 == 1 {
        gaps[gaps.len() / 2]
    } else {
        (gaps[gaps.len() / 2 - 1] + gaps[gaps.len() / 2]) / 2.
    })
}
pub fn weapon_metrics(entries: &[&Entry]) -> Vec<Value> {
    let mut values: Vec<Value> = Vec::new();
    for entry in entries {
        let Some(cause) = entry.cause.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        let index = if let Some(i) = values.iter().position(|v| v["cause"] == cause) {
            i
        } else {
            values.push(json!({"cause":cause,"kills":0,"headshots":0,"maxKillDistanceM":null}));
            values.len() - 1
        };
        let item = &mut values[index];
        item["kills"] = json!(item["kills"].as_u64().unwrap() + 1);
        if entry.headshot {
            item["headshots"] = json!(item["headshots"].as_u64().unwrap() + 1);
        }
        if let Some(d) = entry
            .distance
            .filter(|d| d.is_finite() && *d > 0. && *d < 5000.)
        {
            item["maxKillDistanceM"] =
                json!(item["maxKillDistanceM"].as_f64().unwrap_or(0.).max(d));
        }
    }
    values
}
fn finding(server: &Server, steam_id: &str, player: &Player, snapshot: bool) -> Option<Finding> {
    let entries: Vec<_> = player
        .entries
        .iter()
        .filter(|e| e.clock > server.clock - WINDOW_SECONDS)
        .collect();
    if entries.is_empty() {
        return None;
    }
    let active = entries.iter().any(|e| player.episode.contains(&e.event_id));
    let headshots = entries.iter().filter(|e| e.headshot).count();
    let penetrations = entries.iter().filter(|e| e.penetration).count();
    let clocks: Vec<_> = entries.iter().map(|e| e.clock).collect();
    Some(Finding {
        steam_id: steam_id.into(),
        instance_id: server.instance.clone(),
        round_id: server
            .match_row
            .map(|m| format!("{}:match:{m}", server.instance))
            .unwrap_or_else(|| format!("{}:derived:{}", server.instance, server.sequence)),
        map: server.map.clone(),
        anchor_clock: if active {
            player.anchor.unwrap_or(server.clock)
        } else {
            server.clock
        },
        window_id: if active { player.window_id } else { None },
        clock_from: entries[0].clock,
        clock_to: server.clock,
        infantry_kills: entries.len(),
        kpm180: entries.len() as f64 / 3.,
        unique_victims: entries
            .iter()
            .map(|e| &e.victim)
            .collect::<HashSet<_>>()
            .len(),
        headshots,
        headshot_pct: 100. * headshots as f64 / entries.len() as f64,
        penetrations,
        penetration_pct: 100. * penetrations as f64 / entries.len() as f64,
        burst_points: burst(&clocks),
        max_kills15s: max_within(&clocks, 15.),
        median_kill_interval: median_interval(&clocks),
        weapon_metrics: weapon_metrics(&entries),
        reasons: if active {
            player.reasons.clone()
        } else {
            Vec::new()
        },
        snapshot_only: if snapshot { Some(true) } else { None },
        event_ids: entries.iter().map(|e| e.event_id.clone()).collect(),
    })
}
fn flat_kill(kill: &Value) -> Value {
    json!({"cause":kill["cause"],"tags":kill["tags"],"suicide":kill["suicide"],"teamKill":kill["teamKill"],"killerSteamId":kill["killer"]["steamId"],"victimSteamId":kill["victim"]["steamId"],"killerFaction":kill["killer"]["faction"],"victimFaction":kill["victim"]["faction"],"factionBracketed":kill["factionBracketed"],"factionObservedAt":kill["factionObservedAt"]})
}
pub fn ordered(batch: &[Value]) -> Vec<&Value> {
    let mut ordered: Vec<_> = batch.iter().collect();
    if batch.first().is_some_and(|first| {
        batch.iter().all(|k| {
            k["instanceId"] == first["instanceId"]
                && k["map"] == first["map"]
                && k["matchRow"] == first["matchRow"]
        })
    }) {
        ordered.sort_by(|a, b| num(a, "eventTime").total_cmp(&num(b, "eventTime")));
    }
    ordered
}
impl Windows {
    pub fn reset(&mut self, server: Option<&str>) {
        if let Some(id) = server {
            self.servers.remove(id);
        } else {
            self.servers.clear();
        }
    }
    pub fn has_server(&self, server: &str) -> bool {
        self.servers.contains_key(server)
    }
    pub fn mark_persisted(&mut self, server: &str, f: &Finding, id: i64) {
        if let Some(p) = self
            .servers
            .get_mut(server)
            .and_then(|s| s.players.get_mut(&f.steam_id))
        {
            if p.anchor == Some(f.anchor_clock) {
                p.window_id = Some(id);
            }
        }
    }
    pub fn snapshots(&self, server: &str, ids: &[String]) -> Vec<Finding> {
        let Some(server) = self.servers.get(server) else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        ids.iter()
            .filter(|id| seen.insert((*id).clone()))
            .filter_map(|id| {
                server
                    .players
                    .get(id)
                    .and_then(|p| finding(server, id, p, true))
            })
            .collect()
    }
    pub fn current(&self, server: &str, id: &str) -> Option<Value> {
        let s = self.servers.get(server)?;
        let p = s.players.get(id)?;
        Some(
            json!({"kpm180":p.entries.iter().filter(|e|e.clock>s.clock-WINDOW_SECONDS).count() as f64/3.,"peakKpm180":p.peak}),
        )
    }
    pub fn observe(
        &mut self,
        id: &str,
        batch: &[Value],
        overrides: &HashMap<String, String>,
        config: &Value,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        for kill in ordered(batch) {
            let Some(instance) = kill["instanceId"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let Some(map) = kill["map"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let Some(clock) = kill["eventTime"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.)
            else {
                continue;
            };
            let row = kill["matchRow"].as_i64();
            let old = self.servers.get(id);
            if old.is_some_and(|s| {
                s.instance == instance && s.match_row.zip(row).is_some_and(|(a, b)| b < a)
            }) {
                continue;
            }
            let reset = old.is_none_or(|s| {
                s.instance != instance
                    || s.match_row.zip(row).is_some_and(|(a, b)| b > a)
                    || ((s.match_row.is_none() || row.is_none())
                        && (s.map != map || (clock <= 5. && s.clock >= 20.)))
            });
            if reset {
                let sequence = old.map(|s| s.sequence).unwrap_or(0) + 1;
                self.servers.insert(
                    id.into(),
                    Server {
                        instance: instance.into(),
                        match_row: row,
                        sequence,
                        map: map.into(),
                        clock,
                        players: HashMap::new(),
                    },
                );
            }
            let server = self.servers.get_mut(id).unwrap();
            if server.map != map || clock <= server.clock - WINDOW_SECONDS {
                continue;
            }
            server.clock = server.clock.max(clock);
            let Some(killer) = kill["killer"]["steamId"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            if !integrity_weapons::infantry(&flat_kill(kill), overrides) {
                continue;
            }
            let player = server.players.entry(killer.into()).or_default();
            let event_id = kill["eventId"].as_str().unwrap_or("");
            if player.entries.iter().any(|e| e.event_id == event_id) {
                continue;
            }
            let entry = Entry {
                clock,
                event_id: event_id.into(),
                victim: kill["victim"]["steamId"].as_str().unwrap_or("").into(),
                cause: kill["cause"].as_str().map(str::to_owned),
                distance: kill["distanceM"].as_f64(),
                headshot: kill["headshot"] == true,
                penetration: kill["tags"]
                    .as_array()
                    .is_some_and(|t| t.iter().any(|t| t == "Penetration")),
            };
            let at = player.entries.partition_point(|e| e.clock <= clock);
            player.entries.insert(at, entry);
            player
                .entries
                .retain(|e| e.clock > server.clock - WINDOW_SECONDS);
            let count = player.entries.len();
            let kpm = count as f64 / 3.;
            player.peak = player.peak.max(kpm);
            let heads = player.entries.iter().filter(|e| e.headshot).count();
            let pens = player.entries.iter().filter(|e| e.penetration).count();
            let burst = burst(&player.entries.iter().map(|e| e.clock).collect::<Vec<_>>());
            let mut reasons = Vec::new();
            if kpm >= num(&config["kpmBands"][0], "min") {
                reasons.push("kpm".into());
            }
            if count as f64 >= num(config, "headshotMinKills")
                && 100. * heads as f64 / count as f64 >= num(config, "headshotMinPct")
            {
                reasons.push("headshot".into());
            }
            if count as f64 >= num(config, "penetrationMinKills")
                && 100. * pens as f64 / count as f64 >= num(config, "penetrationMinPct")
            {
                reasons.push("penetration".into());
            }
            if burst >= num(config, "burstFindingMin") {
                reasons.push("burst".into());
            }
            if reasons.is_empty() {
                continue;
            }
            let severity = Severity {
                kpm: tier(kpm, &config["kpmBands"]),
                victims: if reasons.iter().any(|r| r == "kpm") {
                    tier(
                        player
                            .entries
                            .iter()
                            .map(|e| &e.victim)
                            .collect::<HashSet<_>>()
                            .len() as f64,
                        &config["uniqueVictimBands"],
                    )
                } else {
                    0.
                },
                headshot: reasons.iter().any(|r| r == "headshot"),
                penetration: reasons.iter().any(|r| r == "penetration"),
                burst,
            };
            let active = player.anchor.is_some()
                && player
                    .entries
                    .iter()
                    .any(|e| player.episode.contains(&e.event_id));
            if active
                && player.best.as_ref().is_some_and(|b| {
                    severity.kpm <= b.kpm
                        && severity.victims <= b.victims
                        && (!severity.headshot || b.headshot)
                        && (!severity.penetration || b.penetration)
                        && severity.burst <= b.burst
                })
            {
                continue;
            }
            if !active {
                player.anchor = Some(server.clock);
                player.window_id = None;
                player.best = None;
                player.episode.clear();
            }
            player
                .episode
                .extend(player.entries.iter().map(|e| e.event_id.clone()));
            let best = player.best.get_or_insert_with(Severity::default);
            best.kpm = best.kpm.max(severity.kpm);
            best.victims = best.victims.max(severity.victims);
            best.headshot |= severity.headshot;
            best.penetration |= severity.penetration;
            best.burst = best.burst.max(severity.burst);
            player.reasons = reasons;
            if let Some(f) = finding(server, killer, &server.players[killer], false) {
                findings.push(f);
            }
        }
        if let Some(server) = self.servers.get_mut(id) {
            server.players.retain(|_, p| {
                p.entries
                    .retain(|e| e.clock > server.clock - WINDOW_SECONDS);
                !p.entries.is_empty() || p.anchor.is_some_and(|a| server.clock - a <= 900.)
            });
        }
        let mut latest: Vec<Finding> = Vec::new();
        for f in findings {
            if let Some(at) = latest.iter().position(|old| {
                old.steam_id == f.steam_id
                    && old.instance_id == f.instance_id
                    && old.anchor_clock == f.anchor_clock
            }) {
                latest[at] = f;
            } else {
                latest.push(f);
            }
        }
        latest
    }
}
#[derive(Serialize)]
pub struct BatchFeatures {
    pub features: Vec<Value>,
    pub findings: Vec<Finding>,
    pub snapshots: Vec<Finding>,
}
pub fn generate(
    windows: &mut Windows,
    server: &str,
    batch: &[Value],
    overrides: &HashMap<String, String>,
    config: &Value,
) -> BatchFeatures {
    let mut output = BatchFeatures {
        features: Vec::new(),
        findings: Vec::new(),
        snapshots: Vec::new(),
    };
    for event in ordered(batch) {
        output.findings.extend(windows.observe(
            server,
            std::slice::from_ref(event),
            overrides,
            config,
        ));
        let Some(id) = event["killer"]["steamId"].as_str() else {
            continue;
        };
        if let Some(f) = windows
            .snapshots(server, &[id.into()])
            .pop()
            .filter(|f| f.event_ids.iter().any(|e| event["eventId"] == e.as_str()))
        {
            output.features.push(json!({"eventId":event["eventId"],"steamId":f.steam_id,"roundId":f.round_id,"map":f.map,"at":event["ts"],"infantryKills":f.infantry_kills,"kpm180":f.kpm180,"uniqueVictims180":f.unique_victims,"maxKills15s":f.max_kills15s,"medianKillInterval":f.median_kill_interval,"headshotRate":if f.infantry_kills>=10 {Some(f.headshots as f64/f.infantry_kills as f64)}else{None},"penetrationRate":if f.infantry_kills>=10 {Some(f.penetrations as f64/f.infantry_kills as f64)}else{None},"weaponMetrics":f.weapon_metrics,"eventIds":f.event_ids}));
            output.snapshots.push(f);
        }
    }
    output
}
