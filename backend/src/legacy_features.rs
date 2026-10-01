//! Original four-table, one-player 27-channel reconstruction. No expanded-model substitutions.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
pub const MASKS: [&str; 13] = [
    "cash_observed",
    "cash_change_valid",
    "combat_change_valid",
    "history_kpm_observed",
    "window_kpm_observed",
    "headshot_rate_observed",
    "penetration_rate_observed",
    "max15_observed",
    "unique_victims_observed",
    "burst_observed",
    "interval_observed",
    "roster_observed",
    "active_observed",
];
pub const MASK_FOR: [usize; 14] = [0, 1, 2, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
const TABLES: [&str; 4] = [
    "matches",
    "player_progress_samples",
    "integrity_player_metric_history",
    "integrity_windows",
];
#[derive(Clone)]
struct Sample {
    elapsed: f64,
    cash: Option<f64>,
    kills: Option<f64>,
    deaths: Option<f64>,
    roster: Option<f64>,
}
#[derive(Clone)]
struct Metric {
    elapsed: f64,
    data: HashMap<&'static str, Option<f64>>,
}
#[derive(Default)]
struct Group {
    progress: Vec<Sample>,
    history: Vec<Metric>,
    windows: Vec<Metric>,
}
pub fn numeric(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1. } else { 0. }),
        Value::String(s) if !s.is_empty() => {
            let s = s.trim();
            if s.is_empty() {
                Some(0.)
            } else if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(h, 16).ok().map(|v| v as f64)
            } else {
                s.parse::<f64>().ok()
            }
        }
        _ => None,
    };
    n.filter(|n| n.is_finite())
}
fn object(v: &Value) -> Result<&serde_json::Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| anyhow::anyhow!("Object required"))
}
pub fn timestamp(v: &Value) -> Result<Option<i64>> {
    if v.is_null() || v == "" {
        return Ok(None);
    }
    let s = v
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("UTC timestamp required"))?;
    Ok(Some(
        chrono::DateTime::parse_from_rfc3339(s)
            .map_err(|_| anyhow::anyhow!("UTC timestamp required"))?
            .timestamp_millis(),
    ))
}
fn string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        _ => v.to_string(),
    }
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn default_string(v: &Value) -> String {
    if truthy(v) { string(v) } else { String::new() }
}
fn iso(ms: i64) -> String {
    let t = chrono::DateTime::from_timestamp_millis(ms).unwrap();
    if ms.rem_euclid(1000) == 0 {
        t.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        t.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
    }
}
fn mean(a: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let a = a.flatten().collect::<Vec<_>>();
    (!a.is_empty()).then(|| a.iter().sum::<f64>() / a.len() as f64)
}
pub fn build(request: &Value) -> Result<Vec<Value>> {
    object(request)?;
    ensure!(
        request["schema"] == "warcon-raw-30s-v1",
        "Feature schema mismatch"
    );
    let source = object(&request["sources"])?;
    ensure!(
        source.len() == TABLES.len() && TABLES.iter().all(|n| source.contains_key(*n)),
        "Invalid source tables"
    );
    for table in TABLES {
        ensure!(
            source[table].as_array().is_some_and(|a| a.len() <= 25000),
            "Bounded source arrays required"
        )
    }
    let matches = source["matches"].as_array().unwrap();
    ensure!(matches.len() == 1, "One match required");
    let m = &matches[0];
    object(m)?;
    let Some(start) = timestamp(&m["started_at"])? else {
        return Ok(vec![]);
    };
    let end = timestamp(&m["ended_at"])?;
    if end.is_some_and(|e| e <= start) {
        return Ok(vec![]);
    }
    let mid = string(&m["id"]);
    let duration = end
        .map(|e| (e - start) as f64 / 1000.)
        .unwrap_or(f64::INFINITY);
    let elapsed = |v: &Value| -> Result<Option<f64>> {
        Ok(timestamp(v)?
            .map(|t| (t - start) as f64 / 1000.)
            .filter(|v| *v >= -5. && *v < duration)
            .map(|v| v.max(0.)))
    };
    let mut groups: HashMap<(String, String), Group> = HashMap::new();
    for row in source["player_progress_samples"].as_array().unwrap() {
        object(row)?;
        let Some(at) = elapsed(&row["observed_at"])? else {
            continue;
        };
        if default_string(&row["match_id"]) != mid {
            continue;
        }
        let players = row["players"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for p in players {
            if !p.is_object() || !truthy(&p["steamId"]) {
                continue;
            }
            let roster = row
                .get("roster_size")
                .map(numeric)
                .unwrap_or(Some(players.len() as f64));
            groups
                .entry((default_string(&row["server_id"]), string(&p["steamId"])))
                .or_default()
                .progress
                .push(Sample {
                    elapsed: at,
                    cash: numeric(&p["cash"]),
                    kills: numeric(&p["kills"]).map(f64::trunc),
                    deaths: numeric(&p["deaths"]).map(f64::trunc),
                    roster,
                });
        }
    }
    for table in [&TABLES[2], &TABLES[3]] {
        for row in source[*table].as_array().unwrap() {
            object(row)?;
            let Some(at) = elapsed(&row["observed_at"])? else {
                continue;
            };
            let round = default_string(&row["round_id"]);
            let valid_round = round.rsplit_once(":match:").is_some_and(|(_, s)| {
                !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s == mid
            });
            if !valid_round || !truthy(&row["steam_id"]) {
                continue;
            }
            let g = groups
                .entry((default_string(&row["server_id"]), string(&row["steam_id"])))
                .or_default();
            let mut data = HashMap::from([("kpm", numeric(&row["kpm_180"]))]);
            if *table == TABLES[2] {
                g.history.push(Metric { elapsed: at, data })
            } else {
                for (k, n) in [
                    ("inf", "infantry_kills"),
                    ("victims", "unique_victims"),
                    ("head", "headshots"),
                    ("pen", "penetrations"),
                    ("burst", "burst_points"),
                    ("max15", "max_kills_15s"),
                    ("interval", "median_kill_interval"),
                ] {
                    data.insert(k, numeric(&row[n]));
                }
                g.windows.push(Metric { elapsed: at, data });
            }
        }
    }
    let mut results = Vec::new();
    for mut g in groups.into_values() {
        g.progress.sort_by(|a, b| a.elapsed.total_cmp(&b.elapsed));
        let mut spans = Vec::new();
        for (i, p) in g.progress.iter().enumerate() {
            let next = g
                .progress
                .get(i + 1)
                .map(|p| p.elapsed)
                .unwrap_or(p.elapsed + 30.);
            let right = if next - p.elapsed > 0. && next - p.elapsed <= 90. {
                next
            } else {
                p.elapsed + 30.
            };
            spans.push((p.elapsed, right.min(duration).max(p.elapsed)));
        }
        for m in g.history.iter().chain(&g.windows) {
            spans.push((m.elapsed, m.elapsed + 1.))
        }
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (a, b) in spans {
            if let Some(last) = merged.last_mut().filter(|r| a <= r.1) {
                last.1 = last.1.max(b)
            } else {
                merged.push((a, b));
            }
        }
        if merged.is_empty() {
            continue;
        }
        let hi = merged
            .iter()
            .map(|(a, b)| (a.max(b - 1e-9) / 30.).floor())
            .fold(f64::NEG_INFINITY, f64::max)
            .min((duration / 30.).floor() - 1.) as i64;
        let lo = (merged
            .iter()
            .map(|(a, _)| (a / 30.).floor() as i64)
            .min()
            .unwrap())
        .max(hi - 200)
        .max(0);
        let mut ps = BTreeMap::<i64, Vec<Sample>>::new();
        let mut hs = BTreeMap::<i64, Vec<Metric>>::new();
        let mut ws = BTreeMap::<i64, Vec<Metric>>::new();
        for p in g.progress {
            ps.entry((p.elapsed / 30.).floor() as i64)
                .or_default()
                .push(p)
        }
        for p in g.history {
            hs.entry((p.elapsed / 30.).floor() as i64)
                .or_default()
                .push(p)
        }
        for p in g.windows {
            ws.entry((p.elapsed / 30.).floor() as i64)
                .or_default()
                .push(p)
        }
        let mut rows = Vec::new();
        let mut previous: Option<&Sample> = None;
        for bucket in lo..=hi {
            let pr = ps.get(&bucket).map(Vec::as_slice).unwrap_or(&[]);
            let h = hs.get(&bucket).map(Vec::as_slice).unwrap_or(&[]);
            let w = ws.get(&bucket).map(Vec::as_slice).unwrap_or(&[]);
            let p = pr.last();
            let left = (bucket * 30) as f64;
            let right = left + 30.;
            let mut active = merged
                .iter()
                .map(|(a, b)| (right.min(*b).min(duration) - left.max(*a)).max(0.))
                .sum::<f64>()
                .min(30.);
            let observed = !pr.is_empty() || !h.is_empty() || !w.is_empty();
            if observed && active <= 0. {
                active = 1.
            }
            let mut r = json!({"bucket_start_utc":iso(start+bucket*30000),"is_active":i32::from(active>0.||observed),"bucket_observed":i32::from(observed),"cash_balance":p.and_then(|p|p.cash),"cash_observed":i32::from(p.is_some_and(|p|p.cash.is_some())),"roster_size":p.and_then(|p|p.roster),"roster_observed":i32::from(p.is_some_and(|p|p.roster.is_some())),"active_fraction":(active/30.*1e6).round()/1e6,"active_observed":1,"cash_change_30s":null,"cash_change_valid":0,"kills_change_30s":null,"deaths_change_30s":null,"combat_change_valid":0});
            if let (Some(p), Some(old)) = (p, previous) {
                if p.elapsed - old.elapsed >= 0. && p.elapsed - old.elapsed <= 45. {
                    if let (Some(a), Some(b)) = (p.cash, old.cash) {
                        r["cash_change_30s"] = json!(a - b);
                        r["cash_change_valid"] = json!(1)
                    }
                    if let (Some(k), Some(d), Some(ok), Some(od)) =
                        (p.kills, p.deaths, old.kills, old.deaths)
                    {
                        if k >= ok && d >= od {
                            r["kills_change_30s"] = json!(k - ok);
                            r["deaths_change_30s"] = json!(d - od);
                            r["combat_change_valid"] = json!(1)
                        }
                    }
                }
            }
            for (ch, mask, items) in [
                ("kpm_180_history_mean", "history_kpm_observed", h),
                ("kpm_180_window_mean", "window_kpm_observed", w),
            ] {
                let n = mean(items.iter().map(|s| s.data["kpm"]));
                r[ch] = json!(n);
                r[mask] = json!(i32::from(n.is_some()));
            }
            let inf = w
                .iter()
                .filter_map(|s| s.data["inf"])
                .filter(|n| *n > 0.)
                .collect::<Vec<_>>();
            for (ch, field, mask) in [
                ("headshot_rate_window", "head", "headshot_rate_observed"),
                (
                    "penetration_rate_window",
                    "pen",
                    "penetration_rate_observed",
                ),
            ] {
                let values = w.iter().filter_map(|s| s.data[field]).collect::<Vec<_>>();
                let rate = (!inf.is_empty() && !values.is_empty())
                    .then(|| values.iter().sum::<f64>() / inf.iter().sum::<f64>())
                    .filter(|v| *v >= 0. && *v <= 1.);
                r[ch] = json!(rate);
                r[mask] = json!(i32::from(rate.is_some()));
            }
            for (ch, field, mask) in [
                ("max_kills_15s_max", "max15", "max15_observed"),
                (
                    "unique_victims_window_max",
                    "victims",
                    "unique_victims_observed",
                ),
                ("burst_points_window_max", "burst", "burst_observed"),
            ] {
                let n = w
                    .iter()
                    .filter_map(|s| s.data[field])
                    .max_by(f64::total_cmp);
                r[ch] = json!(n);
                r[mask] = json!(i32::from(n.is_some()));
            }
            let n = w.iter().filter_map(|s| s.data["interval"]).next_back();
            r["median_kill_interval_s"] = json!(n);
            r["interval_observed"] = json!(i32::from(n.is_some()));
            rows.push(r);
            previous = p;
        }
        if !rows.is_empty() {
            let from = rows.len().saturating_sub(200);
            results.push(rows.split_off(from));
        }
    }
    ensure!(results.len() <= 1, "One player per request required");
    Ok(results.pop().unwrap_or_default())
}
