//! Restore unauthorized team changes with round protection and durable permits.
use crate::{
    actions,
    config::AppState,
    error::Result,
    game::{Client, Error},
    game_automation as auto,
    integrity_enforcement::date,
    observation_state::PresenceDiff,
    observer::Memory,
};
use chrono::Utc;
use serde_json::{Value, json};
pub fn decision(
    from: Option<&str>,
    to: Option<&str>,
    teams: &[String],
    authorized: bool,
    transition: bool,
    full: Option<bool>,
    leading: Option<bool>,
) -> &'static str {
    if from.is_none_or(str::is_empty)
        || to.is_none_or(str::is_empty)
        || !teams.iter().any(|s| Some(s.as_str()) == from)
        || !teams.iter().any(|s| Some(s.as_str()) == to)
    {
        "INITIAL_OR_UNASSIGNED"
    } else if from == to {
        "UNCHANGED"
    } else if authorized {
        "AUTHORIZED_MOVE"
    } else if transition {
        "ROUND_GRACE"
    } else if full == Some(true) {
        match leading {
            Some(true) => "WARN_KICK",
            Some(false) => "FULL_NOT_LEADING",
            None => "UNKNOWN_SCORE",
        }
    } else {
        "RESTORE"
    }
}
pub fn leading(scores: &[Value], faction: &str) -> Option<bool> {
    let own = scores.iter().find(|s| s["name"] == faction)?["score"].as_f64()?;
    if scores.len() < 2
        || scores
            .iter()
            .any(|s| s["score"].as_f64().is_none_or(|n| !n.is_finite()))
    {
        None
    } else {
        Some(
            scores
                .iter()
                .filter(|s| s["name"] != faction)
                .all(|s| own > s["score"].as_f64().unwrap()),
        )
    }
}
async fn finish(
    state: &AppState,
    m: &Memory,
    event: &Value,
    outcome: &str,
    reason: &str,
) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE faction_lock_events SET state=$2,reason=$3,updated_at=now()WHERE id=$1")
        .bind(event["id"].as_i64())
        .bind(outcome)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    auto::audit(&mut tx,m,&format!("faction_lock.{outcome}"),event["steam_id"].as_str().unwrap(),outcome,&json!({"from":event["from_faction"],"to":event["to_faction"],"incident":event["id"],"reason":reason})).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn run(
    state: &AppState,
    client: &Client,
    m: &Memory,
    diff: &PresenceDiff,
    trusted: bool,
    boundary: bool,
) -> Result<()> {
    state.runtime.check().await?;
    let rule: Option<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(r)FROM faction_lock_rules r WHERE server_id=$1 AND enabled",
    )
    .bind(&m.id)
    .fetch_optional(&state.db)
    .await?;
    let Some(rule) = rule else { return Ok(()) };
    let round:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(r)FROM matches r WHERE server_id=$1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1").bind(&m.id).fetch_optional(&state.db).await?;
    let Some(round) = round else { return Ok(()) };
    let mid = round["id"].as_i64().unwrap();
    let now = Utc::now();
    let grace = rule["grace_seconds"].as_i64().unwrap_or(120);
    let transition = boundary
        || !trusted
        || (now - date(&round["started_at"]).unwrap()).num_seconds() < grace
        || now.timestamp_millis() - m.started_at < grace * 1000
        || round["map"] != m.status["map"]
        || m.status["matchSeconds"]
            .as_f64()
            .is_some_and(|n| n < grace as f64)
        || auto::ended(&m.status);
    let teams: Vec<_> = m.status["scores"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["name"].as_str())
        .map(str::to_owned)
        .collect();
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE faction_lock_events SET state='error',reason='执行中断，结果未知；停止自动重试',updated_at=now()WHERE server_id=$1 AND state='executing' AND updated_at<now()-interval '120 seconds'").bind(&m.id).execute(&mut *tx).await?;
    let permits: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(p)FROM faction_move_permits p WHERE server_id=$1 AND expires_at>=$2",
    )
    .bind(&m.id)
    .bind(now)
    .fetch_all(&mut *tx)
    .await?;
    let pending:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e)FROM faction_lock_events e WHERE server_id=$1 AND state IN('pending','warned')ORDER BY id").bind(&m.id).fetch_all(&mut *tx).await?;
    let authorized = |steam: &str, to: &str| {
        permits
            .iter()
            .any(|p| p["steam_id"] == steam && p["faction"] == to)
    };
    for change in &diff.factioned {
        let player = &change["player"];
        let steam = player["steamId"].as_str().unwrap_or("");
        if pending.iter().any(|e| e["steam_id"] == steam) {
            continue;
        }
        let from = change["from"].as_str();
        let to = player["faction"].as_str();
        let outcome = decision(
            from,
            to,
            &teams,
            authorized(steam, to.unwrap_or("")),
            transition,
            None,
            None,
        );
        if outcome == "UNCHANGED" {
            continue;
        }
        sqlx::query("INSERT INTO faction_lock_events(server_id,match_id,steam_id,from_faction,to_faction,state,reason,created_at,updated_at)VALUES($1,$2,$3,$4,$5,$6,$7,$8,$8)").bind(&m.id).bind(mid).bind(steam).bind(from.unwrap_or("")).bind(to.unwrap_or("")).bind(if outcome=="RESTORE"{"pending"}else{"skipped"}).bind(outcome).bind(now).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM faction_move_permits WHERE server_id=$1 AND expires_at<now()-interval '24 hours'").bind(&m.id).execute(&mut *tx).await?;
    tx.commit().await?;
    for event in pending {
        let steam = event["steam_id"].as_str().unwrap();
        let from = event["from_faction"].as_str().unwrap();
        let to = event["to_faction"].as_str().unwrap();
        let player = m.players.iter().find(|p| p.steam_id == steam);
        if player.is_none_or(|p| p.faction.as_deref() != Some(to))
            || event["match_id"] != mid
            || transition
            || authorized(steam, to)
        {
            finish(
                state,
                m,
                &event,
                "skipped",
                "玩家离线、已变更阵营、处于换图保护或存在面板调队记录",
            )
            .await?;
            continue;
        }
        if (now - date(&event["created_at"]).unwrap()).num_milliseconds() > 120000 {
            finish(state, m, &event, "skipped", "事件过期，停止执行").await?;
            continue;
        }
        if (now - date(&event["updated_at"]).unwrap()).num_milliseconds() < 8000
            || !auto::fresh(m, 10000)
        {
            continue;
        }
        let mut tx = state.worker_transaction().await?;
        let active: Option<Value> = sqlx::query_scalar(
            "SELECT to_jsonb(r)FROM faction_lock_rules r WHERE server_id=$1 FOR SHARE",
        )
        .bind(&m.id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(active) = active.filter(|r| r["enabled"] == true) else {
            return Ok(());
        };
        let permit:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(p)FROM faction_move_permits p WHERE server_id=$1 AND steam_id=$2 AND created_at>=$3 AND expires_at>=now() ORDER BY created_at DESC LIMIT 1").bind(&m.id).bind(steam).bind(date(&event["created_at"])).fetch_optional(&mut *tx).await?;
        if permit.as_ref().is_some_and(|p| p["faction"] != from) {
            tx.rollback().await?;
            finish(state, m, &event, "skipped", "新的面板调队操作").await?;
            continue;
        }
        let claimed=sqlx::query("UPDATE faction_lock_events SET state='executing',updated_at=now()WHERE id=$1 AND state=$2").bind(event["id"].as_i64()).bind(event["state"].as_str()).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        if claimed == 0 {
            continue;
        }
        state.runtime.check().await?;
        let cap = active["capacities"][from].as_u64().filter(|n| *n > 0);
        let mut full = cap.is_some_and(|cap| {
            m.players
                .iter()
                .filter(|p| p.faction.as_deref() == Some(from))
                .count()
                >= cap as usize
        });
        if !full {
            match actions::run(
                client,
                "changeTeam",
                &json!({"steamId":steam,"faction":from}),
            )
            .await
            {
                Ok(_) => {
                    finish(state, m, &event, "restored", "已调回原阵营").await?;
                    continue;
                }
                Err(Error::Game(e)) if ["faction_full", "team_full"].contains(&e.code.as_str()) => {
                    full = true
                }
                Err(_) => {
                    finish(
                        state,
                        m,
                        &event,
                        "error",
                        "操作失败或结果未知；停止自动重试",
                    )
                    .await?;
                    continue;
                }
            }
        }
        let leading = leading(
            m.status["scores"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            from,
        );
        if !full || leading != Some(true) {
            finish(
                state,
                m,
                &event,
                "skipped",
                if leading == Some(false) {
                    "原阵营已满但未领先，保持当前阵营"
                } else {
                    "原阵营分数不可用，不踢出"
                },
            )
            .await?;
            continue;
        }
        if !auto::fresh(m, 10000) {
            finish(state, m, &event, "error", "状态已过期，停止自动执行").await?;
            continue;
        }
        let kicked = event["state"] == "warned";
        let params = if kicked {
            json!({"steamId":steam,"reason":"禁止自行换边：原阵营已满且领先，无法调回。"})
        } else {
            json!({"steamId":steam,"message":"禁止自行换边：原阵营已满且领先，无法调回。8秒后仍符合条件将踢出。"})
        };
        if actions::run(client, if kicked { "kick" } else { "whisper" }, &params)
            .await
            .is_ok()
        {
            finish(
                state,
                m,
                &event,
                if kicked { "kicked" } else { "warned" },
                if kicked {
                    "警告后重新确认原阵营已满且领先，已踢出"
                } else {
                    "警告已发送，等待重新检查后踢出"
                },
            )
            .await?
        } else {
            finish(
                state,
                m,
                &event,
                "error",
                "操作失败或结果未知；停止自动重试",
            )
            .await?
        }
    }
    Ok(())
}
