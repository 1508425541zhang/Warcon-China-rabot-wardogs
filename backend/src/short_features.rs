//! Causal 60/120-second inputs from the original short_risk.py contract.
use anyhow::{Result, ensure};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
pub const FEATURES: [&str; 24] = [
    "small_arm_kills_60s",
    "kills_60s",
    "deaths_60s",
    "headshot_rate_60s",
    "penetration_rate_60s",
    "unique_victims_60s",
    "max_kills_15s_60s",
    "median_interval_s_60s",
    "distance_ratio_p90_60s",
    "headshot_samples_60s",
    "distance_samples_60s",
    "small_arm_kills_120s",
    "kills_120s",
    "deaths_120s",
    "headshot_rate_120s",
    "penetration_rate_120s",
    "unique_victims_120s",
    "max_kills_15s_120s",
    "median_interval_s_120s",
    "distance_ratio_p90_120s",
    "headshot_samples_120s",
    "distance_samples_120s",
    "kill_rate_change",
    "dominant_weapon_fraction_120s",
];
const ARMS: [&str; 16] = [
    "AK74M",
    "Mosin",
    "MP9",
    "WEPN_029",
    "M4",
    "M500",
    "MP43",
    "SKS",
    "SVDM",
    "KH2002",
    "TAR21",
    "A91",
    "SV98",
    "MK22",
    "Glock17",
    "CombatBow",
];
pub fn utc(v: &Value) -> Result<f64> {
    if let Some(n) = number(v) {
        return Ok(n);
    }
    Ok(crate::legacy_features::timestamp(v)?
        .ok_or_else(|| anyhow::anyhow!("Decision timestamp required"))? as f64
        / 1000.)
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}
#[derive(Clone, Debug)]
pub struct Event {
    pub raw: Value,
    pub received: f64,
    pub clock: f64,
    pub scope: [String; 4],
}
pub fn events(raw: &Value) -> Result<Vec<Event>> {
    let raw = raw
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Event array required"))?;
    ensure!(raw.len() <= 100000, "Too many events");
    let mut keys = HashMap::new();
    let mut out = Vec::new();
    for e in raw {
        ensure!(e.is_object(), "Event object required");
        let clock = number(&e["event_time"])
            .filter(|n| *n >= 0.)
            .ok_or_else(|| anyhow::anyhow!("Invalid game clock"))?;
        let received = utc(&e["ts"])?;
        let string = |k: &str| {
            e[k].as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid event identity"))
        };
        let sid = string("server_id")?.to_owned();
        let instance = string("instance_id")?.to_owned();
        let eid = string("event_id")?.to_owned();
        let key = (sid.clone(), instance.clone(), eid);
        if let Some(old) = keys.get(&key) {
            ensure!(old == e, "Conflicting event");
            continue;
        }
        keys.insert(key, e.clone());
        let m = &e["match_row"];
        let mid = if m.is_null() {
            "None".into()
        } else {
            m.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| m.to_string())
        };
        let scope = [sid, instance, mid, e["map"].as_str().unwrap_or("").into()];
        out.push(Event {
            raw: e.clone(),
            clock,
            received,
            scope,
        });
    }
    ensure!(
        out.iter().map(|e| &e.scope).collect::<HashSet<_>>().len() <= 1,
        "Round isolation required"
    );
    Ok(out)
}
fn attack(e: &Event, player: &str) -> bool {
    e.raw["killer_steam_id"] == player
        && e.raw["victim_steam_id"] != player
        && e.raw["suicide"] != true
        && e.raw["team_kill"] != true
        && !e.raw["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|t| matches!(t.as_str(), Some("Suicide" | "Falling" | "RoadKill")))
}
fn arm(e: &Event, player: &str) -> bool {
    attack(e, player)
        && e.raw["cause"]
            .as_str()
            .and_then(|s| s.strip_prefix("Id.Item."))
            .is_some_and(|s| ARMS.contains(&s))
}
fn quantile(values: &mut [f64], q: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    let p = (values.len() - 1) as f64 * q;
    let lo = p.floor() as usize;
    let hi = p.ceil() as usize;
    values[lo] + (values[hi] - values[lo]) * p.fract()
}
pub fn vector(
    events: &[Event],
    player: &str,
    clock: f64,
    received: f64,
    baseline: &Value,
) -> Result<[f32; 24]> {
    ensure!(
        clock.is_finite() && received.is_finite(),
        "Finite decision clocks required"
    );
    let available = events
        .iter()
        .filter(|e| e.received <= received && e.clock <= clock && e.clock > clock - 120.)
        .collect::<Vec<_>>();
    let mut values = Vec::new();
    let mut rates = Vec::new();
    let mut weapons = Vec::new();
    for window in [60., 120.] {
        let recent = available
            .iter()
            .copied()
            .filter(|e| e.clock > clock - window)
            .collect::<Vec<_>>();
        let attacks = recent.iter().filter(|e| attack(e, player)).count();
        let mut arms = recent
            .iter()
            .copied()
            .filter(|e| arm(e, player))
            .collect::<Vec<_>>();
        arms.sort_by(|a, b| {
            a.clock
                .total_cmp(&b.clock)
                .then(a.raw["event_id"].as_str().cmp(&b.raw["event_id"].as_str()))
        });
        let deaths = recent
            .iter()
            .filter(|e| e.raw["victim_steam_id"] == player)
            .count();
        let mut head = Vec::new();
        let mut pen = Vec::new();
        let mut intervals = Vec::new();
        let mut distances = Vec::new();
        let mut left = 0;
        let mut burst = 0;
        for (i, e) in arms.iter().enumerate() {
            if let Some(tags) = e.raw["tags"].as_array() {
                if let Some(h) = e.raw["headshot"].as_bool() {
                    head.push(i32::from(h) as f64)
                }
                let penetrated = e.raw["penetration"].as_bool().unwrap_or_else(|| {
                    tags.iter().any(|t| {
                        t.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| t.to_string())
                            .contains("Penetration")
                    })
                });
                pen.push(i32::from(penetrated) as f64);
            }
            if i > 0 {
                intervals.push(e.clock - arms[i - 1].clock)
            }
            while e.clock - arms[left].clock > 15. {
                left += 1
            }
            burst = burst.max(i - left + 1);
            if let (Some(d), Some(w)) = (number(&e.raw["distance_m"]), e.raw["cause"].as_str()) {
                if (0. ..=2000.).contains(&d) && e.raw["distance_invalid"] != true {
                    if let Some(p) = number(&baseline[w]["p95_m"]).filter(|p| *p > 0.) {
                        distances.push(d / p)
                    }
                }
            }
        }
        let avg = |v: &Vec<f64>| {
            if v.is_empty() {
                f64::NAN
            } else {
                v.iter().sum::<f64>() / v.len() as f64
            }
        };
        let victims = arms
            .iter()
            .map(|e| e.raw["victim_steam_id"].to_string())
            .collect::<HashSet<_>>()
            .len();
        let median = quantile(&mut intervals, 0.5);
        let distance = quantile(&mut distances, 0.9);
        values.extend([
            arms.len() as f64,
            attacks as f64,
            deaths as f64,
            avg(&head),
            avg(&pen),
            victims as f64,
            burst as f64,
            median,
            distance,
            head.len() as f64,
            distances.len() as f64,
        ]);
        rates.push(arms.len() as f64);
        if window == 120. {
            weapons = arms
                .iter()
                .filter_map(|e| e.raw["cause"].as_str())
                .collect();
        }
    }
    let mut counts = HashMap::<&str, usize>::new();
    for w in &weapons {
        *counts.entry(w).or_default() += 1
    }
    values.push(rates[0] - (rates[1] - rates[0]));
    values.push(if weapons.is_empty() {
        f64::NAN
    } else {
        *counts.values().max().unwrap() as f64 / weapons.len() as f64
    });
    Ok(values
        .into_iter()
        .map(|n| n as f32)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap())
}
pub fn has_source(events: &[Event], player: &str, clock: f64, received: f64) -> bool {
    events.iter().any(|e| {
        e.received <= received
            && clock - 120. < e.clock
            && e.clock <= clock
            && (e.raw["killer_steam_id"] == player || e.raw["victim_steam_id"] == player)
    })
}
