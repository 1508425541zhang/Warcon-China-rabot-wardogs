//! The original poller's pure presence, counter and match-boundary state machines.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub steam_id: String,
    pub name: String,
    pub faction: Option<String>,
    #[serde(default)]
    pub kills: i64,
    #[serde(default)]
    pub deaths: i64,
    #[serde(default)]
    pub cash: i64,
    #[serde(default)]
    pub ping: Option<f64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Counters {
    pub kills: i64,
    pub deaths: i64,
    pub cash: i64,
}
impl From<&Player> for Counters {
    fn from(p: &Player) -> Self {
        Self {
            kills: p.kills,
            deaths: p.deaths,
            cash: p.cash,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: i64,
    pub steam_id: String,
    pub name: String,
    pub faction: Option<String>,
    pub kills: i64,
    pub deaths: i64,
    pub cash: i64,
    pub game: Option<Counters>,
    pub seed_ms: i64,
    pub pending_seed_ms: i64,
    pub joined_at: i64,
    pub last_seen: i64,
    pub written_at: i64,
    pub first_visit: bool,
    pub last_faction: Option<String>,
    pub team: Option<String>,
}
pub fn is_team(faction: Option<&str>, teams: Option<&[String]>) -> bool {
    faction.is_some_and(|f| !f.is_empty() && teams.is_none_or(|teams| teams.iter().any(|s| s == f)))
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceDiff {
    pub joined: Vec<Player>,
    pub left: Vec<Session>,
    pub stayed: Vec<Player>,
    pub factioned: Vec<Value>,
    pub renamed: Vec<Player>,
    pub returned: Vec<Player>,
}
pub fn diff(
    open: &HashMap<String, Session>,
    players: &[Player],
    now: i64,
    grace: i64,
    previous: i64,
    teams: Option<&[String]>,
) -> PresenceDiff {
    let mut out = PresenceDiff::default();
    let mut seen = HashSet::new();
    for p in players {
        if p.steam_id.is_empty() || !seen.insert(&p.steam_id) {
            continue;
        }
        if let Some(s) = open.get(&p.steam_id) {
            out.stayed.push(p.clone());
            if is_team(p.faction.as_deref(), teams) && p.faction != s.last_faction {
                out.factioned
                    .push(json!({"player":p,"from":s.last_faction}));
            }
            if p.name != s.name {
                out.renamed.push(p.clone());
            }
            if s.last_seen < previous {
                out.returned.push(p.clone());
            }
        } else {
            out.joined.push(p.clone());
        }
    }
    out.left = open
        .values()
        .filter(|s| !seen.contains(&s.steam_id) && now - s.last_seen > grace)
        .cloned()
        .collect();
    out
}
pub fn follow(s: &mut Session, p: &Player, now: i64, teams: Option<&[String]>) {
    s.name = p.name.clone();
    s.faction = p.faction.clone();
    if is_team(p.faction.as_deref(), teams) {
        s.last_faction = p.faction.clone();
        s.team = p.faction.clone();
    }
    let restart = s
        .game
        .as_ref()
        .is_some_and(|g| p.kills < g.kills || p.deaths < g.deaths);
    let unloaded = s.game.is_none();
    let before = |stored: i64, value: i64, previous: i64| {
        if unloaded {
            (stored - value).max(0)
        } else if restart {
            stored
        } else {
            stored - previous
        }
    };
    s.kills = before(
        s.kills,
        p.kills,
        s.game.as_ref().map(|g| g.kills).unwrap_or(0),
    ) + p.kills;
    s.deaths = before(
        s.deaths,
        p.deaths,
        s.game.as_ref().map(|g| g.deaths).unwrap_or(0),
    ) + p.deaths;
    s.cash = before(s.cash, p.cash, s.game.as_ref().map(|g| g.cash).unwrap_or(0)) + p.cash;
    s.game = Some(p.into());
    s.last_seen = now;
}
pub fn joined(
    id: i64,
    p: &Player,
    now: i64,
    teams: Option<&[String]>,
    first_visit: bool,
) -> Session {
    let team = if is_team(p.faction.as_deref(), teams) {
        p.faction.clone()
    } else {
        None
    };
    Session {
        id,
        steam_id: p.steam_id.clone(),
        name: p.name.clone(),
        faction: p.faction.clone(),
        kills: p.kills,
        deaths: p.deaths,
        cash: p.cash,
        game: Some(p.into()),
        seed_ms: 0,
        pending_seed_ms: 0,
        joined_at: now,
        last_seen: now,
        written_at: now,
        first_visit,
        last_faction: team.clone(),
        team,
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    pub steam_id: String,
    pub name: String,
    pub faction: Option<String>,
    pub banked: Counters,
    pub last: Pair,
    pub cash_first: i64,
    pub cash_last: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cash_before_reset: Option<i64>,
    pub dropped_at: i64,
    pub ms: i64,
    pub last_seen: i64,
    pub dirty: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pair {
    pub kills: i64,
    pub deaths: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TallyRow {
    pub steam_id: String,
    pub name: String,
    pub faction: Option<String>,
    pub seconds: i64,
    pub kills: i64,
    pub deaths: i64,
    pub cash_delta: i64,
}
pub fn tally(
    open: &mut HashMap<String, Tally>,
    players: &[Player],
    now: i64,
    gap: i64,
    stayed: &HashSet<String>,
    teams: Option<&[String]>,
) {
    for p in players {
        if p.steam_id.is_empty() {
            continue;
        }
        if let Some(t) = open.get_mut(&p.steam_id) {
            if p.kills < t.last.kills || p.deaths < t.last.deaths {
                t.cash_before_reset = Some(t.cash_last);
                t.banked.kills += t.last.kills;
                t.banked.deaths += t.last.deaths;
                t.banked.cash += t.cash_last - t.cash_first;
                t.cash_first = p.cash;
                t.dropped_at = now;
            }
            t.last = Pair {
                kills: p.kills,
                deaths: p.deaths,
            };
            t.cash_last = p.cash;
            t.name = p.name.clone();
            if is_team(p.faction.as_deref(), teams) {
                t.faction = p.faction.clone()
            }
            if stayed.contains(&p.steam_id) {
                t.ms += gap
            }
            t.last_seen = now;
        } else {
            open.insert(
                p.steam_id.clone(),
                Tally {
                    steam_id: p.steam_id.clone(),
                    name: p.name.clone(),
                    faction: if is_team(p.faction.as_deref(), teams) {
                        p.faction.clone()
                    } else {
                        None
                    },
                    banked: Counters::default(),
                    last: Pair {
                        kills: p.kills,
                        deaths: p.deaths,
                    },
                    cash_first: p.cash,
                    cash_last: p.cash,
                    cash_before_reset: None,
                    dropped_at: 0,
                    ms: 0,
                    last_seen: now,
                    dirty: false,
                },
            );
        }
    }
}
fn seconds(ms: i64) -> i64 {
    (ms as f64 / 1000. + 0.5).floor() as i64
}
pub fn row(t: &Tally) -> TallyRow {
    TallyRow {
        steam_id: t.steam_id.clone(),
        name: t.name.clone(),
        faction: t.faction.clone(),
        seconds: seconds(t.ms),
        kills: t.banked.kills + t.last.kills,
        deaths: t.banked.deaths + t.last.deaths,
        cash_delta: t.banked.cash + t.cash_last - t.cash_first,
    }
}
pub fn close(
    open: &HashMap<String, Tally>,
    previous: i64,
) -> (Vec<TallyRow>, HashMap<String, Tally>) {
    let mut rows = vec![];
    let mut carried = HashMap::new();
    for t in open.values() {
        if t.dropped_at > 0 && t.dropped_at >= previous {
            rows.push(TallyRow {
                steam_id: t.steam_id.clone(),
                name: t.name.clone(),
                faction: t.faction.clone(),
                seconds: seconds(t.ms),
                kills: t.banked.kills,
                deaths: t.banked.deaths,
                cash_delta: t.banked.cash,
            });
            let mut next = t.clone();
            next.banked = Counters::default();
            next.dropped_at = 0;
            next.ms = 0;
            next.dirty = false;
            carried.insert(next.steam_id.clone(), next);
        } else {
            rows.push(row(t));
        }
    }
    (rows, carried)
}
pub fn boundary(prev: Option<&Value>, next: &Value) -> Option<Value> {
    let prev = prev?;
    let scores = |s: &Value| s["scores"].as_array().cloned().unwrap_or_default();
    let total = |s: &Value| {
        scores(s)
            .iter()
            .map(|s| s["score"].as_f64().unwrap_or(0.))
            .sum::<f64>()
    };
    let clock = match (prev["matchSeconds"].as_f64(), next["matchSeconds"].as_f64()) {
        (Some(a), Some(b)) => b < a - 30.,
        _ => false,
    };
    if prev["map"] == next["map"] && !clock && total(next) >= total(prev) {
        return None;
    }
    let mut scores = scores(prev);
    scores.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.))
    });
    let top = scores
        .first()
        .and_then(|s| s["score"].as_f64())
        .unwrap_or(0.);
    let leaders = scores
        .iter()
        .filter(|s| top > 0. && s["score"].as_f64() == Some(top))
        .map(|s| s["name"].clone())
        .collect::<Vec<_>>();
    Some(
        json!({"map":prev["map"],"scores":scores,"winner":if leaders.len()==1{leaders[0].clone()}else{Value::Null},"leaders":leaders}),
    )
}
pub fn phase(id: &str, interval: u64) -> u64 {
    let mut hash = 0x811c9dc5u32;
    for unit in id.encode_utf16() {
        hash = (hash ^ unit as u32).wrapping_mul(0x01000193)
    }
    if interval > 0 {
        hash as u64 % interval
    } else {
        0
    }
}
pub fn next_due(due: i64, interval: i64, now: i64) -> i64 {
    let next = due.saturating_add(interval);
    if next > now {
        next
    } else {
        now.saturating_add(interval)
    }
}
