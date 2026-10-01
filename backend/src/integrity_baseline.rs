//! Bounded, player-balanced reference replay carried across database pages.
use crate::{
    committee::{FEATURE_VERSION, MODEL_VERSION},
    integrity_score, integrity_statistics, integrity_windows,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Deserialize)]
pub struct ReplayRow {
    pub source: String,
    pub server_id: String,
    pub event_id: String,
    pub at: DateTime<Utc>,
    pub instance_id: String,
    pub match_id: String,
    pub match_row: Value,
    pub event_time: f64,
    pub map: String,
    pub killer_steam_id: String,
    pub victim_steam_id: String,
    pub killer_faction: String,
    pub victim_faction: String,
    pub cause: String,
    pub distance_m: Option<f64>,
    pub headshot: bool,
    pub penetration: bool,
    pub player_count: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub player: String,
    pub day: String,
    pub source: String,
    pub server_id: String,
    pub round_id: String,
    #[serde(serialize_with = "serialize_date")]
    pub at: DateTime<Utc>,
    pub map: String,
    pub bucket: Option<String>,
    pub metric: String,
    pub weapon: String,
    pub value: f64,
    pub event_id: String,
}
fn serialize_date<S: serde::Serializer>(
    date: &DateTime<Utc>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
pub fn rank(id: &str) -> u32 {
    id.encode_utf16().fold(2166136261u32, |hash, c| {
        (hash ^ c as u32).wrapping_mul(16777619)
    })
}
fn key(sample: &Sample, daily: bool) -> String {
    let mut parts = vec![
        json!(sample.source),
        json!(sample.server_id),
        json!(sample.player),
    ];
    if daily {
        parts.push(json!(sample.day));
    }
    parts.extend([
        json!(sample.map),
        json!(sample.bucket),
        json!(sample.metric),
        json!(sample.weapon),
    ]);
    serde_json::to_string(&parts).unwrap()
}
// Keep insertion order: stable rank ties must match the original Map/Array implementation.
#[derive(Default)]
struct Groups {
    indices: HashMap<String, usize>,
    rows: Vec<Vec<Sample>>,
}
impl Groups {
    fn add(&mut self, sample: Sample, daily: bool, cap: usize) {
        let key = key(&sample, daily);
        let index = *self.indices.entry(key).or_insert_with(|| {
            self.rows.push(Vec::new());
            self.rows.len() - 1
        });
        let group = &mut self.rows[index];
        group.push(sample);
        group.sort_by_key(|s| rank(&s.event_id));
        group.truncate(cap);
    }
}
pub struct Replay {
    windows: integrity_windows::Windows,
    daily: Groups,
    overrides: HashMap<String, String>,
    rules: Value,
}
impl Replay {
    pub fn new(overrides: HashMap<String, String>) -> Self {
        Self {
            windows: Default::default(),
            daily: Default::default(),
            overrides,
            rules: integrity_score::defaults(),
        }
    }
    pub fn add(&mut self, row: &ReplayRow) {
        let match_row = if row.match_row.is_null() {
            None
        } else {
            let number = row
                .match_row
                .as_f64()
                .or_else(|| row.match_row.as_str().and_then(|s| s.parse::<f64>().ok()));
            let Some(number) = number.filter(|n| {
                n.is_finite() && n.fract() == 0. && *n >= 1. && *n <= 9007199254740991.
            }) else {
                return;
            };
            Some(number as i64)
        };
        let event = json!({"eventId":row.event_id,"ts":row.at.to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"instanceId":row.instance_id,"matchId":row.match_id,"matchRow":match_row,"map":row.map,"eventTime":row.event_time,"killer":{"steamId":row.killer_steam_id,"name":"","faction":row.killer_faction},"victim":{"steamId":row.victim_steam_id,"name":"","faction":row.victim_faction},"cause":row.cause,"distanceM":row.distance_m,"headshot":row.headshot,"tags":if row.penetration{vec!["Penetration"]}else{vec![]},"suicide":false,"teamKill":false});
        let output = integrity_windows::generate(
            &mut self.windows,
            &format!("{}:{}", row.source, row.server_id),
            &[event],
            &self.overrides,
            &self.rules,
        );
        let Some(feature) = output.features.first() else {
            return;
        };
        let mut add = |metric: &str, value: Option<f64>, weapon: &str| {
            let Some(value) = value.filter(|v| v.is_finite()) else {
                return;
            };
            self.daily.add(
                Sample {
                    player: row.killer_steam_id.clone(),
                    day: row.at.format("%Y-%m-%d").to_string(),
                    source: row.source.clone(),
                    server_id: row.server_id.clone(),
                    round_id: feature["roundId"].as_str().unwrap().into(),
                    at: row.at,
                    map: feature["map"].as_str().unwrap().into(),
                    bucket: integrity_statistics::population_bucket(row.player_count)
                        .map(str::to_owned),
                    metric: metric.into(),
                    weapon: weapon.into(),
                    value,
                    event_id: row.event_id.clone(),
                },
                true,
                20,
            );
        };
        for (metric, field) in [
            ("kpm180", "kpm180"),
            ("uniqueVictims", "uniqueVictims180"),
            ("maxKills15s", "maxKills15s"),
            ("medianKillInterval", "medianKillInterval"),
            ("headshotRate", "headshotRate"),
            ("penetrationRate", "penetrationRate"),
        ] {
            add(metric, feature[field].as_f64(), "INFANTRY");
        }
        for weapon in feature["weaponMetrics"].as_array().into_iter().flatten() {
            let kills = integrity_score::num(weapon, "kills");
            let cause = weapon["cause"].as_str().unwrap();
            if kills >= 10. {
                add(
                    "headshotRateWeapon",
                    Some(integrity_score::num(weapon, "headshots") / kills),
                    cause,
                );
            }
            if kills >= 3. {
                add(
                    "maxKillDistanceWeapon",
                    weapon["maxKillDistanceM"].as_f64(),
                    cause,
                );
            }
        }
    }
    pub fn finish(self) -> Vec<Sample> {
        let mut players = Groups::default();
        for sample in self.daily.rows.into_iter().flatten() {
            players.add(sample, false, 100);
        }
        players.rows.into_iter().flatten().collect()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cohort {
    pub server_id: Option<String>,
    pub source: String,
    pub level: i32,
    pub map: Option<String>,
    pub bucket: Option<String>,
    pub metric: String,
    pub weapon: String,
    pub samples: Vec<Sample>,
}
pub fn cohorts(samples: &[Sample]) -> Vec<Cohort> {
    let mut indices = HashMap::new();
    let mut result: Vec<Cohort> = Vec::new();
    for sample in samples {
        let server = if sample.source == "local"
            && ["headshotRate", "penetrationRate", "headshotRateWeapon"]
                .contains(&sample.metric.as_str())
        {
            Some(sample.server_id.clone())
        } else {
            None
        };
        let mut levels = vec![(3, None, None)];
        if let Some(bucket) = &sample.bucket {
            levels = vec![
                (1, Some(sample.map.clone()), Some(bucket.clone())),
                (2, None, Some(bucket.clone())),
                (3, None, None),
            ];
        }
        for (level, map, bucket) in levels {
            let key = serde_json::to_string(&json!([
                server,
                sample.source,
                level,
                map,
                bucket,
                sample.metric,
                sample.weapon
            ]))
            .unwrap();
            let index = *indices.entry(key).or_insert_with(|| {
                result.push(Cohort {
                    server_id: server.clone(),
                    source: sample.source.clone(),
                    level,
                    map: map.clone(),
                    bucket: bucket.clone(),
                    metric: sample.metric.clone(),
                    weapon: sample.weapon.clone(),
                    samples: Vec::new(),
                });
                result.len() - 1
            });
            result[index].samples.push(sample.clone());
        }
    }
    result
}
fn weighted_quantile(rows: &[(f64, f64)], percentile: f64) -> f64 {
    let target = rows.iter().map(|r| r.1).sum::<f64>() * percentile;
    let mut cumulative = 0.;
    for &(value, weight) in rows {
        cumulative += weight;
        if cumulative >= target {
            return value;
        }
    }
    rows.last().map(|r| r.0).unwrap_or(0.)
}
pub fn summarize(group: &Cohort) -> Value {
    let mut players: HashMap<&str, usize> = HashMap::new();
    for s in &group.samples {
        *players.entry(&s.player).or_default() += 1;
    }
    let mut rows: Vec<_> = group
        .samples
        .iter()
        .map(|s| (s.value, 1. / players[s.player.as_str()] as f64))
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let median = weighted_quantile(&rows, 0.5);
    let mut deviations: Vec<_> = rows.iter().map(|&(v, w)| ((v - median).abs(), w)).collect();
    deviations.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total = rows.iter().map(|r| r.1).sum::<f64>();
    let squared = rows.iter().map(|r| r.1 * r.1).sum::<f64>();
    let (min, max) = (
        rows.first().map(|r| r.0).unwrap_or(0.),
        rows.last().map(|r| r.0).unwrap_or(0.),
    );
    let mut bins:Vec<_>=(0..30).map(|i|json!({"from":min+(max-min)*i as f64/30.,"to":min+(max-min)*(i+1) as f64/30.,"count":0.})).collect();
    let mut cdf: Vec<(f64, f64)> = Vec::new();
    for &(v, w) in &rows {
        if let Some(last) = cdf.last_mut().filter(|r| r.0 == v) {
            last.1 += w
        } else {
            cdf.push((v, w))
        }
        let bin = if max == min {
            0
        } else {
            ((30. * (v - min) / (max - min)).floor() as usize).min(29)
        };
        bins[bin]["count"] = json!(bins[bin]["count"].as_f64().unwrap() + w);
    }
    let days = group
        .samples
        .iter()
        .map(|s| (&s.player, &s.day))
        .collect::<HashSet<_>>()
        .len();
    json!({"sampleCount":group.samples.len(),"uniquePlayers":players.len(),"uniquePlayerDays":days,"effectiveSampleSize":if squared>0.{total*total/squared}else{0.},"median":median,"mad":weighted_quantile(&deviations,0.5),"p90":weighted_quantile(&rows,0.9),"p95":weighted_quantile(&rows,0.95),"p99":weighted_quantile(&rows,0.99),"p995":weighted_quantile(&rows,0.995),"p999":weighted_quantile(&rows,0.999),"p9995":weighted_quantile(&rows,0.9995),"cdf":cdf,"histogram":bins})
}
pub fn histories(samples: &[Sample], org: &str) -> Vec<Value> {
    let mut indices = HashMap::new();
    let mut groups: Vec<HashMap<&str, &Sample>> = Vec::new();
    for sample in samples.iter().filter(|s| s.source == "local") {
        let key = format!("{}:{}", sample.server_id, sample.event_id);
        let index = *indices.entry(key).or_insert_with(|| {
            groups.push(HashMap::new());
            groups.len() - 1
        });
        groups[index].insert(&sample.metric, sample);
    }
    groups.into_iter().filter_map(|metrics| {
        let kpm=metrics.get("kpm180")?;let burst=metrics.get("maxKills15s")?;
        Some(json!({"org_id":org,"steam_id":kpm.player,"server_id":kpm.server_id,"round_id":kpm.round_id,"event_id":kpm.event_id,"observed_at":kpm.at,"kpm_180":kpm.value,"headshot_rate":metrics.get("headshotRate").map(|s|s.value),"max_kills_15s":burst.value.round() as i32,"feature_version":FEATURE_VERSION,"model_version":MODEL_VERSION}))
    }).collect()
}
