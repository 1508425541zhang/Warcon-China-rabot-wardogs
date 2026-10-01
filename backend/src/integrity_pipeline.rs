//! Ordered assessment and immutable case creation; memory is advanced only after database commit.
use crate::{
    api::kills::view as kill_view,
    committee,
    config::AppState,
    error::{ApiError, Result},
    integrity_career, integrity_cases, integrity_context as context,
    integrity_decisions as decisions,
    integrity_enforcement::{self, Candidate, date},
    integrity_rules as rules,
    integrity_score::{self, num},
    integrity_statistics as statistics, integrity_weapons,
    integrity_windows::{self, Finding, Windows},
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;
#[derive(Default, Clone)]
struct ServerState {
    windows: Windows,
    version: i32,
    mapping: i32,
    mode: String,
}
#[derive(Default)]
pub struct Pipeline {
    servers: Mutex<HashMap<String, Arc<Mutex<ServerState>>>>,
}
pub struct Outcome {
    pub alerts: Vec<Value>,
    pub actions: Vec<String>,
}
impl Pipeline {
    pub async fn process(
        &self,
        state: &AppState,
        server: &str,
        batch: &[Value],
        mut allow_actions: bool,
    ) -> Result<Outcome> {
        capture_reports(state, server, batch).await?;
        let org:Option<String>=sqlx::query_scalar("SELECT s.org_id FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1 AND o.suspended_at IS NULL").bind(server).fetch_optional(&state.db).await?;
        let Some(org) = org else {
            return Ok(Outcome {
                alerts: vec![],
                actions: vec![],
            });
        };
        let config = rules::load(&state.db, &org).await?;
        let slot = {
            let mut servers = self.servers.lock().await;
            servers.entry(server.into()).or_default().clone()
        };
        let mut current = slot.lock().await;
        if ["disabled", "short_only"].contains(&config.assessment_mode.as_str()) {
            *current = ServerState::default();
            return Ok(Outcome {
                alerts: vec![],
                actions: vec![],
            });
        }
        if rules::long_enabled(&config.assessment_mode)
            && !crate::model_http::config(&state.db, &org)
                .await?
                .developer_enabled
        {
            return Ok(Outcome {
                alerts: vec![],
                actions: vec![],
            });
        }
        let mapping: i32 = sqlx::query_scalar(
            "SELECT weapon_map_version FROM integrity_model_state WHERE org_id=$1",
        )
        .bind(&org)
        .fetch_optional(&state.db)
        .await?
        .unwrap_or(1);
        let overrides = integrity_weapons::overrides(&state.db, &org).await?;
        let mut next = if current.version != config.version
            || current.mapping != mapping
            || current.mode != config.assessment_mode
        {
            ServerState {
                version: config.version,
                mapping,
                mode: config.assessment_mode.clone(),
                ..Default::default()
            }
        } else {
            current.clone()
        };
        if !next.windows.has_server(server) {
            if let Some(first) = batch.first() {
                if let Some(before) = date(&first["ts"]) {
                    let prior:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND instance_id=$2 AND match_id=$3 AND map=$4 AND ts>=$5 AND ts<$6 ORDER BY ts DESC,event_time DESC LIMIT 1001").bind(server).bind(first["instanceId"].as_str().unwrap_or("")).bind(first["matchId"].as_str().unwrap_or("")).bind(first["map"].as_str().unwrap_or("")).bind(before-Duration::seconds(180)).bind(before).fetch_all(&state.db).await?;
                    if prior.len() > 1000 {
                        allow_actions = false
                    }
                    let prior: Vec<_> = prior.into_iter().take(1000).rev().map(kill_view).collect();
                    next.windows
                        .observe(server, &prior, &overrides, &config.config);
                }
            }
        }
        let generated = integrity_windows::generate(
            &mut next.windows,
            server,
            batch,
            &overrides,
            &config.config,
        );
        let mut tx = state.worker_transaction().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("integrity-feed:{server}"))
            .execute(&mut *tx)
            .await?;
        // A setting changed while historical context was being read: retry the durable job.
        let row: Option<Value> =
            sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_rules r WHERE org_id=$1")
                .bind(&org)
                .fetch_optional(&mut *tx)
                .await?;
        let current_rules = rules::from_row(row.as_ref())?;
        if current_rules.version != config.version
            || current_rules.assessment_mode != config.assessment_mode
        {
            return Err(ApiError::new(
                axum::http::StatusCode::CONFLICT,
                "rules_changed",
                "Integrity rules changed during assessment.",
            ));
        }
        if rules::long_enabled(&config.assessment_mode) {
            for f in &generated.snapshots {
                let Some(event) = batch.iter().find(|k| {
                    f.event_ids
                        .last()
                        .is_some_and(|id| k["eventId"] == id.as_str())
                }) else {
                    continue;
                };
                let Some(at) = date(&event["ts"]) else {
                    continue;
                };
                let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_windows WHERE server_id=$1 AND steam_id=$2 AND observed_at=$3 AND round_id=$4)").bind(server).bind(&f.steam_id).bind(at).bind(&f.round_id).fetch_one(&mut *tx).await?;
                if !exists {
                    save_window(&mut tx, &org, server, f, at, None).await?;
                }
            }
            tx.commit().await?;
            *current = next;
            // Long model queue is registered by the native model consumer, independently
            // of this ordered lane. This branch only persists the measured inputs.
            return Ok(Outcome {
                alerts: vec![],
                actions: vec![],
            });
        }
        let mut findings = generated.findings;
        if config.assessment_mode != "legacy" {
            findings.extend(generated.snapshots);
            let mut latest: Vec<&Value> = Vec::new();
            for event in batch {
                let Some(id) = event["killer"]["steamId"].as_str() else {
                    continue;
                };
                if event["suicide"] == true
                    || event["teamKill"] == true
                    || event["victim"]["steamId"] == id
                    || event["matchRow"].as_i64().is_none()
                {
                    continue;
                }
                if let Some(index) = latest.iter().position(|old| {
                    old["killer"]["steamId"] == id
                        && old["instanceId"] == event["instanceId"]
                        && old["matchRow"] == event["matchRow"]
                }) {
                    latest[index] = event
                } else {
                    latest.push(event)
                }
            }
            for event in latest {
                if findings.iter().any(|f| {
                    event["eventId"]
                        .as_str()
                        .is_some_and(|id| f.event_ids.iter().any(|s| s == id))
                }) {
                    continue;
                }
                let id = event["killer"]["steamId"].as_str().unwrap();
                let round = format!(
                    "{}:match:{}",
                    event["instanceId"].as_str().unwrap_or(""),
                    event["matchRow"].as_i64().unwrap()
                );
                let clock = num(event, "eventTime");
                let snapshot = next
                    .windows
                    .snapshots(server, &[id.into()])
                    .into_iter()
                    .find(|f| f.round_id == round);
                let mut finding = snapshot.unwrap_or(Finding {
                    steam_id: id.into(),
                    instance_id: event["instanceId"].as_str().unwrap_or("").into(),
                    round_id: round,
                    map: event["map"].as_str().unwrap_or("").into(),
                    anchor_clock: clock,
                    window_id: None,
                    clock_from: (clock - 180.).max(0.),
                    clock_to: clock,
                    infantry_kills: 0,
                    kpm180: 0.,
                    unique_victims: 0,
                    headshots: 0,
                    headshot_pct: 0.,
                    penetrations: 0,
                    penetration_pct: 0.,
                    burst_points: 0.,
                    max_kills15s: 0,
                    median_kill_interval: None,
                    weapon_metrics: vec![],
                    reasons: vec![],
                    snapshot_only: Some(true),
                    event_ids: vec![],
                });
                finding.clock_from = (clock - 180.).max(0.);
                finding.clock_to = clock;
                finding.reasons.clear();
                finding.snapshot_only = Some(true);
                if let Some(event_id) = event["eventId"].as_str() {
                    if !finding.event_ids.iter().any(|id| id == event_id) {
                        finding.event_ids.push(event_id.into())
                    }
                }
                findings.push(finding);
            }
        }
        let mut latest: Vec<Finding> = Vec::new();
        let mut indices: HashMap<String, usize> = HashMap::new();
        for f in findings {
            let key = if f.snapshot_only == Some(true) {
                format!("{}:{}:latest-snapshot", f.steam_id, f.round_id)
            } else {
                format!("{}:{}:{}", f.steam_id, f.round_id, f.anchor_clock)
            };
            if let Some(&i) = indices.get(&key) {
                latest[i] = f
            } else {
                indices.insert(key, latest.len());
                latest.push(f)
            }
        }
        let mut alerts = Vec::new();
        let mut candidates = Vec::new();
        let mut refresh = HashSet::new();
        for mut f in latest {
            let now = Utc::now();
            let own = batch.iter().rev().find(|k| {
                k["eventId"]
                    .as_str()
                    .is_some_and(|id| f.event_ids.iter().any(|e| e == id))
            });
            let observed = own.and_then(|k| date(&k["ts"])).unwrap_or(now);
            let recent:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(w) FROM integrity_windows w WHERE org_id=$1 AND steam_id=$2 AND observed_at>=$3 ORDER BY observed_at DESC LIMIT 500").bind(&org).bind(&f.steam_id).bind(now-Duration::milliseconds((num(&config.config,"repeatWindowMinutes")*60000.).max(if config.assessment_mode=="legacy"{0.}else{86400000.}) as i64)).fetch_all(&mut *tx).await?;
            let recent: Vec<_> = recent.into_iter().map(statistics::camel_row).collect();
            if f.window_id.is_none() {
                let saved:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'eventIds',event_ids) FROM integrity_windows WHERE org_id=$1 AND server_id=$2 AND steam_id=$3 AND instance_id=$4 AND map=$5 ORDER BY observed_at DESC").bind(&org).bind(server).bind(&f.steam_id).bind(&f.instance_id).bind(&f.map).fetch_all(&mut *tx).await?;
                f.window_id = saved
                    .iter()
                    .find(|r| context::overlaps(&r["eventIds"], &f.event_ids))
                    .and_then(|r| r["id"].as_i64());
            }
            if let Some(id) = f.window_id {
                let saved: Option<Value> =
                    sqlx::query_scalar("SELECT event_ids FROM integrity_windows WHERE id=$1")
                        .bind(id)
                        .fetch_optional(&mut *tx)
                        .await?;
                if saved
                    .as_ref()
                    .and_then(context::evidence_ids)
                    .is_some_and(|known| f.event_ids.iter().all(|id| known.contains(id)))
                {
                    next.windows.mark_persisted(server, &f, id);
                    continue;
                }
            }
            let episode = json!({"eventIds":f.event_ids,"roundId":f.round_id,"clockFrom":f.clock_from,"observedAt":now});
            let independent: Vec<_> = recent
                .iter()
                .filter(|r| {
                    r["id"].as_i64() != f.window_id
                        && context::independent_episode(r, &episode, 60.)
                })
                .collect();
            let legacy: Vec<_> = independent
                .iter()
                .copied()
                .filter(|r| {
                    date(&r["observedAt"]).is_some_and(|d| {
                        (now - d).num_milliseconds() as f64
                            <= num(&config.config, "repeatWindowMinutes") * 60000.
                    })
                })
                .take(2)
                .collect();
            let mut statistical_prior = Vec::new();
            if config.assessment_mode != "legacy" {
                let ids: Vec<_> = independent
                    .iter()
                    .filter_map(|r| r["id"].as_i64())
                    .collect();
                let scores:Vec<(i64,Option<Value>)>=sqlx::query_as("SELECT window_id,statistical FROM integrity_scores WHERE source='window' AND window_id=ANY($1) ORDER BY id DESC LIMIT 1500").bind(ids).fetch_all(&mut *tx).await?;
                let mut last = HashMap::new();
                for (id, assessment) in scores {
                    last.entry(id).or_insert(assessment.unwrap_or(Value::Null));
                }
                for r in &independent {
                    if !date(&r["observedAt"])
                        .is_some_and(|d| (now - d).num_milliseconds() <= 86400000)
                        || !r["id"]
                            .as_i64()
                            .and_then(|id| last.get(&id))
                            .is_some_and(committee::statistical_anomaly)
                    {
                        continue;
                    }
                    if statistical_prior.iter().any(|newer:&&Value|!context::independent_episode(r,&json!({"eventIds":newer["eventIds"],"roundId":newer["roundId"],"clockFrom":newer["clockFrom"],"observedAt":newer["observedAt"]}),60.)){continue}
                    statistical_prior.push(*r);
                    if statistical_prior.len() == 2 {
                        break;
                    }
                }
            }
            let mut statistical = Value::Null;
            if config.assessment_mode != "legacy" {
                let bucket = statistics::population_at(&state.db, server, observed).await?;
                let (baselines, weapons) =
                    statistics::load(&state.db, &org, &f.map, bucket, Some(server)).await?;
                let present = f.infantry_kills > 0;
                let values = json!({"kpm180":if present{Some(f.kpm180)}else{None},"uniqueVictims":if present{Some(f.unique_victims as f64)}else{None},"maxKills15s":if present{Some(f.max_kills15s as f64)}else{None},"medianKillInterval":f.median_kill_interval,"headshotRate":if present{Some(f.headshots as f64/f.infantry_kills as f64)}else{None},"penetrationRate":if present{Some(f.penetrations as f64/f.infantry_kills as f64)}else{None}});
                statistical = statistics::assess(
                    &values,
                    &baselines,
                    f.infantry_kills,
                    statistical_prior.len() + 1,
                    &f.weapon_metrics,
                    &weapons,
                )
                .map_err(|_| ApiError::bad("Invalid statistical reference."))?;
                let career = if statistical["status"] == "READY" {
                    integrity_career::load(&mut tx, &org, &f.steam_id, observed).await?
                } else {
                    None
                };
                let match_row = f
                    .round_id
                    .rsplit_once(":match:")
                    .and_then(|(_, s)| s.parse::<i64>().ok());
                let (change, precision) = if let Some(match_row) = match_row {
                    let input = context::RoundInput {
                        server,
                        steam: &f.steam_id,
                        instance: &f.instance_id,
                        match_row,
                        clock: f.clock_to,
                        at: observed,
                    };
                    (
                        context::load_change(&mut tx, &input).await?,
                        context::load_precision(&mut tx, &input, &overrides).await?,
                    )
                } else {
                    (Vec::new(), Vec::new())
                };
                statistical["precisionContext"] =
                    json!({"scope":"current_round_weapon_class","rows":precision});
                if change.len() >= 20 || precision.iter().any(|p| num(p, "kills") >= 5.) {
                    statistical["status"] = json!("READY")
                }
                statistical["modelVersion"] = json!(committee::MODEL_VERSION);
                statistical["featureVersion"] = json!(committee::FEATURE_VERSION);
                let metrics = statistical["metrics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                for (field, target) in [
                    ("baselineGeneration", "baselineGeneration"),
                    ("weaponMapVersion", "weaponMapVersion"),
                ] {
                    let values: HashSet<_> = metrics.iter().map(|m| m[field].to_string()).collect();
                    statistical[target] = if values.len() == 1 {
                        metrics[0][field].clone()
                    } else {
                        Value::Null
                    };
                }
                statistical["baselineCalculatedAt"] = metrics
                    .iter()
                    .filter_map(|m| m["calculatedAt"].as_str())
                    .min()
                    .map(|s| json!(s))
                    .unwrap_or(Value::Null);
                statistical["changePointContext"] = json!({"scope":"current_round_all_weapons","bucketSeconds":15,"completedBuckets":change.len(),"roundId":f.round_id});
                statistical["kpmRule"] = json!({"value":f.kpm180,"watchAbove":2,"highAbove":4,"points":if f.kpm180>2.{6}else{0}});
                let episodes = statistical_prior.len()
                    + usize::from(committee::statistical_anomaly(&statistical));
                statistical["independentEpisodes"] = json!(episodes);
                let recent_kpm: Vec<_> = statistical_prior
                    .iter()
                    .rev()
                    .map(|r| num(r, "kpm180"))
                    .chain(std::iter::once(f.kpm180))
                    .collect();
                let career = career
                    .map(|mut career| {
                        career["recentKpm"] = json!(recent_kpm);
                        serde_json::from_value::<committee::Career>(career)
                    })
                    .transpose()
                    .map_err(|_| ApiError::bad("Invalid clean career context."))?;
                let local: Vec<_> = metrics
                    .iter()
                    .filter(|m| statistics::action_eligible(m))
                    .collect();
                let live: Option<Value> =
                    sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=$1")
                        .bind(server)
                        .fetch_optional(&mut *tx)
                        .await?;
                let healthy = live
                    .as_ref()
                    .and_then(|l| date(&l["players_at"]))
                    .is_some_and(|at| (now - at).num_milliseconds() < 300000);
                let quality = committee::Quality {
                    feed_healthy: healthy,
                    backlog_safe: allow_actions,
                    identity_reliable: !f.steam_id.is_empty(),
                    round_reliable: match_row.is_some(),
                    baseline_fresh: !local.is_empty()
                        && local.iter().all(|m| {
                            date(&m["calculatedAt"])
                                .is_some_and(|at| (now - at).num_milliseconds() < 48 * 3600000)
                        }),
                    versions_match: !statistical["baselineGeneration"].is_null()
                        && !statistical["weaponMapVersion"].is_null()
                        && metrics.iter().all(|m| {
                            m["modelVersion"] == committee::MODEL_VERSION
                                && m["featureVersion"] == committee::FEATURE_VERSION
                                && m["weaponMapVersion"] == statistical["weaponMapVersion"]
                                && m["baselineGeneration"] == statistical["baselineGeneration"]
                        }),
                    baseline_population_adequate: !local.is_empty(),
                };
                let precision = serde_json::from_value(json!(precision))
                    .map_err(|_| ApiError::bad("Invalid precision context."))?;
                let result = committee::assess(
                    &committee::Input {
                        statistical: statistical.clone(),
                        precision,
                        independent_episodes: episodes as u64,
                        career,
                        change_series: Some(change),
                        current_kpm: f.kpm180,
                        event_ids: f.event_ids.clone(),
                    },
                    &quality,
                );
                statistical["level"] = json!(match result.decision.as_str() {
                    "KICK_CANDIDATE" => "KICK_CANDIDATE",
                    "CASE" => "CASE",
                    "WATCH" => "WATCH",
                    _ => "NORMAL",
                });
                statistical["committee"] = serde_json::to_value(result).unwrap();
            }
            if !f.reasons.is_empty()
                || ["CASE", "KICK_CANDIDATE"].contains(&statistical["level"].as_str().unwrap_or(""))
            {
                refresh.insert(f.steam_id.clone());
            }
            if f.reasons.is_empty() && !committee::should_save(&statistical) {
                continue;
            }
            let window = save_window(&mut tx, &org, server, &f, now, f.window_id).await?;
            f.window_id = Some(window);
            let repeat:Vec<Value>=sqlx::query_scalar("SELECT w.event_ids FROM integrity_scores s JOIN integrity_windows w ON w.id=s.window_id WHERE s.org_id=$1 AND s.steam_id=$2 AND s.source='window' AND s.level IN ('AUTO_KO','AUTO_QUARANTINE_ELIGIBLE','AUTO_QUARANTINE_24H','AUTO_QUARANTINE_7D') AND s.scored_at>=$3 AND s.scored_at<$4 AND s.window_id<>$5").bind(&org).bind(&f.steam_id).bind(now-Duration::milliseconds((num(&config.config,"repeatKoWindowHours")*3600000.) as i64)).bind(now).bind(window).fetch_all(&mut *tx).await?;
            let repeat = repeat.iter().any(|ids| {
                context::evidence_ids(ids).is_some_and(|ids| {
                    !ids.is_empty() && !ids.iter().any(|id| f.event_ids.contains(id))
                })
            });
            let reporters:i64=sqlx::query_scalar("SELECT count(DISTINCT reporter_steam_id) FROM integrity_reports WHERE org_id=$1 AND target_steam_id=$2 AND created_at>=now()-interval '24 hours'").bind(&org).bind(&f.steam_id).fetch_one(&mut *tx).await?;
            let profile:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(p) FROM steam_profiles p WHERE steam_id=$1 AND error='' AND fetched_at<=now() AND fetched_at>=now()-interval '24 hours'").bind(&f.steam_id).fetch_optional(&mut *tx).await?;
            let steam_known = profile.is_some();
            let profile = profile.unwrap_or(Value::Null);
            let signals = json!({"committeeMode":config.assessment_mode!="legacy","behaviorReasons":f.reasons,"kpm180":f.kpm180,"uniqueVictims":f.unique_victims,"previousKpm":legacy.iter().map(|r|num(r,"kpm180")).collect::<Vec<_>>(),"uniqueReporters":reporters,"repeatHighRiskWindow":repeat,"infantryKills":f.infantry_kills,"headshots":f.headshots,"penetrations":f.penetrations,"burstPoints":f.burst_points,"vacBans":profile["vac_bans"].as_i64().unwrap_or(0),"gameBans":profile["game_bans"].as_i64().unwrap_or(0),"daysSinceLastBan":profile["days_since_last_ban"],"wardogsPlaytimeHours":null});
            let score = serde_json::to_value(integrity_score::score(
                &serde_json::from_value(signals.clone())
                    .map_err(|_| ApiError::bad("Invalid integrity signals."))?,
                &config.config,
            ))
            .unwrap();
            let score_id:i64=sqlx::query_scalar("INSERT INTO integrity_scores(window_id,org_id,server_id,steam_id,scored_at,rule_version,score,level,breakdown,statistical,current_behavior_anomaly) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING id").bind(window).bind(&org).bind(server).bind(&f.steam_id).bind(now).bind(config.version).bind(num(&score,"score")).bind(score["level"].as_str()).bind(&score["breakdown"]).bind(if statistical.is_null(){None}else{Some(&statistical)}).bind(score["currentBehaviorAnomaly"].as_bool().unwrap_or(false)).fetch_one(&mut *tx).await?;
            let legacy_case = config.assessment_mode == "legacy"
                && num(&score, "score") >= num(&config.config, "koThreshold");
            // CASE is an explicit expert review outcome and must not fall through the queue.
            let statistical_case = rules::committee_enabled(&config.assessment_mode)
                && ["WATCH", "CASE", "KICK_CANDIDATE"]
                    .contains(&statistical["level"].as_str().unwrap_or(""));
            let prior: Vec<Value> = if statistical_case {
                sqlx::query_scalar("SELECT to_jsonb(c) FROM integrity_cases c WHERE org_id=$1 AND server_id=$2 AND steam_id=$3 AND statistical IS NOT NULL ORDER BY created_at DESC LIMIT 20").bind(&org).bind(server).bind(&f.steam_id).fetch_all(&mut *tx).await?.into_iter().map(statistics::camel_row).collect()
            } else {
                vec![]
            };
            let reused = prior.iter().find(|c| {
                c["snapshot"]["roundId"] == f.round_id
                    && context::overlaps(&c["snapshot"]["eventIds"], &f.event_ids)
                    && (c["status"] != "OPEN"
                        || (c["ruleVersion"] == config.version
                            && decisions::reuse(&c["statistical"], &statistical)))
                    && c["trigger"] != "AI_PRESCREEN"
                    && c["statistical"]["level"] == statistical["level"]
            });
            let mut case = reused.and_then(|c| c["id"].as_str()).map(str::to_owned);
            if legacy_case || statistical_case && case.is_none() {
                case = Some(
                    integrity_cases::freeze(
                        &mut tx,
                        &integrity_cases::Freeze {
                            org: &org,
                            server,
                            steam: &f.steam_id,
                            finding: &f,
                            score: &score,
                            signals: &signals,
                            steam_known,
                            version: config.version,
                            rules: &config.config,
                            created: now,
                            score_id: Some(score_id),
                            statistical: if statistical.is_null() {
                                None
                            } else {
                                Some(&statistical)
                            },
                            trigger: Some(if !legacy_case && statistical_case {
                                "STATISTICAL_WINDOW"
                            } else {
                                "ABNORMAL_INFANTRY_WINDOW"
                            }),
                        },
                    )
                    .await?,
                );
                sqlx::query("UPDATE integrity_player_careers SET status='FROZEN',updated_at=now() WHERE org_id=$1 AND steam_id=$2").bind(&org).bind(&f.steam_id).execute(&mut *tx).await?;
                if legacy_case || config.assessment_mode == "statistical" {
                    alerts.push(json!({"caseId":case,"serverId":server,"steamId":f.steam_id,"map":f.map,"score":score["score"],"level":score["level"],"statisticalLevel":statistical["level"],"breakdown":score["breakdown"],"infantryKills":f.infantry_kills,"kpm180":f.kpm180,"uniqueVictims":f.unique_victims,"uniqueReporters":reporters,"createdAt":now}));
                }
            }
            if let Some(case) = case.filter(|_| {
                if config.assessment_mode == "statistical" {
                    statistical["level"] == "KICK_CANDIDATE"
                } else {
                    legacy_case
                }
            }) {
                let has_action: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM integrity_actions WHERE case_id=$1)",
                )
                .bind(&case)
                .fetch_one(&mut *tx)
                .await?;
                let last: Option<DateTime<Utc>> = sqlx::query_scalar(
                    "SELECT last_attempt_at FROM integrity_action_eligibility WHERE case_id=$1",
                )
                .bind(&case)
                .fetch_optional(&mut *tx)
                .await?;
                if decisions::retry(
                    &json!({"caseOpen":reused.is_none_or(|c|c["status"]=="OPEN"),"hasAction":has_action,"lastAttemptAt":last,"now":now}),
                ) {
                    sqlx::query("INSERT INTO integrity_action_eligibility(case_id,last_attempt_at) VALUES($1,$2) ON CONFLICT(case_id) DO UPDATE SET last_attempt_at=excluded.last_attempt_at,attempts=integrity_action_eligibility.attempts+1").bind(&case).bind(now).execute(&mut *tx).await?;
                    candidates.push((case, f.clone(), score));
                }
            }
            next.windows.mark_persisted(server, &f, window);
        }
        if !refresh.is_empty() {
            crate::steam::enqueue(&mut tx, &refresh.into_iter().collect::<Vec<_>>())
                .await
                .map_err(|_| ApiError::bad("Unable to queue profile enrichment."))?;
        }
        tx.commit().await?;
        *current = next;
        drop(current);
        let mut actions = Vec::new();
        let live = batch
            .first()
            .and_then(|e| date(&e["ts"]))
            .is_some_and(|d| (Utc::now() - d).num_milliseconds() <= 300000);
        if allow_actions && live {
            for (case, finding, score) in candidates {
                match integrity_enforcement::enforce(
                    state,
                    &Candidate {
                        org: &org,
                        server,
                        steam: &finding.steam_id,
                        case: &case,
                        finding: &finding,
                        score: &score,
                    },
                )
                .await
                {
                    Ok(action) => actions.push(action.into()),
                    Err(_) => {
                        tracing::warn!(server_id=%server,case_id=%case,"Integrity action eligibility failed; assessment is committed")
                    }
                }
            }
        }
        Ok(Outcome { alerts, actions })
    }
}
async fn save_window(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    server: &str,
    f: &Finding,
    at: DateTime<Utc>,
    saved: Option<i64>,
) -> Result<i64> {
    let event_ids = if let Some(id) = saved {
        let known: Value =
            sqlx::query_scalar("SELECT event_ids FROM integrity_windows WHERE id=$1")
                .bind(id)
                .fetch_one(&mut **tx)
                .await?;
        let mut ids = context::evidence_ids(&known).unwrap_or_default();
        for id in &f.event_ids {
            if !ids.contains(id) {
                ids.push(id.clone())
            }
        }
        ids
    } else {
        f.event_ids.clone()
    };
    let row = json!({"id":saved,"org_id":org,"server_id":server,"steam_id":f.steam_id,"instance_id":f.instance_id,"round_id":f.round_id,"map":f.map,"clock_from":f.clock_from,"clock_to":f.clock_to,"observed_at":at,"infantry_kills":f.infantry_kills,"kpm_180":f.kpm180,"unique_victims":f.unique_victims,"headshots":f.headshots,"penetrations":f.penetrations,"burst_points":f.burst_points.round() as i32,"max_kills_15s":f.max_kills15s,"median_kill_interval":f.median_kill_interval,"behavior_reasons":f.reasons,"event_ids":event_ids});
    if let Some(id) = saved {
        sqlx::query("UPDATE integrity_windows w SET round_id=r.round_id,clock_from=r.clock_from,clock_to=r.clock_to,infantry_kills=r.infantry_kills,kpm_180=r.kpm_180,unique_victims=r.unique_victims,headshots=r.headshots,penetrations=r.penetrations,burst_points=r.burst_points,max_kills_15s=r.max_kills_15s,median_kill_interval=r.median_kill_interval,behavior_reasons=r.behavior_reasons,event_ids=r.event_ids FROM jsonb_populate_record(NULL::integrity_windows,$2) r WHERE w.id=$1").bind(id).bind(row).execute(&mut **tx).await?;
        Ok(id)
    } else {
        Ok(sqlx::query_scalar("INSERT INTO integrity_windows(org_id,server_id,steam_id,instance_id,round_id,map,clock_from,clock_to,observed_at,infantry_kills,kpm_180,unique_victims,headshots,penetrations,burst_points,max_kills_15s,median_kill_interval,behavior_reasons,event_ids) SELECT org_id,server_id,steam_id,instance_id,round_id,map,clock_from,clock_to,observed_at,infantry_kills,kpm_180,unique_victims,headshots,penetrations,burst_points,max_kills_15s,median_kill_interval,behavior_reasons,event_ids FROM jsonb_populate_record(NULL::integrity_windows,$1) RETURNING id").bind(row).fetch_one(&mut **tx).await?)
    }
}
pub async fn capture_reports(state: &AppState, server: &str, batch: &[Value]) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let mut tx = state.worker_transaction().await?;
    let reports:Vec<(i64,String,DateTime<Utc>,DateTime<Utc>)>=sqlx::query_as("SELECT id,target_steam_id,evidence_from,evidence_until FROM integrity_reports WHERE server_id=$1 AND evidence_until>=now()-interval '30 seconds' AND created_at<=now()").bind(server).fetch_all(&mut *tx).await?;
    let ids: Vec<_> = batch
        .iter()
        .filter_map(|k| k["eventId"].as_str().map(str::to_owned))
        .collect();
    for (report, steam, from, until) in reports {
        let events:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND ts>=$2 AND ts<=$3 AND (killer_steam_id=$4 OR victim_steam_id=$4) AND event_id=ANY($5) ORDER BY ts DESC LIMIT 1001").bind(server).bind(from).bind(until).bind(steam).bind(&ids).fetch_all(&mut *tx).await?;
        if events.len() > 1000 {
            return Err(ApiError::bad("Report evidence event limit exceeded."));
        }
        for event in events {
            let received = date(&event["ts"]);
            let instance = event["instance_id"].as_str();
            let id = event["event_id"].as_str();
            let frozen = integrity_cases::row_view(event.clone());
            sqlx::query("INSERT INTO integrity_report_events(report_id,instance_id,event_id,received_at,event) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING").bind(report).bind(instance).bind(id).bind(received).bind(frozen).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}
