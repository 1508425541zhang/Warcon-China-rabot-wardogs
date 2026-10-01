//! Independent read-only inference. Action claims are separate, fenced and never blindly retried.
use crate::{config::AppState, error::Result, short_features as features, short_model::Model};
use anyhow::ensure;
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
pub type History = HashMap<String, Vec<Value>>;
const SNAPSHOT: &str = "rust:shortRiskSnapshot";
pub async fn source(app: &AppState) -> anyhow::Result<Value> {
    let mut tx = app.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout='5s'")
        .execute(&mut *tx)
        .await?;
    let rows:Value=sqlx::query_scalar("WITH active AS (SELECT s.id FROM servers s JOIN integrity_rules r ON r.org_id=s.org_id JOIN organizations o ON o.id=s.org_id WHERE r.assessment_mode IN ('short_only','model_only') AND o.suspended_at IS NULL) SELECT jsonb_build_object('now',now(),'live',(SELECT coalesce(jsonb_agg(l),'[]') FROM server_live l JOIN active a ON a.id=l.server_id),'matches',(SELECT coalesce(jsonb_agg(m),'[]') FROM matches m JOIN active a ON a.id=m.server_id WHERE ended_at IS NULL),'events',(SELECT coalesce(jsonb_agg(k),'[]') FROM kills k JOIN active a ON a.id=k.server_id WHERE ts>now()-interval '10 minutes'))").fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(rows)
}
fn healthy(live: &Value, now: f64, max: f64) -> bool {
    live["ok"] == true
        && ["feed_at", "status_at", "players_at"]
            .iter()
            .all(|k| features::utc(&live[*k]).is_ok_and(|v| (0. ..=max).contains(&(now - v))))
}
fn fields(v: &Value) -> [String; 4] {
    [
        v["server_id"].as_str().unwrap_or("").into(),
        v["instance_id"].as_str().unwrap_or("").into(),
        if v["match_row"].is_null() {
            "None".into()
        } else {
            v["match_row"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v["match_row"].to_string())
        },
        v["map"].as_str().unwrap_or("").into(),
    ]
}
pub fn evaluate(model: &Model, data: &Value, history: &mut History) -> anyhow::Result<Value> {
    let now = features::utc(&data["now"])?;
    let mut groups = HashMap::<[String; 4], Vec<Value>>::new();
    for e in data["events"].as_array().into_iter().flatten() {
        groups.entry(fields(e)).or_default().push(e.clone());
    }
    let mut current = HashMap::<String, &Value>::new();
    for m in data["matches"].as_array().into_iter().flatten() {
        if let (Some(s), Some(id)) = (m["server_id"].as_str(), m["id"].as_i64()) {
            if current
                .get(s)
                .is_none_or(|old| old["id"].as_i64().unwrap_or(0) < id)
            {
                current.insert(s.into(), m);
            }
        }
    }
    let mut players = Vec::new();
    let mut seen = HashSet::new();
    for live in data["live"].as_array().into_iter().flatten() {
        let Some(sid) = live["server_id"].as_str() else {
            continue;
        };
        let mid = current.get(sid).map(|m| m["id"].to_string());
        let selected = groups
            .iter()
            .filter(|(scope, _)| scope[0] == sid && Some(&scope[2]) == mid.as_ref())
            .filter_map(|(scope, events)| {
                let latest = events
                    .iter()
                    .filter_map(|e| features::utc(&e["ts"]).ok().map(|t| (t, e)))
                    .max_by(|a, b| a.0.total_cmp(&b.0))?;
                Some((latest.0, scope, events, latest.1))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0));
        for player in live["players"].as_array().into_iter().flatten() {
            let Some(pid) = player["steamId"].as_str() else {
                continue;
            };
            let mut r = json!({"player_id":pid,"server_id":sid,"name":player["name"],"ShortRisk":null,"threshold_score":model.threshold(0.996),"threshold_percentile":99.6,"candidate":false,"kick_executed":false,"execution_status":"handled_by_panel","status":"waiting_for_current_round_events"});
            if !healthy(live, now, 45.) {
                r["status"] = json!("feed_or_state_unhealthy")
            } else if let Some((received, scope, events, latest)) = &selected {
                let elapsed = now - *received;
                if !(0. ..=120.).contains(&elapsed) {
                    r["status"] = json!("game_clock_unavailable")
                } else {
                    let clock = latest["event_time"]
                        .as_f64()
                        .ok_or_else(|| anyhow::anyhow!("Invalid game clock"))?
                        + elapsed;
                    let pred=model.predict(&json!({"events":events,"player_id":pid,"decision_received_utc":now,"decision_game_seconds":clock}))?;
                    r.as_object_mut()
                        .unwrap()
                        .extend(pred.as_object().unwrap().clone());
                    r["scope"] = json!(scope);
                    if !r["ShortRisk"].is_null() {
                        r["candidate"] = r["tail_candidate"].clone();
                        r["reference_status"] = json!("ready");
                    }
                }
            }
            if let (Some(score), Some(scope)) = (
                r["anomaly_score"].as_f64(),
                r["scope"].as_array().filter(|s| s.len() == 4),
            ) {
                let key = json!([sid, pid, scope]).to_string();
                let rows = history.entry(key.clone()).or_default();
                if rows.last().is_some_and(|v| {
                    features::utc(&v["at"]).is_ok_and(|at| now - at > 20. || now <= at)
                }) {
                    rows.clear()
                }
                rows.push(json!({"at":data["now"],"score":score}));
                if rows.len() > 5 {
                    rows.remove(0);
                }
                r["recent_windows"] = json!(rows);
                seen.insert(key);
            }
            players.push(r);
        }
    }
    history.retain(|k, _| seen.contains(k));
    Ok(
        json!({"evaluated_at":data["now"],"mode":"independent_readonly_observer","requested_policy":"warning_at_p996_kick_at_p999","effective_policy":"panel_warning_p996_kick_p999","model_sha256":model.source_model_sha256,"model_readiness":model.readiness,"threshold_meaning":"upper 0.4% of fixed held-out reference scores; not cheating probability","threshold_percentile":99.6,"algorithm":model.algorithm,"reference_samples":model.reference_samples(),"players":players}),
    )
}
pub fn consensus(windows: &Value, end: &Value, kick: f64) -> Option<f64> {
    let rows = windows.as_array()?;
    if rows.len() != 5 {
        return None;
    }
    let end = features::utc(end).ok()?;
    if features::utc(&rows[4]["at"]).ok()? != end {
        return None;
    }
    let mut prior = None;
    let mut total = 0.;
    for (i, w) in rows.iter().enumerate() {
        let at = features::utc(&w["at"]).ok()?;
        let score = w["score"].as_f64().filter(|v| v.is_finite() && *v > kick)?;
        if prior.is_some_and(|p| !(8. ..=20.).contains(&(at - p))) {
            return None;
        }
        prior = Some(at);
        total += score * (i + 1) as f64;
    }
    let weighted = total / 15.;
    (weighted > kick).then_some(weighted)
}
pub async fn actions(app: &AppState, model: &Model, snapshot: &Value) -> Result<()> {
    let now = Utc::now().timestamp_millis() as f64 / 1000.;
    if snapshot["model_sha256"] != model.source_model_sha256
        || snapshot["algorithm"] != "IsolationForest"
        || !features::utc(&snapshot["evaluated_at"])
            .is_ok_and(|at| (0. ..=30.).contains(&(now - at)))
    {
        return Ok(());
    }
    let Some(warning) = model.threshold(0.996) else {
        return Ok(());
    };
    let kick = model.threshold(0.999).unwrap();
    for p in snapshot["players"].as_array().into_iter().flatten() {
        let (Some(sid), Some(pid), Some(score), Some(scope)) = (
            p["server_id"].as_str(),
            p["player_id"].as_str(),
            p["anomaly_score"].as_f64(),
            p["scope"].as_array().filter(|a| a.len() == 4),
        ) else {
            continue;
        };
        if crate::api::notes::steam_id(pid).is_err() || score <= warning {
            continue;
        }
        let key = format!(
            "shortRisk:{sid}:{pid}:{}:{}",
            scope[1].as_str().unwrap_or(""),
            scope[2].as_str().unwrap_or("")
        );
        let _lane = crate::dispatcher::acquire(sid, 1, Duration::from_secs(15)).await?;
        app.runtime.check().await?;
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut tx = app.worker_transaction().await?;
        let row:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('org',s.org_id,'name',s.name,'mode',r.assessment_mode,'minimum',coalesce((r.config->>'minimumOnlineForAutoAction')::int,20),'suspended',r.auto_suspended_at,'hour',r.auto_action_max_per_hour,'percent',r.auto_action_max_percent_online) FROM servers s JOIN organizations o ON o.id=s.org_id JOIN integrity_rules r ON r.org_id=s.org_id WHERE s.id=$1 AND o.suspended_at IS NULL").bind(sid).fetch_optional(&mut *tx).await?;
        let Some(rule) =
            row.filter(|r| matches!(r["mode"].as_str(), Some("short_only" | "model_only")))
        else {
            continue;
        };
        let org = rule["org"].as_str().unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("shortRisk:{org}"))
            .execute(&mut *tx)
            .await?;
        let prior: Option<Value> =
            sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1 FOR UPDATE")
                .bind(&key)
                .fetch_optional(&mut *tx)
                .await?;
        if prior.as_ref().is_some_and(|r| r["attemptedAt"].is_string()) {
            continue;
        }
        let mut record = json!({"serverId":sid,"steamId":pid,"name":p["name"].as_str().unwrap_or(pid),"score":score,"percentile":p["percentile"],"scope":scope,"state":"warning","updatedAt":now,"createdAt":prior.as_ref().and_then(|r|r["createdAt"].as_str()).unwrap_or(&now)});
        let live: Option<Value> =
            sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=$1")
                .bind(sid)
                .fetch_optional(&mut *tx)
                .await?;
        let empty = Value::Null;
        let live = live.as_ref().unwrap_or(&empty);
        let count = live["players"].as_array().map(|p| p.len()).unwrap_or(0);
        let online = live["players"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|p| p["steamId"] == pid));
        let current:Option<i64>=sqlx::query_scalar("SELECT id FROM matches WHERE server_id=$1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1").bind(sid).fetch_optional(&mut *tx).await?;
        let vip: Option<Value> =
            sqlx::query_scalar("SELECT value FROM site_settings WHERE key='qqVip'")
                .fetch_optional(&mut *tx)
                .await?;
        let mut reason = if !healthy(live, features::utc(&json!(now)).unwrap(), 30.) || !online {
            "玩家离线或数据不健康"
        } else if current.map(|m| m.to_string()).as_deref() != scope[2].as_str() {
            "对局已变化"
        } else if count < (rule["minimum"].as_u64().unwrap_or(20) as usize) {
            "人数不足"
        } else if !rule["suspended"].is_null() {
            "自动处罚已暂停"
        } else if vip
            .as_ref()
            .and_then(|v| v["entries"].as_array())
            .is_some_and(|a| {
                a.iter().any(|v| {
                    v["serverId"] == sid
                        && v["steamId"] == pid
                        && v["enabled"] == true
                        && v["whitelist"] == true
                })
            })
        {
            "VIP 白名单"
        } else {
            ""
        };
        let actions:i64=sqlx::query_scalar("SELECT count(*) FROM site_settings WHERE key LIKE 'shortRisk:%' AND value->>'serverId' IN(SELECT id FROM servers WHERE org_id=$1) AND (value->>'attemptedAt')::timestamptz>now()-interval '1 hour'").bind(org).fetch_one(&mut *tx).await?;
        let cap = rule["hour"].as_i64().unwrap_or(10).min(
            ((count as f64 * rule["percent"].as_f64().unwrap_or(10.) / 100.).floor() as i64).max(1),
        );
        if actions >= cap {
            reason = "达到自动处罚频率上限"
        }
        let weighted = consensus(&p["recent_windows"], &snapshot["evaluated_at"], kick);
        let should_kick = weighted.is_some() && reason.is_empty();
        record["windowScores"] = p["recent_windows"].clone();
        if let Some(score) = weighted {
            record["weightedScore"] = json!(score)
        }
        record["reason"] = json!(if !reason.is_empty() {
            reason
        } else if should_kick {
            "连续五窗均超过 P99.9，按 1:2:3:4:5 加权确认踢出"
        } else {
            "P99.6 警惕；等待连续五窗 P99.9 加权确认"
        });
        if should_kick {
            record["state"] = json!("pending");
            record["attemptedAt"] = json!(now)
        }
        sqlx::query("INSERT INTO site_settings(key,value,updated_at) VALUES($1,$2,now()) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=now()").bind(&key).bind(&record).execute(&mut *tx).await?;
        tx.commit().await?;
        if !should_kick {
            continue;
        }
        // Recheck persisted mode after claiming and immediately before the network request.
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_rules WHERE org_id=$1 AND assessment_mode IN ('short_only','model_only') AND auto_suspended_at IS NULL)").bind(org).fetch_one(&app.db).await?;
        let result = if active {
            crate::gateway::run_held(app,sid,"kick",&json!({"steamId":pid,"reason":"短窗连续五窗均超过 P99.9，加权确认异常，请联系管理员复核。"}),1,None).await
        } else {
            Err(crate::game::Error::Api(crate::error::ApiError::forbidden()))
        };
        record["state"] = json!(if result.is_ok() {
            "delivered"
        } else {
            "unknown"
        });
        record["reason"] = json!(if result.is_ok() {
            "连续五窗 P99.9 加权确认；游戏服务器已接受踢出请求"
        } else {
            "请求未确认，保留记录且不自动重发"
        });
        let mut tx = app.worker_transaction().await?;
        sqlx::query("UPDATE site_settings SET value=$2,updated_at=now() WHERE key=$1")
            .bind(&key)
            .bind(&record)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO audit_log(org_id,server_id,server_name,actor_name,category,action,target,outcome,message,detail) VALUES($1,$2,$3,'无监督短窗 P99.9','trigger','shortRisk.kick',$4,$5,$6,$7)").bind(org).bind(sid).bind(rule["name"].as_str()).bind(pid).bind(if result.is_ok(){"ok"}else{"error"}).bind(record["reason"].as_str()).bind(&record).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(())
}
pub async fn tick(
    app: &AppState,
    model: Arc<Model>,
    history: &mut History,
    execute: bool,
) -> anyhow::Result<Value> {
    app.runtime.check().await?;
    let data = source(app).await?;
    let snapshot = if data["live"].as_array().is_none_or(|v| v.is_empty()) {
        history.clear();
        json!({"mode":"inactive","status":"engine_not_selected","evaluated_at":data["now"],"players":[],"kick_executed":false})
    } else {
        let mut owned = std::mem::take(history);
        let cloned = model.clone();
        let (snapshot, next) = tokio::task::spawn_blocking(move || {
            let result = evaluate(&cloned, &data, &mut owned);
            (result, owned)
        })
        .await?;
        *history = next;
        snapshot?
    };
    let mut tx = app.worker_transaction().await?;
    sqlx::query("INSERT INTO site_settings(key,value,updated_at) VALUES($1,$2,now()) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=now()").bind(SNAPSHOT).bind(&snapshot).execute(&mut *tx).await?;
    tx.commit().await?;
    if execute {
        actions(app, &model, &snapshot).await?
    }
    Ok(snapshot)
}
pub async fn run(app: AppState, model: Arc<Model>, execute: bool) -> anyhow::Result<()> {
    ensure!(
        model.algorithm == "IsolationForest",
        "Automatic short observer requires the calibrated reference rank contract"
    );
    let mut history = History::new();
    let mut clock = tokio::time::interval(Duration::from_secs(10));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=app.runtime.stop.cancelled()=>break,_=clock.tick()=>{if tick(&app,model.clone(),&mut history,execute).await.is_err(){history.clear();tracing::warn!("Native short window pass unavailable");}}}
    }
    Ok(())
}
