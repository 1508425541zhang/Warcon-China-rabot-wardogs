//! Non-overlapping clean player history; case evidence excludes episodes, never entire players.
use crate::committee::{FEATURE_VERSION, MODEL_VERSION};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub round_id: String,
    pub observed_at: DateTime<Utc>,
    pub kpm180: f64,
    pub headshot_rate: Option<f64>,
    pub max_kills15s: f64,
}
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.
    }
}
pub fn distribution(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let q = |p: f64| {
        let at = (sorted.len() - 1) as f64 * p;
        let low = at.floor() as usize;
        sorted[low] + (sorted[at.ceil() as usize] - sorted[low]) * (at - low as f64)
    };
    let center = median(&sorted);
    let deviations: Vec<_> = sorted.iter().map(|v| (v - center).abs()).collect();
    json!({"sampleCount":sorted.len(),"median":center,"mad":median(&deviations),"p90":q(0.9),"p95":q(0.95),"p99":q(0.99)})
}
pub fn summarize(rows: &[History]) -> Value {
    let mut ordered: Vec<_> = rows.iter().collect();
    ordered.sort_by_key(|r| r.observed_at);
    let mut last = HashMap::new();
    let independent: Vec<_> = ordered
        .into_iter()
        .filter(|r| {
            let at = r.observed_at.timestamp_millis();
            if last.get(&r.round_id).is_some_and(|prev| at - prev < 180000) {
                return false;
            }
            last.insert(r.round_id.clone(), at);
            true
        })
        .collect();
    let Some(first) = independent.first() else {
        return Value::Null;
    };
    let last = independent.last().unwrap();
    let kpm: Vec<_> = independent.iter().map(|r| r.kpm180).collect();
    let d = distribution(&kpm);
    let heads: Vec<_> = independent.iter().filter_map(|r| r.headshot_rate).collect();
    let bursts: Vec<_> = independent.iter().map(|r| r.max_kills15s).collect();
    let recent = |days: i64| {
        let cutoff = last.observed_at - chrono::Duration::days(days);
        distribution(
            &independent
                .iter()
                .filter(|r| r.observed_at >= cutoff)
                .map(|r| r.kpm180)
                .collect::<Vec<_>>(),
        )
    };
    json!({"sampleCount":independent.len(),"uniqueDays":independent.iter().map(|r|r.observed_at.format("%Y-%m-%d").to_string()).collect::<HashSet<_>>().len(),"firstSeenAt":first.observed_at.to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"lastSeenAt":last.observed_at.to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"kpmMedian":d["median"],"kpmMad":d["mad"],"kpmDistribution":d,"orderedKpm":kpm,"lifetimeMatches":independent.iter().map(|r|&r.round_id).collect::<HashSet<_>>().len(),"headshotDistribution":distribution(&heads),"burstDistribution":distribution(&bursts),"recent7d":recent(7),"recent24h":recent(1),"recent30d":recent(30)})
}
pub async fn load(
    db: &mut sqlx::PgConnection,
    org: &str,
    steam: &str,
    before: DateTime<Utc>,
) -> crate::error::Result<Option<Value>> {
    let existing:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(p) FROM integrity_player_careers p WHERE org_id=$1 AND steam_id=$2 AND status='READY' AND model_version=$3 AND feature_version=$4 AND last_seen_at<$5 AND updated_at>now()-interval '24 hours' AND NOT EXISTS(SELECT 1 FROM integrity_case_events ce JOIN integrity_cases c ON c.id=ce.case_id WHERE c.org_id=p.org_id AND c.steam_id=p.steam_id AND c.created_at>p.updated_at AND ce.event->>'killerSteamId'=p.steam_id AND (ce.event->>'ts')::timestamptz BETWEEN p.first_seen_at-interval '180 seconds' AND p.last_seen_at)").bind(org).bind(steam).bind(MODEL_VERSION).bind(FEATURE_VERSION).bind(before).fetch_optional(&mut *db).await?;
    if let Some(row) = existing.filter(|row| {
        row["ordered_kpm"]
            .as_array()
            .is_some_and(|rows| rows.iter().all(|r| r.as_f64().is_some_and(f64::is_finite)))
            && row["kpm_distribution"]["median"].as_f64().is_some()
            && row["kpm_distribution"]["mad"].as_f64().is_some()
    }) {
        return Ok(Some(
            json!({"sampleCount":row["lifetime_windows"],"uniqueDays":row["active_days"],"kpmMedian":row["kpm_distribution"]["median"],"kpmMad":row["kpm_distribution"]["mad"],"orderedKpm":row["ordered_kpm"]}),
        ));
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('roundId',h.round_id,'observedAt',h.observed_at,'kpm180',h.kpm_180,'headshotRate',h.headshot_rate,'maxKills15s',h.max_kills_15s) FROM integrity_player_metric_history h WHERE h.org_id=$1 AND h.steam_id=$2 AND h.model_version=$3 AND h.feature_version=$4 AND h.observed_at<$5 AND NOT EXISTS(SELECT 1 FROM integrity_case_events ce JOIN integrity_cases c ON c.id=ce.case_id WHERE c.org_id=$1 AND c.steam_id=$2 AND ce.event->>'killerSteamId'=$2 AND ce.event->>'ts' IS NOT NULL AND (ce.event->>'ts')::timestamptz>h.observed_at-interval '180 seconds' AND (ce.event->>'ts')::timestamptz<=h.observed_at) ORDER BY h.observed_at DESC LIMIT 2001").bind(org).bind(steam).bind(MODEL_VERSION).bind(FEATURE_VERSION).bind(before).fetch_all(&mut *db).await?;
    if rows.len() > 2000 {
        return Ok(None);
    }
    let history: Vec<History> = rows
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()
        .map_err(|_| crate::error::ApiError::bad("Invalid career history."))?;
    let summary = summarize(&history);
    if summary.is_null() {
        return Ok(None);
    }
    let totals:Value=sqlx::query_scalar("SELECT jsonb_build_object('valid',coalesce(sum(greatest(mp.kills-mp.team_kills-mp.suicides-mp.vehicle_kills,0)),0),'seconds',coalesce(sum(mp.seconds),0)) FROM match_players mp JOIN servers s ON s.id=mp.server_id WHERE s.org_id=$1 AND mp.steam_id=$2").bind(org).bind(steam).fetch_one(&mut *db).await?;
    let mut row = json!({"org_id":org,"steam_id":steam,"lifetime_valid_kills":totals["valid"],"lifetime_playtime_seconds":totals["seconds"],"lifetime_windows":summary["sampleCount"],"active_days":summary["uniqueDays"],"model_version":MODEL_VERSION,"feature_version":FEATURE_VERSION,"status":"READY","updated_at":Utc::now()});
    for (camel, snake) in [
        ("firstSeenAt", "first_seen_at"),
        ("lastSeenAt", "last_seen_at"),
        ("lifetimeMatches", "lifetime_matches"),
        ("kpmDistribution", "kpm_distribution"),
        ("headshotDistribution", "headshot_distribution"),
        ("burstDistribution", "burst_distribution"),
        ("recent24h", "recent_24h"),
        ("recent7d", "recent_7d"),
        ("recent30d", "recent_30d"),
        ("orderedKpm", "ordered_kpm"),
    ] {
        row[snake] = summary[camel].clone();
    }
    sqlx::query("INSERT INTO integrity_player_careers SELECT * FROM jsonb_populate_record(NULL::integrity_player_careers,$1) ON CONFLICT(org_id,steam_id) DO UPDATE SET first_seen_at=excluded.first_seen_at,last_seen_at=excluded.last_seen_at,lifetime_valid_kills=excluded.lifetime_valid_kills,lifetime_playtime_seconds=excluded.lifetime_playtime_seconds,lifetime_windows=excluded.lifetime_windows,lifetime_matches=excluded.lifetime_matches,active_days=excluded.active_days,kpm_distribution=excluded.kpm_distribution,headshot_distribution=excluded.headshot_distribution,burst_distribution=excluded.burst_distribution,recent_24h=excluded.recent_24h,recent_7d=excluded.recent_7d,recent_30d=excluded.recent_30d,ordered_kpm=excluded.ordered_kpm,model_version=excluded.model_version,feature_version=excluded.feature_version,status=excluded.status,updated_at=excluded.updated_at").bind(row).execute(&mut *db).await?;
    Ok(Some(summary))
}
