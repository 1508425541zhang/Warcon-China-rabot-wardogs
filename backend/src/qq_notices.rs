//! Only confirmed RCON kicks produce QQ punishment announcements.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    qq_config, qq_transport,
    webhooks::{clip, text},
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
fn clean(v: &str, n: usize) -> String {
    clip(
        &v.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>(),
        n,
    )
}
pub async fn enqueue(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    row: &Value,
) -> Result<()> {
    if row["action"] != "kick"
        || row["steamId"].is_null()
        || !["integrity", "model_integrity"].contains(&text(&row["triggerKind"]))
    {
        return Ok(());
    }
    let c = qq_config::load_conn(state, tx).await?;
    let server = text(&row["serverId"]);
    let steam = text(&row["steamId"]);
    let Some(policy) = c.policy(server).filter(|p| p.anti_cheat_notices) else {
        return Ok(());
    };
    if !c.active() {
        return Ok(());
    }
    let confirmed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM outbox WHERE id=$1 AND server_id=$2 AND steam_id=$3 AND trigger_kind=$4 AND action='kick' AND state='delivered')").bind(row["id"].as_i64()).bind(server).bind(steam).bind(text(&row["triggerKind"])).fetch_one(&mut **tx).await?;
    if !confirmed {
        return Ok(());
    }
    let detail = &row["detail"];
    let (source, tier, reference, identity) = if row["triggerKind"] == "model_integrity" {
        let run:Value=sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_model_runs r WHERE id=$1 AND server_id=$2 AND steam_id=$3 AND state='READY' AND action_state='delivered' AND action IN ('KICK','QUARANTINE_24H')").bind(detail["runId"].as_str()).bind(server).bind(steam).fetch_optional(&mut **tx).await?.ok_or_else(||ApiError::bad("已确认模型处罚记录不存在。"))?;
        let calibration = if detail["modelCalibration"].is_object() {
            detail["modelCalibration"].clone()
        } else if run["result"]["calibrationSha256"]
            == crate::model_http::manifest()["calibration_sha256"]
        {
            crate::model_http::calibration()
        } else {
            Value::Null
        };
        let tier = if run["action"] == "QUARANTINE_24H" {
            "P99 · 隔离24小时并踢出"
        } else if calibration["p98"].as_f64() == run["threshold"].as_f64() {
            "P98 · 自动踢出"
        } else if calibration["p97"].as_f64() == run["threshold"].as_f64() {
            "P97 · 自动踢出"
        } else if calibration["p95"].as_f64() == run["threshold"].as_f64() {
            "P95 · 自动踢出"
        } else {
            "自动踢出（历史阈值）"
        };
        (
            "AI 自动决策（30 分钟时序模型）".to_owned(),
            tier.to_owned(),
            format!(
                "异常分数：{:.6}\n踢出阈值：{:.6}\n参考分布：{}\n异常分数不是作弊概率。",
                run["score"].as_f64().unwrap_or(0.),
                run["threshold"].as_f64().unwrap_or(0.),
                calibration
            ),
            format!("模型记录：{}", clean(text(&detail["runId"]), 100)),
        )
    } else {
        let source:String=sqlx::query_scalar("SELECT source FROM integrity_actions WHERE id=$1 AND case_id=$2 AND server_id=$3 AND steam_id=$4 AND delivery_state='delivered' AND action IN ('KICK','QUARANTINE_24H','QUARANTINE_7D') AND source IN ('RULE','REVIEW')").bind(detail["actionId"].as_str()).bind(detail["caseId"].as_str()).bind(server).bind(steam).fetch_optional(&mut **tx).await?.ok_or_else(||ApiError::bad("已确认反作弊处罚记录不存在。"))?;
        (
            if source == "REVIEW" {
                format!(
                    "管理员人工审核 · {}",
                    clean(text(&detail["reviewerName"]), 80)
                )
            } else {
                "反作弊规则自动处置".into()
            },
            "不适用（本次不是模型百分位处罚）".into(),
            if source == "REVIEW" {
                format!(
                    "人工审核说明：{}",
                    clean(text(&detail["reviewReason"]), 700)
                )
            } else {
                String::new()
            },
            format!("案件：{}", clean(text(&detail["caseId"]), 100)),
        )
    };
    let server_name: Option<String> = sqlx::query_scalar("SELECT name FROM servers WHERE id=$1")
        .bind(server)
        .fetch_optional(&mut **tx)
        .await?;
    let player:Option<String>=sqlx::query_scalar("SELECT name FROM player_sessions WHERE server_id=$1 AND steam_id=$2 ORDER BY last_seen DESC LIMIT 1").bind(server).bind(steam).fetch_optional(&mut **tx).await?;
    let name = detail["playerName"]
        .as_str()
        .or(player.as_deref())
        .unwrap_or("未知昵称");
    let content = format!(
        "【反作弊踢出通知】\n服务器：{}\n玩家：{}\nSteamID64：{steam}\n操作来源：{source}\n处罚档位：{tier}\n踢出理由：{}\n{reference}\n{identity}\n时间：{}（UTC+8）",
        clean(server_name.as_deref().unwrap_or(server), 100),
        clean(name, 100),
        clean(
            row["params"]["reason"]
                .as_str()
                .unwrap_or(text(&row["okMessage"])),
            700
        ),
        chrono::Utc::now()
            .with_timezone(&chrono::FixedOffset::east_opt(28800).unwrap())
            .format("%Y-%m-%d %H:%M:%S")
    );
    for group in &policy.groups {
        sqlx::query("INSERT INTO qq_integrity_notifications(outbox_id,server_id,group_id,self_id,content) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING").bind(row["id"].as_i64()).bind(server).bind(group).bind(&c.stored.self_id).bind(&content).execute(&mut **tx).await?;
    }
    Ok(())
}
/// Caller holds the QQ sender advisory lock; old sending rows came from an interrupted sender.
pub async fn pass(state: &AppState, c: &qq_config::Configuration) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE qq_integrity_notifications SET state='unknown',outcome='发送中断，结果未知，不自动重发',finished_at=now() WHERE state='sending'").execute(&mut *tx).await?;
    let notice:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(n) FROM qq_integrity_notifications n WHERE state='pending' ORDER BY created_at,outbox_id,group_id LIMIT 1 FOR UPDATE SKIP LOCKED").fetch_optional(&mut *tx).await?;
    let Some(notice) = notice else {
        tx.commit().await?;
        return Ok(());
    };
    let valid = notice["self_id"] == c.stored.self_id
        && c.policy(text(&notice["server_id"])).is_some_and(|p| {
            p.anti_cheat_notices && p.groups.iter().any(|g| g == text(&notice["group_id"]))
        })
        && crate::integrity_enforcement::date(&notice["created_at"])
            .is_some_and(|at| (chrono::Utc::now() - at).num_seconds() <= 86400);
    if !valid {
        sqlx::query("UPDATE qq_integrity_notifications SET state='skipped',outcome='通知过期或授权已改变',finished_at=now() WHERE outbox_id=$1 AND group_id=$2").bind(notice["outbox_id"].as_i64()).bind(notice["group_id"].as_str()).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(());
    }
    tx.commit().await?;
    if !qq_transport::logged_in(c).await {
        return Ok(());
    }
    let mut tx = state.worker_transaction().await?;
    let changed=sqlx::query("UPDATE qq_integrity_notifications SET state='sending' WHERE outbox_id=$1 AND group_id=$2 AND state='pending'").bind(notice["outbox_id"].as_i64()).bind(notice["group_id"].as_str()).execute(&mut *tx).await?;
    tx.commit().await?;
    if changed.rows_affected() == 0 {
        return Ok(());
    }
    state.runtime.check().await?;
    let ok = qq_transport::reply(
        c,
        text(&notice["group_id"]),
        &format!("integrity:{}", notice["outbox_id"]),
        text(&notice["content"]),
    )
    .await
    .is_ok();
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE qq_integrity_notifications SET state=$3,outcome=$4,finished_at=now() WHERE outbox_id=$1 AND group_id=$2 AND state='sending'").bind(notice["outbox_id"].as_i64()).bind(notice["group_id"].as_str()).bind(if ok{"delivered"}else{"unknown"}).bind(if ok{"QQ 接口确认发送"}else{"发送结果未知，不自动重发"}).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
