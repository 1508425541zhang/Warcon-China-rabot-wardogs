//! Frozen expanded30m feature contract. Missing values remain NaN until normalization;
//! neither a missing roster nor an absent feed is interpreted as zero activity.
use crate::model::Contract;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

fn num(v: &Value) -> f64 {
    v.as_f64().filter(|x| x.is_finite()).unwrap_or(f64::NAN)
}
fn text(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}
fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn strings(v: &Value) -> Vec<String> {
    array(v).iter().map(text).collect()
}
fn time(v: &Value) -> f64 {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp_millis() as f64 / 1000.)
        .unwrap_or(f64::NAN)
}
fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}
fn quantile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let p = (s.len() - 1) as f64 * q;
    s[p.floor() as usize] + (s[p.ceil() as usize] - s[p.floor() as usize]) * p.fract()
}
fn flatten(v: &Value, p: &str, out: &mut HashMap<String, f64>) {
    if let Some(o) = v.as_object() {
        for (k, x) in o {
            let n = if p.is_empty() {
                k.to_owned()
            } else {
                format!("{p}.{k}")
            };
            if num(x).is_finite() {
                out.insert(n, num(x));
            } else if x.is_object() {
                flatten(x, &n, out)
            }
        }
    }
}
fn boolnum(v: bool) -> f64 {
    if v { 1. } else { 0. }
}
fn get(out: &HashMap<String, f64>, n: &str) -> f64 {
    out.get(n).copied().unwrap_or(f64::NAN)
}
const ARMS: &[&str] = &[
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
const CLASSES: &[&str] = &[
    "automatic",
    "sniper",
    "shotgun",
    "other_infantry",
    "vehicle",
    "fixed_weapon",
    "unknown",
];
struct Event {
    data: Value,
    received: f64,
    clock: f64,
    tags: Option<Vec<String>>,
    headshot: Option<bool>,
    raw: bool,
}
impl Event {
    fn is_attack(&self, player: &str) -> bool {
        self.data["killer_steam_id"] == player
            && self.data["victim_steam_id"] != player
            && self.data["suicide"] != true
            && self.data["team_kill"] != true
            && !["Suicide", "Falling", "RoadKill"]
                .iter()
                .any(|t| self.has_tag(t))
    }
    fn has_tag(&self, t: &str) -> bool {
        self.tags.as_ref().is_some_and(|a| a.iter().any(|x| x == t))
    }
    fn arm(&self) -> bool {
        self.data["cause"]
            .as_str()
            .is_some_and(|w| ARMS.iter().any(|x| w == format!("Id.Item.{x}")))
    }
    fn distance(&self) -> Option<f64> {
        let d = num(&self.data["distance_m"]);
        (d.is_finite() && (0. ..=2000.).contains(&d) && self.data["distance_invalid"] != true)
            .then_some(d)
    }
}
struct Poll {
    at: f64,
    players: Vec<Value>,
    size: f64,
}

pub fn normalize(rows: &[Vec<f32>], contract: &Contract) -> Vec<f32> {
    let f = contract.features.len();
    let mut input = vec![0.; rows.len() * f * 2];
    for (t, row) in rows.iter().enumerate() {
        for (i, v) in row.iter().enumerate() {
            if v.is_finite() {
                input[t * f * 2 + i] = ((*v as f64 - contract.center[i]) / contract.scale[i])
                    .clamp(-100., 100.) as f32;
                input[t * f * 2 + f + i] = 1.;
            }
        }
    }
    input
}

pub fn build(source: &Value, contract: &Contract) -> Vec<Vec<f32>> {
    let match_ = &source["match"];
    let player = text(&source["player"]);
    let start = time(&match_["started_at"]);
    let end = (time(&source["end"]) / 30.).floor() * 30.;
    if !start.is_finite() || !end.is_finite() || end - start < 1800. {
        return vec![];
    }
    let mut raw = HashMap::new();
    let mut feed = Vec::new();
    for b in array(&source["batches"]) {
        feed.push(time(&b["received_at"]));
        for e in array(&b["payload"]["events"]) {
            raw.insert(
                format!("{}/{}", text(&b["instance_id"]), text(&e["eventId"])),
                e,
            );
        }
    }
    feed.sort_by(f64::total_cmp);
    let mut seen = HashSet::new();
    let mut events = Vec::new();
    for e in array(&source["kills"]) {
        let key = format!("{}/{}", text(&e["instance_id"]), text(&e["event_id"]));
        let clock = num(&e["event_time"]);
        if seen.contains(&key)
            || !clock.is_finite()
            || clock < 0.
            || text(&e["match_row"]) != text(&match_["id"])
        {
            continue;
        }
        seen.insert(key.clone());
        let r = raw.get(&key).copied();
        let tags = r
            .filter(|v| v["contextTags"].is_array())
            .map(|v| {
                strings(&v["contextTags"])
                    .into_iter()
                    .map(|s| s.rsplit('.').next().unwrap_or("").to_owned())
                    .collect()
            })
            .or_else(|| e["tags"].as_array().map(|_| strings(&e["tags"])));
        let headshot = if let Some(r) = r {
            r["contextTags"].as_array().map(|a| {
                a.iter()
                    .any(|s| s.as_str().is_some_and(|s| s.ends_with(".Headshot")))
            })
        } else {
            e["headshot"].as_bool()
        };
        events.push(Event {
            data: e.clone(),
            received: time(&e["ts"]),
            clock,
            tags,
            headshot,
            raw: r.is_some(),
        });
    }
    events.sort_by(|a, b| a.received.total_cmp(&b.received));
    for key in ["instance_id", "map"] {
        if events
            .iter()
            .map(|e| text(&e.data[key]))
            .collect::<HashSet<_>>()
            .len()
            > 1
        {
            return vec![];
        }
    }
    let mut polls = Vec::new();
    let mut statuses = Vec::new();
    for o in array(&source["observations"]) {
        let at = time(&o["received_at"]);
        if o["endpoint"] == "/v1/players" {
            let p = &o["payload"];
            polls.push(Poll {
                at,
                players: array(if p.is_array() { p } else { &p["players"] }).to_vec(),
                size: num(&p["roster_size"]),
            });
        } else if o["endpoint"] == "/v1/status" {
            statuses.push((at, o["payload"].clone()));
        }
    }
    for p in array(&source["progress"]) {
        polls.push(Poll {
            at: time(&p["observed_at"]),
            players: array(&p["players"]).to_vec(),
            size: num(&p["roster_size"]),
        });
    }
    polls.sort_by(|a, b| a.at.total_cmp(&b.at));
    statuses.sort_by(|a, b| a.0.total_cmp(&b.0));
    let names = strings(&contract.base["features"]);
    let weapons = strings(&contract.base["weapon_vocabulary"]);
    let weapon_set: HashSet<_> = weapons.iter().cloned().collect();
    let numeric: Vec<_> = names
        .iter()
        .filter(|n| n.starts_with("player_") && !n.starts_with("player_delta_"))
        .map(|n| n[7..].to_owned())
        .collect();
    let (mut pi, mut ei, mut si, mut fi) = (0, 0, 0, 0);
    let mut anchor: Option<&Event> = None;
    let mut state = Value::Null;
    let (mut state_at, mut obs_at) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut active_count = 0.;
    let mut fields: HashMap<String, (f64, Value)> = HashMap::new();
    let mut previous: HashMap<String, (f64, f64)> = HashMap::new();
    let mut output = Vec::new();
    for step in 0..=61 {
        let tick = end - 1830. + step as f64 * 30.;
        while pi < polls.len() && polls[pi].at <= tick {
            let p = &polls[pi];
            pi += 1;
            if p.players
                .iter()
                .any(|v| v.get("kills").is_some() || v.get("pingMs").is_some())
            {
                active_count = p
                    .players
                    .iter()
                    .map(|v| text(&v["steamId"]))
                    .collect::<HashSet<_>>()
                    .len() as f64;
            }
            if p.size.is_finite() {
                active_count = p.size;
            }
            for v in &p.players {
                if text(&v["steamId"]) == player {
                    if let Some(o) = v.as_object() {
                        for (k, x) in o {
                            fields.insert(k.clone(), (p.at, x.clone()));
                        }
                    }
                    obs_at = obs_at.max(p.at);
                }
            }
        }
        while ei < events.len() && events[ei].received <= tick {
            let e = &events[ei];
            ei += 1;
            if anchor.is_none_or(|a| e.clock >= a.clock) {
                anchor = Some(e)
            }
        }
        while si < statuses.len() && statuses[si].0 <= tick {
            state = statuses[si].1.clone();
            state_at = statuses[si].0;
            si += 1;
        }
        while fi < feed.len() && feed[fi] <= tick {
            fi += 1;
        }
        let feed_age = if fi > 0 {
            tick - feed[fi - 1]
        } else {
            f64::INFINITY
        };
        let clock = anchor
            .filter(|a| tick - a.received <= 120.)
            .map(|a| a.clock + tick - a.received)
            .unwrap_or(f64::NAN);
        let recent: Vec<_> = events[..ei]
            .iter()
            .filter(|e| {
                tick - e.received <= 600.
                    && (e.data["killer_steam_id"] == player || e.data["victim_steam_id"] == player)
            })
            .collect();
        let window: Vec<_> = recent
            .iter()
            .copied()
            .filter(|e| clock.is_finite() && clock - 120. < e.clock && e.clock <= clock)
            .collect();
        let known = feed_age <= 45. || window.iter().any(|e| tick - e.received < 30.);
        let valid_core = clock.is_finite() && known;
        let attacks: Vec<_> = window
            .iter()
            .copied()
            .filter(|e| e.is_attack(&player))
            .collect();
        let mut out = HashMap::new();
        let mut rates = Vec::new();
        for w in [60, 120] {
            let es: Vec<_> = window
                .iter()
                .copied()
                .filter(|e| e.clock > clock - w as f64)
                .collect();
            let ak: Vec<_> = es
                .iter()
                .copied()
                .filter(|e| e.is_attack(&player))
                .collect();
            let mut ar: Vec<_> = ak.iter().copied().filter(|e| e.arm()).collect();
            ar.sort_by(|a, b| {
                a.clock
                    .total_cmp(&b.clock)
                    .then_with(|| text(&a.data["event_id"]).cmp(&text(&b.data["event_id"])))
            });
            let h: Vec<_> = ar
                .iter()
                .filter(|e| e.tags.is_some())
                .filter_map(|e| e.headshot.map(boolnum))
                .collect();
            let p: Vec<_> = ar
                .iter()
                .filter_map(|e| {
                    e.tags.as_ref().map(|t| {
                        boolnum(
                            e.data["penetration"]
                                .as_bool()
                                .unwrap_or_else(|| t.iter().any(|s| s.contains("Penetration"))),
                        )
                    })
                })
                .collect();
            let intervals: Vec<_> = ar.windows(2).map(|p| p[1].clock - p[0].clock).collect();
            let (mut burst, mut left) = (0, 0);
            for right in 0..ar.len() {
                while ar[right].clock - ar[left].clock > 15. {
                    left += 1;
                }
                burst = burst.max(right - left + 1);
            }
            let ds: Vec<_> = ar
                .iter()
                .filter_map(|e| {
                    let b = &contract.base["distance_baseline"][text(&e.data["cause"])];
                    if b.is_null() {
                        None
                    } else {
                        e.distance().map(|d| d / num(&b["p95_m"]))
                    }
                })
                .collect();
            let vals = [
                ar.len() as f64,
                ak.len() as f64,
                es.iter()
                    .filter(|e| e.data["victim_steam_id"] == player)
                    .count() as f64,
                mean(&h),
                mean(&p),
                ar.iter()
                    .map(|e| text(&e.data["victim_steam_id"]))
                    .collect::<HashSet<_>>()
                    .len() as f64,
                burst as f64,
                quantile(&intervals, 0.5),
                quantile(&ds, 0.9),
                h.len() as f64,
                ds.len() as f64,
            ];
            for (n, v) in [
                "small_arm_kills",
                "kills",
                "deaths",
                "headshot_rate",
                "penetration_rate",
                "unique_victims",
                "max_kills_15s",
                "median_interval_s",
                "distance_ratio_p90",
                "headshot_samples",
                "distance_samples",
            ]
            .iter()
            .zip(vals)
            {
                out.insert(format!("{n}_{w}s"), if valid_core { v } else { f64::NAN });
            }
            rates.push(ar.len() as f64);
        }
        out.insert(
            "kill_rate_change".into(),
            if valid_core {
                rates[0] - (rates[1] - rates[0])
            } else {
                f64::NAN
            },
        );
        let mut wc: HashMap<String, usize> = HashMap::new();
        for e in &attacks {
            if e.arm() {
                *wc.entry(text(&e.data["cause"])).or_default() += 1;
            }
        }
        let arms_count = wc.values().sum::<usize>();
        out.insert(
            "dominant_weapon_fraction_120s".into(),
            if valid_core && arms_count > 0 {
                *wc.values().max().unwrap() as f64 / arms_count as f64
            } else {
                f64::NAN
            },
        );
        let ds: Vec<_> = attacks.iter().filter_map(|e| e.distance()).collect();
        let heads: Vec<_> = attacks
            .iter()
            .filter(|e| e.tags.is_some())
            .filter_map(|e| e.headshot.map(boolnum))
            .collect();
        for (n, v) in [
            ("distance_mean_m_120s", mean(&ds)),
            (
                "distance_max_m_120s",
                ds.iter().copied().reduce(f64::max).unwrap_or(f64::NAN),
            ),
            ("all_headshot_rate_120s", mean(&heads)),
            (
                "all_headshot_samples_120s",
                if known { heads.len() as f64 } else { f64::NAN },
            ),
            (
                "raw_event_fraction_120s",
                mean(&window.iter().map(|e| boolnum(e.raw)).collect::<Vec<_>>()),
            ),
            (
                "suicides_120s",
                if known {
                    window
                        .iter()
                        .filter(|e| {
                            e.data["suicide"] == true && e.data["victim_steam_id"] == player
                        })
                        .count() as f64
                } else {
                    f64::NAN
                },
            ),
            (
                "teamkills_120s",
                if known {
                    window
                        .iter()
                        .filter(|e| {
                            e.data["team_kill"] == true && e.data["killer_steam_id"] == player
                        })
                        .count() as f64
                } else {
                    f64::NAN
                },
            ),
            (
                "kill_clock_spread_120s",
                if attacks.is_empty() {
                    f64::NAN
                } else {
                    attacks.iter().map(|e| e.clock).reduce(f64::max).unwrap()
                        - attacks.iter().map(|e| e.clock).reduce(f64::min).unwrap()
                },
            ),
        ] {
            out.insert(n.into(), v);
        }
        for k in &numeric {
            let v = fields
                .get(k)
                .filter(|(at, _)| tick - at <= 45.)
                .map(|(_, v)| num(v))
                .unwrap_or(f64::NAN);
            out.insert(format!("player_{k}"), v);
            let d = previous
                .get(k)
                .filter(|(at, p)| *at == tick - 30. && v.is_finite() && p.is_finite())
                .map(|(_, p)| v - p)
                .unwrap_or(f64::NAN);
            out.insert(format!("player_delta_{k}"), d);
            previous.insert(k.clone(), (tick, v));
        }
        let faction = fields
            .get("faction")
            .filter(|(at, _)| tick - at <= 45.)
            .map(|(_, v)| text(v))
            .unwrap_or_default();
        let status = if tick - state_at <= 45. {
            &state
        } else {
            &Value::Null
        };
        let scores_src = status
            .get("factionScores")
            .filter(|v| !v.is_null())
            .unwrap_or(&status["scores"]);
        let mut scores = HashMap::new();
        for s in array(scores_src) {
            if num(&s["score"]).is_finite() {
                scores.insert(text(&s["name"]), num(&s["score"]));
            }
        }
        let leading = scores
            .values()
            .copied()
            .reduce(f64::max)
            .unwrap_or(f64::NAN);
        let own = scores.get(&faction).copied().unwrap_or(f64::NAN);
        for (n, v) in [
            (
                "roster_size",
                if tick - obs_at <= 45. {
                    active_count
                } else {
                    f64::NAN
                },
            ),
            ("own_faction_score", own),
            ("leading_faction_score", leading),
            ("faction_score_gap", leading - own),
            ("round_elapsed_s", tick - start),
            (
                "feed_age_s",
                if feed_age.is_finite() {
                    feed_age
                } else {
                    f64::NAN
                },
            ),
            (
                "roster_age_s",
                if obs_at.is_finite() {
                    tick - obs_at
                } else {
                    f64::NAN
                },
            ),
            (
                "utc_hour_sin",
                ((tick % 86400.) / 86400. * 2. * std::f64::consts::PI).sin(),
            ),
            (
                "utc_hour_cos",
                ((tick % 86400.) / 86400. * 2. * std::f64::consts::PI).cos(),
            ),
        ] {
            out.insert(n.into(), v);
        }
        let mut flat = HashMap::new();
        flatten(status, "", &mut flat);
        for n in &names {
            if let Some(k) = n.strip_prefix("status:") {
                out.insert(n.clone(), get(&flat, k));
            }
        }
        let mut per: HashMap<String, Vec<&Event>> = HashMap::new();
        for e in &attacks {
            let cause = text(&e.data["cause"]);
            let w = if weapon_set.contains(&cause) {
                cause
            } else {
                "UNKNOWN".into()
            };
            per.entry(w).or_default().push(e);
        }
        for w in &weapons {
            let es = per.get(w).map(Vec::as_slice).unwrap_or(&[]);
            let h: Vec<_> = es
                .iter()
                .filter(|e| e.tags.is_some())
                .filter_map(|e| e.headshot.map(boolnum))
                .collect();
            let b = &contract.distance_baseline[w];
            let d: Vec<_> = es
                .iter()
                .filter_map(|e| {
                    if b.is_null() {
                        None
                    } else {
                        e.distance().map(|d| d / num(&b["p95_m"]))
                    }
                })
                .collect();
            for (n, v) in [
                ("kills_120s", if known { es.len() as f64 } else { f64::NAN }),
                (
                    "headshots_120s",
                    if h.is_empty() {
                        f64::NAN
                    } else {
                        h.iter().sum()
                    },
                ),
                ("headshot_rate_120s", mean(&h)),
                ("mean_distance_ratio_120s", mean(&d)),
            ] {
                out.insert(format!("weapon:{w}:{n}"), v);
            }
        }
        for n in &names {
            if let Some(tag) = n.strip_prefix("tag_120s:") {
                out.insert(
                    n.clone(),
                    if known {
                        attacks.iter().filter(|e| e.has_tag(tag)).count() as f64
                    } else {
                        f64::NAN
                    },
                );
            }
            for prefix in ["faction", "map", "lighting"] {
                if let Some(category) = n.strip_prefix(&format!("{prefix}:")) {
                    let value = match prefix {
                        "faction" => faction.clone(),
                        "map" => text(&match_["map"]),
                        _ => text(&status["lighting"]),
                    };
                    let known = names.iter().any(|x| x == &format!("{prefix}:{value}"));
                    out.insert(
                        n.clone(),
                        boolnum(category == if known { &value } else { "UNKNOWN" }),
                    );
                }
            }
            if let Some(exp) = n.strip_prefix("experience:") {
                out.insert(
                    n.clone(),
                    boolnum(array(&status["experiences"]).iter().any(|x| x == exp)),
                );
            }
        }
        for v in out.values_mut() {
            *v = (*v as f32) as f64;
        }
        for class in CLASSES {
            let ws: Vec<_> = weapons
                .iter()
                .filter(|w| contract.weapon_class_by_id[*w] == *class)
                .collect();
            let any = ws
                .iter()
                .any(|w| get(&out, &format!("weapon:{w}:kills_120s")).is_finite());
            let (mut count, mut h, mut hn) = (0., 0., 0.);
            let mut d = Vec::new();
            for w in ws {
                let k = get(&out, &format!("weapon:{w}:kills_120s"));
                let v = get(&out, &format!("weapon:{w}:headshots_120s"));
                let dist = get(&out, &format!("weapon:{w}:mean_distance_ratio_120s"));
                if k.is_finite() {
                    count += k;
                }
                if v.is_finite() {
                    h += v;
                    if k.is_finite() {
                        hn += k;
                    }
                }
                if dist.is_finite() {
                    d.push(dist);
                }
            }
            for (n, v) in [
                ("kills_120s", if any { count } else { f64::NAN }),
                ("headshots_observed_120s", if any { h } else { f64::NAN }),
                ("headshot_samples_120s", if any { hn } else { f64::NAN }),
                (
                    "headshot_rate_120s",
                    if hn > 0. { h / hn } else { f64::NAN },
                ),
                (
                    "max_weapon_mean_distance_ratio_120s",
                    d.iter().copied().reduce(f64::max).unwrap_or(f64::NAN),
                ),
            ] {
                out.insert(format!("class:{class}:{n}"), v);
            }
        }
        if tick > end - 1800. {
            let current = tick - obs_at <= 45. || recent.iter().any(|e| tick - e.received <= 120.);
            output.push(
                contract
                    .features
                    .iter()
                    .map(|n| {
                        if current {
                            get(&out, n) as f32
                        } else {
                            f32::NAN
                        }
                    })
                    .collect(),
            );
        }
    }
    output
}
