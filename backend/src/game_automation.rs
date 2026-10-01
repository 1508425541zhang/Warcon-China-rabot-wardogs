//! Automations execute in the observer's held game lane. Commit sending records
//! before HTTP; ambiguous delivery is terminal, never a confirmed warning.
use crate::{
    actions,
    automation_policy::{LimitObservation, NumericLimits, numeric_breaches},
    config::AppState,
    error::{ApiError, Result},
    game::Client,
    integrity_enforcement::date,
    observer::Memory,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use std::collections::{HashMap, HashSet};
pub fn version(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}
pub fn ended(status: &Value) -> bool {
    status["scoreCap"].as_f64().is_some_and(|cap| {
        status["scores"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|s| s["score"].as_f64().is_some_and(|score| score >= cap))
    })
}
pub fn fresh(m: &Memory, maximum: i64) -> bool {
    (0..=maximum).contains(&(Utc::now().timestamp_millis() - m.status_at))
}
pub async fn round(state: &AppState, m: &Memory) -> Result<Option<Value>> {
    let row:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(r) FROM matches r WHERE server_id=$1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1").bind(&m.id).fetch_optional(&state.db).await?;
    Ok(row.filter(|r| {
        r["map"].as_str().is_some_and(|map| {
            crate::feed::map_id(map) == crate::feed::map_id(m.status["map"].as_str().unwrap_or(""))
        }) && !ended(&m.status)
    }))
}
pub async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    m: &Memory,
    action: &str,
    steam: &str,
    state: &str,
    detail: &Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(server_id,server_name,org_id,category,action,target,outcome,detail)VALUES($1,$2,$3,'trigger',$4,$5,$6,$7)").bind(&m.id).bind(&m.name).bind(&m.org).bind(action).bind(steam).bind(if state=="delivered"||["restored","warned","kicked","skipped"].contains(&state){"ok"}else{"error"}).bind(crate::audit::redact(detail,0)).execute(&mut **tx).await?;
    Ok(())
}
async fn vip_metrics(db: &mut sqlx::PgConnection, server: &str, steam: &str) -> Result<Value> {
    Ok(sqlx::query_scalar::<_,Value>("SELECT coalesce((SELECT v FROM site_settings s,LATERAL jsonb_array_elements(s.value->'entries')v WHERE s.key='qqVip' AND v->>'serverId'=$1 AND v->>'steamId'=$2 AND v->'enabled'='true'::jsonb LIMIT 1),'{}'::jsonb)").bind(server).bind(steam).fetch_one(db).await?)
}
fn permits_metric(vip: &Value, metric: &str) -> bool {
    vip["whitelist"] != true && !(vip["allowOverkill"] == true && ["kpm", "kd"].contains(&metric))
}
pub async fn numeric(
    state: &AppState,
    client: &Client,
    m: &Memory,
    trusted: bool,
    boundary: bool,
) -> Result<()> {
    if !trusted || boundary || !fresh(m, 30000) {
        return Ok(());
    }
    state.runtime.check().await?;
    let rule: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM numeric_limit_rules r WHERE server_id=$1")
            .bind(&m.id)
            .fetch_optional(&state.db)
            .await?;
    let Some(rule) = rule else { return Ok(()) };
    let Some(config) = NumericLimits::parse(&rule["config"]).filter(|c| c.enabled) else {
        return Ok(());
    };
    let rule_at =
        date(&rule["updated_at"]).ok_or_else(|| ApiError::bad("Invalid numeric rule revision"))?;
    let revision = version(rule_at);
    let Some(round) = round(state, m).await? else {
        return Ok(());
    };
    let mid = round["id"].as_i64().unwrap();
    let now = Utc::now();
    let since = now - chrono::Duration::seconds(config.window_seconds + 60);
    let samples:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(p)FROM player_progress_samples p WHERE server_id=$1 AND match_id=$2 AND observed_at>=$3 ORDER BY observed_at").bind(&m.id).bind(mid).bind(since).fetch_all(&state.db).await?;
    let sessions: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT steam_id,joined_at FROM player_sessions WHERE server_id=$1 AND left_at IS NULL",
    )
    .bind(&m.id)
    .fetch_all(&state.db)
    .await?;
    for player in &m.players {
        let Some((_, joined)) = sessions.iter().find(|s| s.0 == player.steam_id) else {
            continue;
        };
        if (now - *joined).num_seconds() < config.window_seconds {
            continue;
        }
        let eligible: Vec<_> = samples
            .iter()
            .filter(|s| {
                date(&s["observed_at"]).is_some_and(|t| t >= *joined && t >= rule_at && t < now)
            })
            .collect();
        let Some(first) = eligible.iter().rposition(|s| {
            date(&s["observed_at"]).unwrap().timestamp_millis()
                <= now.timestamp_millis() - config.window_seconds * 1000
        }) else {
            continue;
        };
        let mut points: Vec<_> = eligible[first..]
            .iter()
            .map(|s| {
                let p = s["players"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| p["steamId"] == player.steam_id);
                LimitObservation {
                    at: date(&s["observed_at"]).unwrap().timestamp_millis() as f64,
                    kills: p.and_then(|p| p["kills"].as_f64()).unwrap_or(f64::NAN),
                    deaths: p.and_then(|p| p["deaths"].as_f64()).unwrap_or(f64::NAN),
                    cash: p.and_then(|p| p["cash"].as_f64()).unwrap_or(f64::NAN),
                }
            })
            .collect();
        points.push(LimitObservation {
            at: now.timestamp_millis() as f64,
            kills: player.kills as f64,
            deaths: player.deaths as f64,
            cash: player.cash as f64,
        });
        let mut tx = state.worker_transaction().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("{}:numeric:{}", m.id, player.steam_id))
            .execute(&mut *tx)
            .await?;
        let active: Option<Value> = sqlx::query_scalar(
            "SELECT to_jsonb(r)FROM numeric_limit_rules r WHERE server_id=$1 FOR SHARE",
        )
        .bind(&m.id)
        .fetch_optional(&mut *tx)
        .await?;
        if active.as_ref().is_none_or(|r| {
            date(&r["updated_at"]) != Some(rule_at) || r["config"]["enabled"] != true
        }) {
            continue;
        }
        let vip = vip_metrics(&mut tx, &m.id, &player.steam_id).await?;
        let breaches: Vec<_> = numeric_breaches(&config, &points)
            .into_iter()
            .filter(|b| permits_metric(&vip, b.metric))
            .collect();
        if breaches.is_empty() {
            continue;
        }
        let prior:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e)FROM numeric_limit_events e WHERE server_id=$1 AND match_id=$2 AND steam_id=$3 AND rule_version=$4 ORDER BY created_at DESC").bind(&m.id).bind(mid).bind(&player.steam_id).bind(&revision).fetch_all(&mut *tx).await?;
        if prior.iter().any(|e| {
            ["sending", "unknown"].contains(&e["state"].as_str().unwrap_or(""))
                || e["action"] == "kick"
        }) {
            continue;
        }
        let warning = prior
            .iter()
            .find(|e| e["action"] == "warn" && e["state"] == "delivered");
        if warning.is_some_and(|w| {
            points[0].at <= date(&w["updated_at"]).unwrap().timestamp_millis() as f64
                || player.kills as f64 <= w["evidence"]["kills"].as_f64().unwrap_or(0.)
                    && breaches.iter().all(|b| b.metric == "kd")
        }) {
            continue;
        }
        let action = if warning.is_some() { "kick" } else { "warn" };
        let id = uuid::Uuid::new_v4().to_string();
        let evidence = json!({"name":player.name,"kills":player.kills,"from":points[0].at,"to":now.timestamp_millis(),"breaches":breaches});
        sqlx::query("INSERT INTO numeric_limit_events(id,server_id,match_id,steam_id,rule_version,action,state,evidence,created_at,updated_at)VALUES($1,$2,$3,$4,$5,$6,'sending',$7,$8,$8)").bind(&id).bind(&m.id).bind(mid).bind(&player.steam_id).bind(&revision).bind(action).bind(&evidence).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        let mut tx = state.worker_transaction().await?;
        let current: Option<Value> = sqlx::query_scalar(
            "SELECT to_jsonb(r)FROM numeric_limit_rules r WHERE server_id=$1 FOR SHARE",
        )
        .bind(&m.id)
        .fetch_optional(&mut *tx)
        .await?;
        let vip = vip_metrics(&mut tx, &m.id, &player.steam_id).await?;
        let valid = current.as_ref().is_some_and(|r| {
            date(&r["updated_at"]) == Some(rule_at) && r["config"]["enabled"] == true
        }) && fresh(m, 30000)
            && breaches.iter().all(|b| permits_metric(&vip, b.metric));
        tx.commit().await?;
        let outcome = if !valid {
            "skipped"
        } else {
            state.runtime.check().await?;
            let label = breaches
                .iter()
                .map(|b| {
                    format!(
                        "{} {:.2}>{}",
                        if b.metric == "cash" {
                            "金钱净增长/分钟"
                        } else {
                            if b.metric == "kd" { "KD" } else { "KPM" }
                        },
                        b.value,
                        b.limit
                    )
                })
                .collect::<Vec<_>>()
                .join("；");
            let params = if action == "warn" {
                json!({"steamId":player.steam_id,"message":format!("数值限制警告：{label}。再经过完整{}秒窗口仍超限将踢出。",config.window_seconds)})
            } else {
                json!({"steamId":player.steam_id,"reason":format!("数值限制：警告后再次超限。{label}")})
            };
            if actions::run(
                client,
                if action == "warn" { "whisper" } else { "kick" },
                &params,
            )
            .await
            .is_ok()
            {
                "delivered"
            } else {
                "unknown"
            }
        };
        let mut tx = state.worker_transaction().await?;
        sqlx::query("UPDATE numeric_limit_events SET state=$2,updated_at=now()WHERE id=$1 AND state='sending'").bind(&id).bind(outcome).execute(&mut *tx).await?;
        audit(
            &mut tx,
            m,
            &format!("numeric_limit.{action}"),
            &player.steam_id,
            outcome,
            &json!({"eventId":id,"breaches":breaches,"state":outcome}),
        )
        .await?;
        tx.commit().await?;
    }
    Ok(())
}
pub fn restricted(cause: &str, rule: &Value) -> bool {
    let lower = cause.to_ascii_lowercase();
    !cause.is_empty()
        && (rule["causes"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(cause)))
            || rule["groups"].as_array().is_some_and(|a| {
                a.contains(&json!("items")) && lower.starts_with("id.item.")
                    || a.contains(&json!("vehicles"))
                        && (lower.starts_with("vehicle.") || lower.starts_with("id.vehicle."))
                    || a.contains(&json!("buildables")) && lower.starts_with("id.buildable.")
            }))
}
pub fn restriction_stage(event: f64, now: f64, last: Option<f64>) -> bool {
    event.is_finite()
        && now.is_finite()
        && event <= now + 5.
        && event >= now - 60.
        && last.is_none_or(|last| event >= last + 60.)
}
pub async fn weapons(state: &AppState, client: &Client, m: &Memory, boundary: bool) -> Result<()> {
    if boundary || !fresh(m, 30000) || m.status["matchSeconds"].as_f64().is_none() {
        return Ok(());
    }
    state.runtime.check().await?;
    let rule: Option<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(r)FROM weapon_restriction_rules r WHERE server_id=$1 AND enabled",
    )
    .bind(&m.id)
    .fetch_optional(&state.db)
    .await?;
    let Some(rule) = rule else { return Ok(()) };
    let at = date(&rule["updated_at"]).unwrap();
    let revision = version(at);
    let Some(round) = round(state, m).await? else {
        return Ok(());
    };
    let mid = round["id"].as_i64().unwrap();
    let now = Utc::now();
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE weapon_restriction_events SET state='error',reason='执行中断，结果未知；不会自动补踢',updated_at=now()WHERE server_id=$1 AND state='executing' AND updated_at<now()-interval '120 seconds'").bind(&m.id).execute(&mut *tx).await?;
    tx.commit().await?;
    let kills:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k)FROM kills k WHERE server_id=$1 AND match_row=$2 AND ts>=$3 ORDER BY ts,event_time,event_id LIMIT 2001").bind(&m.id).bind(mid).bind(at.max(now-chrono::Duration::seconds(60))).fetch_all(&state.db).await?;
    if kills.len() > 2000 {
        return Ok(());
    }
    let online: HashMap<_, _> = m.players.iter().map(|p| (&p.steam_id, p)).collect();
    let mut acted = HashSet::new();
    for kill in kills {
        let Some(steam) = kill["killer_steam_id"]
            .as_str()
            .filter(|s| crate::api::notes::steam_id(s).is_ok())
        else {
            continue;
        };
        let Some(player) = online.get(&steam.to_owned()) else {
            continue;
        };
        let cause = kill["cause"].as_str().unwrap_or("");
        if kill["suicide"] == true
            || kill["victim_steam_id"] == steam
            || acted.contains(steam)
            || crate::feed::map_id(kill["map"].as_str().unwrap_or(""))
                != crate::feed::map_id(m.status["map"].as_str().unwrap_or(""))
            || !restricted(cause, &rule)
        {
            continue;
        }
        let id = crate::crypto::hash_token(
            &json!([m.id, revision, kill["instance_id"], kill["event_id"]]).to_string(),
        );
        let clock = m.status["matchSeconds"].as_f64().unwrap()
            + (Utc::now().timestamp_millis() - m.status_at).max(0) as f64 / 1000.;
        let mut tx = state.worker_transaction().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("{}:weapon:{steam}", m.id))
            .execute(&mut *tx)
            .await?;
        let active: Option<Value> = sqlx::query_scalar(
            "SELECT to_jsonb(r)FROM weapon_restriction_rules r WHERE server_id=$1 FOR SHARE",
        )
        .bind(&m.id)
        .fetch_optional(&mut *tx)
        .await?;
        if active
            .as_ref()
            .is_none_or(|r| r["enabled"] != true || date(&r["updated_at"]) != Some(at))
        {
            continue;
        }
        let prior:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e)FROM weapon_restriction_events e WHERE server_id=$1 AND match_id=$2 AND steam_id=$3 AND rule_version=$4 ORDER BY created_at DESC").bind(&m.id).bind(mid).bind(steam).bind(&revision).fetch_all(&mut *tx).await?;
        if prior
            .iter()
            .any(|e| e["id"] == id || e["state"] == "executing")
        {
            continue;
        }
        let last = prior
            .iter()
            .find(|e| e["action"] == "kick" && e["state"] == "delivered");
        if last.is_some_and(|e| date(&kill["ts"]) <= date(&e["updated_at"]))
            || !restriction_stage(
                kill["event_time"].as_f64().unwrap_or(f64::NAN),
                clock,
                last.and_then(|e| e["clock"].as_f64()),
            )
        {
            continue;
        }
        let count=sqlx::query("INSERT INTO weapon_restriction_events(id,server_id,match_id,rule_version,steam_id,player_name,cause,event_id,action,state,reason,clock,created_at,updated_at)VALUES($1,$2,$3,$4,$5,$6,$7,$8,'kick','executing','等待执行',$9,$10,$10)ON CONFLICT DO NOTHING").bind(&id).bind(&m.id).bind(mid).bind(&revision).bind(steam).bind(&player.name).bind(cause).bind(&kill["event_id"].as_str().unwrap()).bind(clock as f32).bind(now).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        if count == 0 {
            continue;
        }
        acted.insert(steam.to_owned());
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM weapon_restriction_rules WHERE server_id=$1 AND enabled AND updated_at=$2)").bind(&m.id).bind(at).fetch_one(&state.db).await?;
        let (outcome, reason) = if !valid || !fresh(m, 30000) {
            ("skipped", "规则已变更或数据已过期")
        } else {
            state.runtime.check().await?;
            let label = crate::feed::truncate(cause, 90);
            if actions::run(client,"kick",&json!({"steamId":steam,"reason":format!("武器限制：使用 {label} 造成击杀，直接踢出。")})).await.is_ok(){("delivered","使用受限来源造成击杀，已直接踢出")}else{("error","游戏接口执行失败或结果未知；不会视为踢出成功，不自动重试本事件")}
        };
        let mut tx = state.worker_transaction().await?;
        sqlx::query("UPDATE weapon_restriction_events SET state=$2,reason=$3,updated_at=now(),clock=CASE WHEN $2='delivered' THEN $4 ELSE clock END WHERE id=$1 AND state='executing'").bind(&id).bind(outcome).bind(reason).bind((m.status["matchSeconds"].as_f64().unwrap()+(Utc::now().timestamp_millis()-m.status_at).max(0)as f64/1000.)as f32).execute(&mut *tx).await?;
        audit(
            &mut tx,
            m,
            "weapon_restriction.kick",
            steam,
            outcome,
            &json!({"cause":cause,"eventId":kill["event_id"],"reason":reason}),
        )
        .await?;
        tx.commit().await?;
    }
    Ok(())
}
