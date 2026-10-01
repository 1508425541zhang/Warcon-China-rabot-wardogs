//! Durable Discord fan-out. Timeouts are terminal unknown deliveries; only 429 is retried.
use crate::{config::AppState, error::Result, webhooks as hooks};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::time::Duration;
pub async fn fanout(state: &AppState) -> Result<usize> {
    let mut tx = state.worker_transaction().await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e) FROM webhook_events e WHERE processed_at IS NULL ORDER BY created_at,id LIMIT 100 FOR UPDATE SKIP LOCKED").fetch_all(&mut *tx).await?;
    for r in &rows {
        let kind = hooks::text(&r["kind"]);
        let event = if kind == "audit" {
            hooks::classify(&r["payload"])
        } else if hooks::EVENTS.contains(&kind) {
            Some(kind)
        } else {
            None
        };
        let org = if let Some(org) = r["org_id"].as_str() {
            Some(org.to_owned())
        } else {
            sqlx::query_scalar::<_, String>("SELECT org_id FROM servers WHERE id=$1")
                .bind(r["server_id"].as_str())
                .fetch_optional(&mut *tx)
                .await?
        };
        if let (Some(event), Some(org)) = (event, org) {
            let configs:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(w) FROM webhooks w JOIN organizations o ON o.id=w.org_id WHERE w.org_id=$1 AND w.enabled AND o.suspended_at IS NULL").bind(&org).fetch_all(&mut *tx).await?;
            for h in configs {
                if h["events"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v == event))
                    && hooks::scope(&h["server_ids"], r["server_id"].as_str())
                {
                    let embed = if kind == "audit" {
                        hooks::audit_embed(&state.config.identity.app_name, &r["payload"])
                    } else if kind == "integrity" {
                        hooks::case_embed(&state.config.identity.app_name, &r["payload"])
                    } else {
                        r["payload"].clone()
                    };
                    sqlx::query("INSERT INTO webhook_queue(hook_id,hook_version,source_id,url_enc,payload) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING").bind(h["id"].as_str()).bind(crate::integrity_enforcement::date(&h["updated_at"])).bind(r["id"].as_str()).bind(h["url_enc"].as_str()).bind(hooks::payload(state,embed)).execute(&mut *tx).await?;
                }
            }
        }
        sqlx::query("UPDATE webhook_events SET processed_at=now() WHERE id=$1")
            .bind(r["id"].as_str())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(rows.len())
}
pub async fn claim(state: &AppState) -> Result<Vec<Value>> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE webhook_queue SET state='unknown',done_at=now(),lease_until=NULL,outcome='发送中断，结果未知，不自动重发' WHERE state='sending' AND lease_until<=now()").execute(&mut *tx).await?;
    let first:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(q) FROM webhook_queue q WHERE state='pending' AND not_before<=now() AND NOT EXISTS(SELECT 1 FROM webhook_queue earlier WHERE earlier.hook_id IS NOT DISTINCT FROM q.hook_id AND earlier.id<q.id AND earlier.state IN ('pending','sending')) ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED").fetch_optional(&mut *tx).await?;
    let Some(first) = first else {
        tx.commit().await?;
        return Ok(vec![]);
    };
    let method = hooks::text(&first["method"]);
    let rows:Vec<Value>=sqlx::query_scalar("UPDATE webhook_queue SET state='sending',attempts=attempts+1,lease_until=now()+interval '30 seconds' WHERE id IN (SELECT id FROM webhook_queue WHERE state='pending' AND not_before<=now() AND hook_id IS NOT DISTINCT FROM $1 AND hook_version IS NOT DISTINCT FROM $2 AND method=$3 AND path=$4 AND url_enc=$5 ORDER BY id LIMIT $6 FOR UPDATE SKIP LOCKED) RETURNING to_jsonb(webhook_queue)").bind(first["hook_id"].as_str()).bind(crate::integrity_enforcement::date(&first["hook_version"])).bind(method).bind(first["path"].as_str()).bind(first["url_enc"].as_str()).bind(if method=="POST"{10i64}else{1i64}).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let mut rows = rows;
    rows.sort_by_key(|v| v["id"].as_i64());
    Ok(rows)
}
pub async fn complete(
    state: &AppState,
    rows: &[Value],
    result: &hooks::PostResult,
    skipped: bool,
) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    for row in rows {
        let state_name = if skipped {
            "skipped"
        } else if result.retry_after_ms.is_some() {
            "pending"
        } else if result.ok {
            "delivered"
        } else if result.status == 0 {
            "unknown"
        } else {
            "failed"
        };
        let changed=sqlx::query("UPDATE webhook_queue SET state=$4,lease_until=NULL,done_at=CASE WHEN $4='pending' THEN NULL ELSE now() END,not_before=now()+($5::bigint*interval '1 millisecond'),outcome=$6 WHERE id=$1 AND attempts=$2 AND lease_until=$3 AND state='sending' AND lease_until>clock_timestamp()").bind(row["id"].as_i64()).bind(row["attempts"].as_i64().unwrap_or(0) as i32).bind(crate::integrity_enforcement::date(&row["lease_until"])).bind(state_name).bind(result.retry_after_ms.unwrap_or(0) as i64).bind(hooks::clip(&result.error,300)).execute(&mut *tx).await?;
        if changed.rows_affected() > 0 && !skipped {
            if let Some(id) = row["hook_id"].as_str() {
                hooks::record(&mut tx, id, result).await?
            }
        }
    }
    tx.commit().await?;
    Ok(())
}
pub async fn deliver(state: &AppState, mut rows: Vec<Value>) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let first = &rows[0];
    if first["method"] != "DELETE" {
        let current:Option<(DateTime<Utc>,bool)>=sqlx::query_as("SELECT w.updated_at,w.enabled AND o.suspended_at IS NULL FROM webhooks w JOIN organizations o ON o.id=w.org_id WHERE w.id=$1").bind(first["hook_id"].as_str()).fetch_optional(&state.db).await?;
        if current.is_none_or(|(date, enabled)| {
            !enabled || Some(date) != crate::integrity_enforcement::date(&first["hook_version"])
        }) {
            complete(
                state,
                &rows,
                &hooks::PostResult {
                    error: "Webhook 已停用或配置已变更。".into(),
                    ..Default::default()
                },
                true,
            )
            .await?;
            return Ok(());
        }
    }
    let mut embeds = Vec::new();
    let mut chars = 0;
    let mut count = 0;
    for r in &rows {
        let values = r["payload"]["embeds"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let length = values
            .iter()
            .map(crate::webhook_status::embed_length)
            .sum::<usize>();
        if count > 0 && chars + length > 5800 {
            break;
        }
        chars += length;
        embeds.extend(values);
        count += 1
    }
    if count < rows.len() {
        let extra = rows.split_off(count);
        let mut tx = state.worker_transaction().await?;
        for r in extra {
            sqlx::query("UPDATE webhook_queue SET state='pending',lease_until=NULL WHERE id=$1 AND attempts=$2 AND lease_until=$3 AND state='sending'").bind(r["id"].as_i64()).bind(r["attempts"].as_i64().unwrap() as i32).bind(crate::integrity_enforcement::date(&r["lease_until"])).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    let first = &rows[0];
    let body = if first["method"] == "POST" {
        let mut payload = first["payload"].clone();
        payload["embeds"] = json!(embeds);
        Some(payload)
    } else {
        None
    };
    state.runtime.check().await?;
    let result = hooks::discord_call(
        state,
        hooks::text(&first["url_enc"]),
        hooks::text(&first["method"]),
        hooks::text(&first["path"]),
        body.as_ref(),
    )
    .await;
    complete(state, &rows, &result, false).await
}
pub async fn run(state: AppState) -> Result<()> {
    let mut tick = tokio::time::interval(Duration::from_millis(1500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=state.runtime.stop.cancelled()=>break,_=tick.tick()=>{state.runtime.check().await?;if fanout(&state).await.is_err(){tracing::warn!("Webhook fan-out failed");continue}for _ in 0..10{let rows=claim(&state).await?;if rows.is_empty(){break}if deliver(&state,rows).await.is_err(){tracing::warn!("Webhook delivery failed")}}}}
    }
    Ok(())
}
