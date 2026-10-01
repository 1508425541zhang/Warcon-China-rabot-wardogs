//! Empirical CDF and baseline selection, including player-balanced automatic-action gates.
use crate::{
    committee::{FEATURE_VERSION, MODEL_VERSION},
    integrity_score::num,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::HashMap;
pub const METRICS: &[&str] = &[
    "kpm180",
    "uniqueVictims",
    "maxKills15s",
    "medianKillInterval",
    "headshotRate",
    "penetrationRate",
    "headshotRateWeapon",
    "maxKillDistanceWeapon",
];
pub fn config() -> Value {
    json!({"modelVersion":MODEL_VERSION,"featureVersion":FEATURE_VERSION,"persistenceEpisodeHorizonHours":24,"minimumEpisodeSeparationSeconds":60,"minimumIndependentEpisodes":2,"minimumAssessmentSamples":50,"minimumBaselineSamples":200,"minimumBaselinePlayers":20,"minimumBaselinePlayerDays":20,"minimumEffectiveSampleSize":100,"watchPercentile":0.9,"kickPercentile":0.95,"baselineRefreshMinutes":5,"maximumBaselineAgeHours":48})
}
pub fn population_bucket(count: Option<f64>) -> Option<&'static str> {
    let n = count.filter(|n| n.is_finite() && n.fract() == 0. && *n >= 1.)?;
    Some(if n <= 20. {
        "1–20"
    } else if n <= 40. {
        "21–40"
    } else if n <= 60. {
        "41–60"
    } else if n <= 80. {
        "61–80"
    } else {
        "81+"
    })
}
pub fn sample_quality(count: f64) -> &'static str {
    if count < 50. {
        "INSUFFICIENT_DATA"
    } else if count < 1000. {
        "LOW_SAMPLE"
    } else if count < 5000. {
        "NORMAL_SAMPLE"
    } else {
        "HIGH_SAMPLE"
    }
}
pub fn action_eligible(metric: &Value) -> bool {
    metric["source"] == "local"
        && metric["code"] != "maxKillDistanceWeapon"
        && num(metric, "sampleCount") >= 200.
        && num(metric, "uniquePlayers") >= 20.
        && num(metric, "uniquePlayerDays") >= 20.
        && num(metric, "effectiveSampleSize") >= 100.
}
pub fn percentile(cdf: &Value, value: f64) -> anyhow::Result<f64> {
    let rows = cdf
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("invalid_cdf"))?;
    let total: f64 = rows.iter().map(|r| r[1].as_f64().unwrap_or(0.)).sum();
    anyhow::ensure!(total > 0. && total.is_finite(), "empty_cdf");
    let mut less = 0.;
    for row in rows {
        let sample = row[0]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("invalid_cdf"))?;
        let count = row[1]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("invalid_cdf"))?;
        if value < sample {
            return Ok(less / total);
        }
        if value == sample {
            return Ok((less + count / 2.) / total);
        }
        less += count;
    }
    Ok(1.)
}
pub fn robust_z(value: f64, median: f64, mad: Option<f64>) -> Option<f64> {
    mad.filter(|m| *m != 0.)
        .map(|m| 0.6745 * (value - median) / m)
}
fn family(code: &str) -> &'static str {
    if matches!(
        code,
        "kpm180" | "uniqueVictims" | "maxKills15s" | "medianKillInterval"
    ) {
        "Tempo"
    } else {
        "Precision"
    }
}
pub fn assess(
    values: &Value,
    baselines: &HashMap<String, Value>,
    kills: usize,
    episodes: usize,
    weapons: &[Value],
    weapon_baselines: &HashMap<String, Value>,
) -> anyhow::Result<Value> {
    let mut observations: Vec<(&str, Option<f64>, Option<&Value>, usize)> = METRICS[..6]
        .iter()
        .map(|code| (*code, values[*code].as_f64(), baselines.get(*code), kills))
        .collect();
    for weapon in weapons {
        let count = num(weapon, "kills") as usize;
        let cause = weapon["cause"].as_str().unwrap_or("");
        if count >= 10 {
            observations.push((
                "headshotRateWeapon",
                Some(num(weapon, "headshots") / count as f64),
                weapon_baselines.get(&format!("headshotRateWeapon:{cause}")),
                count,
            ));
        }
        if count >= 3 {
            observations.push((
                "maxKillDistanceWeapon",
                weapon["maxKillDistanceM"].as_f64(),
                weapon_baselines.get(&format!("maxKillDistanceWeapon:{cause}")),
                count,
            ));
        }
    }
    let mut metrics = Vec::new();
    for (code, value, baseline, kills) in observations {
        let (Some(value), Some(b)) = (value, baseline) else {
            continue;
        };
        if num(b, "sampleCount") < if b["source"] == "external" { 30. } else { 50. }
            || (matches!(
                code,
                "headshotRate" | "penetrationRate" | "headshotRateWeapon"
            ) && kills < 10)
        {
            continue;
        }
        let pct = percentile(&b["cdf"], value)?;
        let lower = code == "medianKillInterval";
        let extreme = if lower { 1. - pct } else { pct };
        let mut metric = json!({"code":code,"source":b["source"],"value":value,"tail":if lower{"lower"}else{"upper"},"percentile":pct,"extremenessPercentile":extreme,"median":b["median"],"mad":b["mad"],"robustZ":robust_z(value,num(b,"median"),b["mad"].as_f64()),"sampleCount":b["sampleCount"],"baselineId":b["id"],"map":b["map"],"populationBucket":b["populationBucket"],"weaponCategory":b["weaponCategory"],"windowDays":b["windowDays"],"calculatedAt":b["calculatedAt"],"p95":b["p95"],"p99":b["p99"],"p999":b["p999"],"histogram":b["histogram"]});
        for key in [
            "serverId",
            "uniquePlayers",
            "uniquePlayerDays",
            "effectiveSampleSize",
            "modelVersion",
            "featureVersion",
            "weaponMapVersion",
            "baselineGeneration",
        ] {
            if let Some(v) = b.get(key) {
                metric[key] = v.clone();
            }
        }
        metrics.push(metric);
    }
    let maximum = |kind: &str, action: bool| {
        metrics
            .iter()
            .filter(|m| {
                family(m["code"].as_str().unwrap()) == kind && (!action || action_eligible(m))
            })
            .map(|m| num(m, "extremenessPercentile"))
            .reduce(f64::max)
    };
    let tempo = maximum("Tempo", false);
    let precision = maximum("Precision", false);
    let action_tempo = maximum("Tempo", true);
    let action_precision = maximum("Precision", true);
    let strongest = metrics.iter().reduce(|a, b| {
        if num(b, "extremenessPercentile") > num(a, "extremenessPercentile") {
            b
        } else {
            a
        }
    });
    let case = tempo.is_some_and(|p| p >= 0.95)
        || precision.is_some_and(|p| p >= 0.95)
        || (tempo.is_some_and(|p| p >= 0.9) && precision.is_some_and(|p| p >= 0.9));
    let kick = action_tempo.is_some_and(|p| p >= 0.95)
        && (action_precision.is_some_and(|p| p >= 0.9) || episodes >= 2);
    let level = if metrics.is_empty() {
        None
    } else if kick {
        Some("KICK_CANDIDATE")
    } else if case {
        Some("CASE")
    } else if tempo.is_some_and(|p| p >= 0.9) || precision.is_some_and(|p| p >= 0.9) {
        Some("WATCH")
    } else {
        Some("NORMAL")
    };
    let count = metrics
        .iter()
        .filter(|m| !kick || action_eligible(m))
        .map(|m| num(m, "sampleCount"))
        .reduce(f64::min)
        .unwrap_or(0.);
    Ok(
        json!({"status":if metrics.is_empty(){"INSUFFICIENT_DATA"}else{"READY"},"level":level,"tempoPercentile":tempo,"precisionPercentile":precision,"actionTempoPercentile":action_tempo,"actionPrecisionPercentile":action_precision,"strongestMetric":strongest.map(|m|json!({"code":m["code"],"value":m["value"],"percentile":m["extremenessPercentile"]})),"independentEpisodes":episodes,"sampleCount":count,"metrics":metrics}),
    )
}
fn distribution(row: &Value) -> Value {
    let mut value = row.clone();
    let map = value.as_object_mut().unwrap();
    map.remove("orgId");
    map.remove("level");
    map.insert("baselineGeneration".into(), row["generation"].clone());
    map.remove("generation");
    if let Some(t) = row["calculatedAt"]
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    {
        value["calculatedAt"] = json!(t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    }
    value
}
pub fn select(
    rows: &[Value],
    map: &str,
    bucket: Option<&str>,
    now: DateTime<Utc>,
    state: Option<&Value>,
    server: Option<&str>,
    weapon: bool,
) -> HashMap<String, Value> {
    let mut ordered: Vec<_> = rows.iter().collect();
    ordered.sort_by(|a, b| {
        if a["source"] == b["source"] {
            num(a, "level").total_cmp(&num(b, "level"))
        } else if a["source"] == "local" {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });
    let mut selected = HashMap::new();
    for row in ordered {
        let Some(metric) = row["metric"].as_str().filter(|m| METRICS.contains(m)) else {
            continue;
        };
        if weapon {
            if !matches!(metric, "headshotRateWeapon" | "maxKillDistanceWeapon")
                || row["weaponCategory"] == "INFANTRY"
            {
                continue;
            }
        } else if row["weaponCategory"] != "INFANTRY" {
            continue;
        }
        if matches!(
            metric,
            "headshotRate" | "penetrationRate" | "headshotRateWeapon"
        ) && (server.is_none() || row["serverId"].as_str() != server)
        {
            continue;
        }
        if num(row, "sampleCount")
            < if row["source"] == "external" {
                30.
            } else {
                50.
            }
            || row["windowDays"] != 30
            || row["modelVersion"] != MODEL_VERSION
            || row["featureVersion"] != FEATURE_VERSION
        {
            continue;
        }
        if state.is_some_and(|s| {
            s["baselineStatus"] != "READY"
                || row["generation"] != s["activeBaselineGeneration"]
                || row["weaponMapVersion"] != s["weaponMapVersion"]
        }) {
            continue;
        }
        let Some(calculated) = row["calculatedAt"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        else {
            continue;
        };
        if now.signed_duration_since(calculated).num_milliseconds() > 48 * 3600000 {
            continue;
        }
        let level = num(row, "level");
        let bucket_matches = bucket.is_some() && row["populationBucket"].as_str() == bucket;
        if (level == 1. && (row["map"] != map || !bucket_matches))
            || (level == 2. && !bucket_matches)
            || (!weapon && ![1., 2., 3.].contains(&level))
        {
            continue;
        }
        let key = if weapon {
            format!("{metric}:{}", row["weaponCategory"].as_str().unwrap_or(""))
        } else {
            metric.into()
        };
        selected.entry(key).or_insert_with(|| distribution(row));
    }
    selected
}
pub async fn population_at(
    db: &sqlx::PgPool,
    server: &str,
    at: DateTime<Utc>,
) -> crate::error::Result<Option<&'static str>> {
    let count:Option<i32>=sqlx::query_scalar("SELECT player_count FROM samples WHERE server_id=$1 AND ok AND ts<=$2 AND ts>$2-interval '120 seconds' ORDER BY ts DESC LIMIT 1").bind(server).bind(at).fetch_optional(db).await?.flatten();
    Ok(population_bucket(count.map(|n| n as f64)))
}
pub fn camel_row(row: Value) -> Value {
    let Some(object) = row.as_object() else {
        return row;
    };
    Value::Object(
        object
            .iter()
            .map(|(key, value)| {
                let mut upper = false;
                let mut name = String::new();
                for c in key.chars() {
                    if c == '_' {
                        upper = true;
                    } else if upper {
                        name.extend(c.to_uppercase());
                        upper = false;
                    } else {
                        name.push(c);
                    }
                }
                (name, value.clone())
            })
            .collect(),
    )
}
pub async fn load(
    db: &sqlx::PgPool,
    org: &str,
    map: &str,
    bucket: Option<&str>,
    server: Option<&str>,
) -> crate::error::Result<(HashMap<String, Value>, HashMap<String, Value>)> {
    let state:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id=$1 AND baseline_status='READY'").bind(org).fetch_optional(db).await?;
    let Some(state) = state.map(camel_row) else {
        return Ok((HashMap::new(), HashMap::new()));
    };
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(b) FROM integrity_baselines b WHERE org_id=$1 AND sample_count>=30 AND calculated_at>=now()-interval '48 hours'").bind(org).fetch_all(db).await?;
    let rows: Vec<Value> = rows.into_iter().map(camel_row).collect();
    let now = Utc::now();
    Ok((
        select(&rows, map, bucket, now, Some(&state), server, false),
        select(&rows, map, bucket, now, Some(&state), server, true),
    ))
}
