//! Descriptive charts and read-time scores. Nothing here creates enforcement evidence.
use super::Context;
use crate::{
    auth,
    config::AppState,
    error::{ApiError, Result},
    integrity_score as score, integrity_weapons as weapons,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.)
}
pub fn map(s: &str) -> String {
    crate::feed::map_id(s)
}
pub fn infantry(
    rows: &[Value],
    status: &Value,
    overrides: &HashMap<String, String>,
    now: i64,
) -> HashMap<String, Value> {
    let Some(first) = rows.first() else {
        return HashMap::new();
    };
    if status["map"]
        .as_str()
        .is_some_and(|v| !v.is_empty() && map(v) != map(first["map"].as_str().unwrap_or("")))
    {
        return HashMap::new();
    }
    let batch = rows
        .iter()
        .filter(|r| r["instanceId"] == first["instanceId"] && r["map"] == first["map"])
        .collect::<Vec<_>>();
    let latest = batch
        .iter()
        .map(|r| num(&r["eventTime"]))
        .fold(f64::NEG_INFINITY, f64::max);
    if status["matchSeconds"]
        .as_f64()
        .is_some_and(|n| n < latest - 5.)
    {
        return HashMap::new();
    }
    let elapsed = crate::integrity_enforcement::date(&first["ts"])
        .map(|d| ((now - d.timestamp_millis()) as f64 / 1000.).max(0.))
        .unwrap_or(0.);
    let clock = latest.max(status["matchSeconds"].as_f64().unwrap_or(latest + elapsed));
    let mut by: HashMap<String, Vec<&Value>> = HashMap::new();
    let mut unknown: HashMap<String, f64> = HashMap::new();
    for r in batch {
        let t = num(&r["eventTime"]);
        if t > clock - 600.
            && t <= clock
            && r["killerSteamId"].as_str().is_some_and(|s| !s.is_empty())
            && r["killerSteamId"] != r["victimSteamId"]
            && r["suicide"] != true
        {
            let category = weapons::classify(r, overrides);
            if category == "UNKNOWN"
                || (category == "INFANTRY"
                    && (!r["killerFaction"].as_str().is_some_and(|v| !v.is_empty())
                        || !r["victimFaction"].as_str().is_some_and(|v| !v.is_empty())
                        || r["factionBracketed"] == false
                        || (r["factionBracketed"] == true
                            && !crate::http::truthy(&r["factionObservedAt"]))))
            {
                let id = r["killerSteamId"].as_str().unwrap().to_owned();
                unknown.entry(id).and_modify(|v| *v = v.max(t)).or_insert(t);
            }
        }
        if weapons::infantry(r, overrides) {
            by.entry(r["killerSteamId"].as_str().unwrap().into())
                .or_default()
                .push(r);
        }
    }
    let ids: HashSet<String> = by.keys().chain(unknown.keys()).cloned().collect();
    let mut out = HashMap::new();
    for id in ids {
        let mut rows = by.remove(&id).unwrap_or_default();
        rows.sort_by(|a, b| num(&a["eventTime"]).total_cmp(&num(&b["eventTime"])));
        let mut start = 0;
        let mut peak = 0;
        for end in 0..rows.len() {
            while num(&rows[start]["eventTime"]) <= num(&rows[end]["eventTime"]) - 180. {
                start += 1;
            }
            peak = peak.max(end - start + 1);
        }
        let current = rows
            .iter()
            .filter(|r| num(&r["eventTime"]) > clock - 180. && num(&r["eventTime"]) <= clock)
            .collect::<Vec<_>>();
        let short = current
            .iter()
            .filter(|r| num(&r["eventTime"]) > clock - 60.)
            .count();
        let victims = current
            .iter()
            .filter_map(|r| r["victimSteamId"].as_str())
            .collect::<HashSet<_>>()
            .len();
        let uncertain = unknown.get(&id).copied().unwrap_or(f64::NEG_INFINITY);
        out.insert(id,json!({"infantryKills180":current.len(),"kpm180":current.len()as f64/3.,"infantryKills60":short,"kpm60":short,"reliable180":uncertain<=clock-180.,"reliable60":uncertain<=clock-60.,"peakKpm180":peak as f64/3.,"uniqueVictims180":victims,"reliable":!uncertain.is_finite()}));
    }
    out
}
pub async fn live_events(state: &AppState, id: &str) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar::<_,Value>("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND ts>=now()-interval '10 minutes' ORDER BY ts DESC LIMIT 3001").bind(id).fetch_all(&state.db).await?.into_iter().map(crate::ai_evidence::row).collect())
}
pub async fn current_risks(
    state: &AppState,
    org: &str,
    ids: &[String],
    saved: &HashMap<String, Value>,
    config: &Value,
) -> Result<HashMap<String, Value>> {
    let profiles:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(p) FROM steam_profiles p WHERE steam_id=ANY($1) AND error='' AND fetched_at<=now() AND fetched_at>=now()-interval '24 hours'").bind(ids).fetch_all(&state.db).await?;
    let counts:HashMap<String,i64>=sqlx::query_as::<_,(String,i64)>("SELECT target_steam_id,count(DISTINCT reporter_steam_id) FROM integrity_reports WHERE org_id=$1 AND target_steam_id=ANY($2) AND created_at>=now()-interval '24 hours' GROUP BY target_steam_id").bind(org).bind(ids).fetch_all(&state.db).await?.into_iter().collect();
    let mut result = HashMap::new();
    for id in ids {
        let profile = profiles.iter().find(|p| p["steam_id"] == *id);
        let reports = counts.get(id).copied().unwrap_or(0);
        let old = saved.get(id);
        if old.is_none() && profile.is_none() && reports == 0 {
            continue;
        }
        if let Some(old) = old.filter(|s| !s["breakdown"].is_array()) {
            result.insert(
                id.clone(),
                json!({"score":old["score"],"level":old["level"],"breakdown":[]}),
            );
            continue;
        }
        let p = profile.cloned().unwrap_or(Value::Null);
        let signals:score::Signals=serde_json::from_value(json!({"behaviorReasons":[],"kpm180":0,"uniqueVictims":0,"previousKpm":[],"uniqueReporters":reports,"repeatHighRiskWindow":false,"infantryKills":0,"headshots":0,"penetrations":0,"burstPoints":0,"vacBans":num(&p["vac_bans"]),"gameBans":num(&p["game_bans"]),"daysSinceLastBan":p["days_since_last_ban"],"wardogsPlaytimeHours":null})).expect("typed signals");
        let mut parts = old
            .map(|s| {
                s["breakdown"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|v| {
                        v["code"].is_string()
                            && v["points"].as_f64().is_some_and(|n| n >= 0.)
                            && v["code"] != "steam_ban_prior"
                            && v["code"] != "unique_reports"
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        parts.extend(score::score(&signals, config).breakdown);
        let value = parts
            .iter()
            .map(|v| num(&v["points"]))
            .sum::<f64>()
            .min(100.);
        result.insert(
            id.clone(),
            json!({"score":value,"level":score::level(value,config),"breakdown":parts}),
        );
    }
    Ok(result)
}
pub async fn player(c: &mut Context) -> Result<Value> {
    let a = c.actor().await?;
    let id = c.param("id").to_owned();
    let steam = c.param("steamId").to_owned();
    let scope = auth::server_scope(&c.state, &a, &id, "server.view").await?;
    if crate::api::notes::steam_id(&steam).is_err() {
        return Err(ApiError::missing());
    }
    let (ids, names) = crate::player_views::visible(&c.state, &a, &scope.org_id).await?;
    let dossier = crate::player_views::dossier(&c.state, &a, &scope, &steam).await?;
    let career = crate::leaderboards::career(&c.state, &id, &ids, &names, &steam).await?;
    let allowed = scope.caps.iter().any(|v| v == "integrity.view");
    let (integrity, distances, progress) = if allowed {
        (
            Some(player_integrity(&c.state, &scope.org_id, &id, &steam).await?),
            Some(distances(&c.state, &id, &steam).await?),
            Some(progress(&c.state, &id, &steam).await?),
        )
    } else {
        (None, None, None)
    };
    Ok(
        json!({"dossier":dossier,"career":career,"multiServer":ids.len()>1,"integrity":integrity,"weaponDistances":distances,"playerProgress":progress}),
    )
}
pub async fn player_integrity(state: &AppState, org: &str, id: &str, steam: &str) -> Result<Value> {
    let profile: Option<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(p) FROM integrity_profiles p WHERE org_id=$1 AND steam_id=$2",
    )
    .bind(org)
    .bind(steam)
    .fetch_optional(&state.db)
    .await?;
    let profile = profile.map(crate::ai_evidence::row).unwrap_or(Value::Null);
    let risk:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_scores s WHERE server_id=$1 AND steam_id=$2 AND scored_at>=now()-interval '15 minutes' ORDER BY score DESC,scored_at DESC LIMIT 1").bind(id).bind(steam).fetch_optional(&state.db).await?;
    let saved: HashMap<_, _> = risk
        .map(|v| [(steam.to_owned(), crate::ai_evidence::row(v))].into())
        .unwrap_or_default();
    let rules = crate::integrity_rules::load(&state.db, org).await?;
    let mut risks = current_risks(state, org, &[steam.to_owned()], &saved, &rules.config).await?;
    let risk = risks.remove(steam).unwrap_or(Value::Null);
    let window:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('kpm180',kpm_180,'infantryKills',infantry_kills,'uniqueVictims',unique_victims,'observedAt',observed_at) FROM integrity_windows WHERE server_id=$1 AND steam_id=$2 ORDER BY observed_at DESC LIMIT 1").bind(id).bind(steam).fetch_optional(&state.db).await?;
    let reports:i64=sqlx::query_scalar("SELECT count(DISTINCT reporter_steam_id) FROM integrity_reports WHERE org_id=$1 AND target_steam_id=$2 AND created_at>=now()-interval '24 hours'").bind(org).bind(steam).fetch_one(&state.db).await?;
    let live: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=$1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let live = live.unwrap_or(Value::Null);
    let events = live_events(state, id).await?;
    let overrides = weapons::overrides(&state.db, org).await?;
    let metrics = if events.len() <= 3000 {
        infantry(
            &events,
            &live["status"],
            &overrides,
            Utc::now().timestamp_millis(),
        )
        .remove(steam)
    } else {
        None
    };
    let current = live["players"]
        .as_array()
        .and_then(|a| a.iter().find(|p| p["steamId"] == steam))
        .map(|p| json!({"kills":p["kills"],"deaths":p["deaths"]}));
    let feed: bool =
        sqlx::query_scalar("SELECT feed_token_hash IS NOT NULL FROM servers WHERE id=$1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let available = feed
        && crate::integrity_enforcement::date(&live["feed_at"])
            .is_some_and(|t| Utc::now() - t < chrono::Duration::minutes(5))
        && events.len() <= 3000
        && feed_metrics_available(&events, &live["status"]);
    Ok(
        json!({"aliases":profile["aliases"].as_array().cloned().unwrap_or_default(),"firstSeen":profile["firstSeen"],"lastSeen":profile["lastSeen"],"riskScore":risk["score"],"riskLevel":risk["level"],"breakdown":risk["breakdown"].as_array().cloned().unwrap_or_default(),"latestWindow":window,"reports24h":reports,"current":current,"metrics":metrics,"metricsAvailable":available}),
    )
}
pub fn feed_metrics_available(events: &[Value], status: &Value) -> bool {
    events.first().is_some_and(|k| {
        status["map"]
            .as_str()
            .is_none_or(|v| v.is_empty() || map(v) == map(k["map"].as_str().unwrap_or("")))
            && status["matchSeconds"]
                .as_f64()
                .is_none_or(|n| n >= num(&k["eventTime"]) - 5.)
    })
}
pub fn distribution(values: &[f64], current: f64) -> Value {
    let values = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > 0.)
        .collect::<Vec<_>>();
    let upper = values
        .iter()
        .copied()
        .fold(1_f64, f64::max)
        .max(if current.is_finite() { current } else { 0. })
        * 1.05;
    let step = upper / 12.;
    let mut counts = [0; 12];
    for v in &values {
        counts[((*v / step).floor() as usize).min(11)] += 1;
    }
    json!({"upper":upper,"step":step,"players":values.len(),"peak":counts.iter().copied().max().unwrap_or(0).max(1),"bins":counts.into_iter().enumerate().map(|(i,count)|json!({"from":i as f64*step,"to":(i+1)as f64*step,"center":(i as f64+0.5)*step,"count":count})).collect::<Vec<_>>()})
}
type DistanceCache = HashMap<(String, String), (Instant, String, Vec<Value>)>;
static DISTANCE: OnceLock<Mutex<DistanceCache>> = OnceLock::new();
async fn distances(state: &AppState, id: &str, steam: &str) -> Result<Value> {
    let key = (state.config.origin.clone(), id.to_owned());
    let cache = DISTANCE.get_or_init(Default::default);
    let hit = cache
        .lock()
        .await
        .get(&key)
        .filter(|v| v.0.elapsed() < Duration::from_secs(60))
        .cloned();
    let (_, at, rows) = if let Some(v) = hit {
        v
    } else {
        let rows:Vec<Value>=sqlx::query_scalar("WITH grouped AS(SELECT killer_steam_id AS \"steamId\",cause,count(*) AS samples,avg(distance_m::float8) AS average,max(distance_m) AS maximum FROM kills WHERE server_id=$1 AND ts>=now()-interval '30 days' AND ts<=now() AND killer_steam_id~'^[0-9]{17}$' AND killer_steam_id<>victim_steam_id AND NOT suicide AND NOT team_kill AND NOT distance_invalid AND distance_m>0 AND distance_m<5000 AND cause LIKE 'Id.Item.%' GROUP BY killer_steam_id,cause),ranked AS(SELECT \"steamId\",cause,rank() OVER(PARTITION BY cause ORDER BY average DESC) AS rank FROM grouped WHERE samples>=10),totals AS(SELECT cause,sum(average*samples)/sum(samples) AS \"serverAverage\",count(*) FILTER(WHERE samples>=10) AS \"eligiblePlayers\" FROM grouped GROUP BY cause) SELECT to_jsonb(g)||to_jsonb(t)||jsonb_build_object('rank',r.rank) FROM grouped g JOIN totals t USING(cause) LEFT JOIN ranked r USING(\"steamId\",cause)").bind(id).fetch_all(&state.db).await?;
        let value = (
            Instant::now(),
            Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            rows,
        );
        let mut cache = cache.lock().await;
        cache.retain(|_, v| v.0.elapsed() < Duration::from_secs(60));
        if cache.len() >= 32 {
            if let Some(key) = cache
                .iter()
                .min_by_key(|(_, v)| v.0)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&key);
            }
        }
        cache.insert(key, value.clone());
        value
    };
    let mut own = rows
        .iter()
        .filter(|r| r["steamId"] == steam)
        .cloned()
        .collect::<Vec<_>>();
    for row in &mut own {
        let peers = rows
            .iter()
            .filter(|r| r["cause"] == row["cause"] && num(&r["samples"]) >= 10.)
            .map(|r| num(&r["average"]))
            .collect::<Vec<_>>();
        row["distribution"] = distribution(&peers, num(&row["average"]));
    }
    own.sort_by(|a, b| {
        num(&b["samples"])
            .total_cmp(&num(&a["samples"]))
            .then_with(|| a["cause"].as_str().cmp(&b["cause"].as_str()))
    });
    Ok(json!({"days":30,"minimumSamples":10,"refreshedAt":at,"rows":own}))
}
pub fn cash_progress(samples: &[Value], steam: &str, start: DateTime<Utc>) -> Vec<Value> {
    let mut initial: HashMap<String, f64> = HashMap::new();
    let mut out = vec![];
    let mut rows = samples.to_vec();
    rows.sort_by_key(|s| crate::integrity_enforcement::date(&s["observedAt"]));
    for sample in rows {
        let players = sample["players"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["cash"].as_f64().is_some())
            .collect::<Vec<_>>();
        for p in &players {
            initial
                .entry(p["steamId"].as_str().unwrap_or("").into())
                .or_insert(num(&p["cash"]));
        }
        let Some(own) = players.iter().find(|p| p["steamId"] == steam) else {
            continue;
        };
        let Some(at) = crate::integrity_enforcement::date(&sample["observedAt"]) else {
            continue;
        };
        let growth = players
            .iter()
            .map(|p| num(&p["cash"]) - initial[p["steamId"].as_str().unwrap_or("")])
            .sum::<f64>();
        out.push(json!({"seconds":((at-start).num_milliseconds()as f64/1000.).max(0.),"cash":own["cash"],"growth":num(&own["cash"])-initial[steam],"serverGrowth":growth/players.len()as f64,"peers":players.len()}));
    }
    out
}
async fn progress(state: &AppState, id: &str, steam: &str) -> Result<Vec<Value>> {
    let rounds:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',m.id,'map',m.map,'startedAt',m.started_at,'endedAt',m.ended_at) FROM matches m WHERE m.server_id=$1 AND m.started_at>now()-interval '30 days' AND EXISTS(SELECT 1 FROM player_progress_samples p WHERE p.match_id=m.id AND p.players@>jsonb_build_array(jsonb_build_object('steamId',$2::text))) ORDER BY m.id DESC LIMIT 10").bind(id).bind(steam).fetch_all(&state.db).await?;
    let mut out = vec![];
    for mut round in rounds {
        let samples:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('observedAt',observed_at,'players',players) FROM player_progress_samples WHERE match_id=$1 ORDER BY observed_at LIMIT 6000").bind(round["id"].as_i64().unwrap()).fetch_all(&state.db).await?;
        round["truncated"] = json!(samples.len() == 6000);
        round["points"] = json!(cash_progress(
            &samples,
            steam,
            crate::integrity_enforcement::date(&round["startedAt"])
                .ok_or_else(|| ApiError::bad("Invalid match date."))?
        ));
        out.push(round);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn equivalent(actual: &Value, expected: &Value, path: &str) {
        match (actual, expected) {
            (Value::Number(a), Value::Number(b)) => {
                let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                assert!(
                    (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.),
                    "{path}: {a} != {b}"
                );
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "{path}");
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    equivalent(a, b, &format!("{path}[{i}]"));
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(
                    a.keys().collect::<Vec<_>>(),
                    b.keys().collect::<Vec<_>>(),
                    "{path}"
                );
                for (key, value) in a {
                    equivalent(value, &b[key], &format!("{path}.{key}"));
                }
            }
            _ => assert_eq!(actual, expected, "{path}"),
        }
    }
    #[test]
    fn descriptive_charts_match_existing_page_sources() {
        let f: Value =
            serde_json::from_str(include_str!("../../fixtures/page-charts.json")).unwrap();
        for c in f["infantry"].as_array().unwrap() {
            equivalent(
                &json!(infantry(
                    c["rows"].as_array().unwrap(),
                    &c["status"],
                    &HashMap::new(),
                    c["now"].as_i64().unwrap()
                )),
                &c["result"],
                &format!("infantry status={} first={}", c["status"], c["rows"][0]),
            );
        }
        for c in f["distributions"].as_array().unwrap() {
            let values = c["values"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_f64)
                .collect::<Vec<_>>();
            equivalent(
                &distribution(&values, num(&c["current"])),
                &c["result"],
                "distance distribution",
            );
        }
        for c in f["cash"].as_array().unwrap() {
            equivalent(
                &json!(cash_progress(
                    c["samples"].as_array().unwrap(),
                    c["steam"].as_str().unwrap(),
                    crate::integrity_enforcement::date(&c["startedAt"]).unwrap()
                )),
                &c["result"],
                "cash progress",
            );
        }
    }
}
