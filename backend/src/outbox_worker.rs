//! Durable RCON delivery: unknown responses are terminal and never replayed.
use crate::{
    actions,
    config::AppState,
    error::{ApiError, Result},
    game, integrity_delivery,
    integrity_enforcement::date,
    integrity_statistics::camel_row,
};
use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};
pub async fn claim(state: &AppState, lease: i64) -> Result<Vec<Value>> {
    let mut tx = state.worker_transaction().await?;
    let expired:Vec<Value>=sqlx::query_scalar("UPDATE outbox SET state='unknown',outcome='The worker stopped while sending; the game may have acted.',done_at=now(),lease_until=NULL WHERE state='sending' AND lease_until<now() RETURNING to_jsonb(outbox)").fetch_all(&mut *tx).await?;
    let expired_count = expired.len() as u64;
    for row in expired {
        integrity_delivery::record(&mut tx, &camel_row(row), "unknown").await?;
    }
    let rows:Vec<Value>=sqlx::query_scalar("UPDATE outbox SET state='sending',lease_until=now()+($1::bigint*interval '1 millisecond'),attempts=attempts+1 WHERE id IN (SELECT id FROM outbox WHERE state='pending' AND not_before<=now() ORDER BY id LIMIT 50 FOR UPDATE SKIP LOCKED) RETURNING to_jsonb(outbox)").bind(lease).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    crate::diagnostics::delivery("unknown", expired_count);
    let mut rows: Vec<_> = rows.into_iter().map(camel_row).collect();
    rows.sort_by_key(|r| r["id"].as_i64());
    Ok(rows)
}
async fn disposition(
    state: &AppState,
    row: &Value,
    max_age: i64,
) -> Result<Option<(&'static str, String)>> {
    let server = row["serverId"].as_str().unwrap_or("");
    let live:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l JOIN servers s ON s.id=l.server_id JOIN organizations o ON o.id=s.org_id WHERE l.server_id=$1 AND o.suspended_at IS NULL").bind(server).fetch_optional(&state.db).await?;
    let Some(live) = live else {
        return Ok(Some(("skipped", "Server no longer polled.".into())));
    };
    // A queued rule must not execute after its owner disabled, edited or deleted it.
    if let Some(trigger) = row["triggerId"].as_str() {
        let enabled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM triggers WHERE id=$1 AND server_id=$2 AND enabled AND updated_at<= $3)").bind(trigger).bind(server).bind(date(&row["createdAt"])).fetch_one(&state.db).await?;
        if !enabled {
            return Ok(Some(("skipped", "规则已经关闭、变更或删除。".into())));
        }
    } else if row["triggerKind"]
        .as_str()
        .is_some_and(|k| crate::trigger_policy::KINDS.contains(&k))
    {
        return Ok(Some(("skipped", "规则已经删除。".into())));
    }
    if date(&row["createdAt"]).is_none_or(|at| (Utc::now() - at).num_milliseconds() > max_age) {
        return Ok(Some(("skipped", "Queued action is stale.".into())));
    }
    let roster = live["players"].as_array().cloned().unwrap_or_default();
    if let Some(steam) = row["steamId"].as_str() {
        if !live["players_at"].is_null() && !roster.iter().any(|p| p["steamId"] == steam) {
            let open:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM player_sessions WHERE server_id=$1 AND steam_id=$2 AND left_at IS NULL)").bind(server).bind(steam).fetch_one(&state.db).await?;
            return Ok(Some(if open {
                ("wait", "Player may be returning after map travel.".into())
            } else {
                ("skipped", "Player already left.".into())
            }));
        }
    }
    if row["action"] == "empty_reset"
        && (!roster.is_empty() || live["status"]["playerCount"].as_u64().unwrap_or(0) > 0)
    {
        return Ok(Some((
            "skipped",
            "Players arrived before the reset.".into(),
        )));
    }
    if let Some(reason) = integrity_delivery::skip(state, row).await? {
        return Ok(Some(("skipped", reason.into())));
    }
    Ok(None)
}
async fn finish_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &Value,
    result: &str,
    message: &str,
) -> Result<bool> {
    let lease = date(&row["leaseUntil"]);
    let attempts = row["attempts"].as_i64().unwrap_or(0) as i32;
    let changed=sqlx::query("UPDATE outbox SET state=$4,outcome=$5,done_at=now(),lease_until=NULL WHERE id=$1 AND attempts=$2 AND lease_until=$3 AND state='sending' AND lease_until>clock_timestamp()").bind(row["id"].as_i64()).bind(attempts).bind(lease).bind(result).bind(crate::feed::truncate(message,300)).execute(&mut **tx).await?;
    if changed.rows_affected() == 0 {
        return Ok(false);
    }
    integrity_delivery::record(tx, row, result).await?;
    if result == "delivered" {
        crate::qq_notices::enqueue(state, tx, row).await?;
    }
    if result != "skipped" {
        sqlx::query("INSERT INTO audit_log(actor_name,org_id,server_id,server_name,category,action,target,outcome,message,detail) SELECT $2,s.org_id,s.id,s.name,'system','trigger.delivery',$3,$4,$5,$6 FROM servers s WHERE s.id=$1").bind(row["serverId"].as_str()).bind(row["triggerName"].as_str().unwrap_or("automation")).bind(row["target"].as_str().unwrap_or("")).bind(if result=="delivered"{"ok"}else{"error"}).bind(crate::feed::truncate(message,1000)).bind(crate::audit::redact(&json!({"rconAction":row["action"],"outboxId":row["id"],"state":result,"detail":row["detail"]}),0)).execute(&mut **tx).await?;
    }
    sqlx::query("SELECT pg_notify('warcon_events',$1)")
        .bind(
            json!({"type":"outbox","serverId":row["serverId"],"id":row["id"],"state":result})
                .to_string(),
        )
        .execute(&mut **tx)
        .await?;
    Ok(true)
}
pub async fn finish(state: &AppState, row: &Value, result: &str, message: &str) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    let applied = finish_tx(state, &mut tx, row, result, message).await?;
    commit_finished(tx, applied, result).await
}
async fn commit_finished(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    applied: bool,
    result: &str,
) -> Result<()> {
    tx.commit().await?;
    if applied {
        crate::diagnostics::delivery(result, 1);
    }
    Ok(())
}
async fn wait(state: &AppState, row: &Value) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE outbox SET state='pending',lease_until=NULL,not_before=now()+interval '5 seconds' WHERE id=$1 AND attempts=$2 AND lease_until=$3 AND state='sending' AND lease_until>clock_timestamp()").bind(row["id"].as_i64()).bind(row["attempts"].as_i64().unwrap_or(0) as i32).bind(date(&row["leaseUntil"])).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
async fn seed(state: &AppState, row: &Value) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    let org: Option<String> = sqlx::query_scalar("SELECT org_id FROM servers WHERE id=$1")
        .bind(row["serverId"].as_str())
        .fetch_optional(&mut *tx)
        .await?;
    let Some(org) = org else {
        let applied = finish_tx(state, &mut tx, row, "skipped", "Server no longer exists.").await?;
        return commit_finished(tx, applied, "skipped").await;
    };
    let p = &row["params"];
    let steam = p["steamId"].as_str().unwrap_or("");
    let days = p["slotDays"].as_f64().unwrap_or(0.);
    if crate::api::notes::steam_id(steam).is_err()
        || !days.is_finite()
        || days <= 0.
        || days > 3650.
    {
        let applied = finish_tx(
            state,
            &mut tx,
            row,
            "failed",
            "Invalid reserved slot grant.",
        )
        .await?;
        return commit_finished(tx, applied, "failed").await;
    }
    let server = if p["scope"] == "server" {
        row["serverId"].as_str()
    } else {
        None
    };
    let list = crate::api::lists::ensure(&mut tx, &org, server, "reserve").await?;
    let old:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(e) FROM list_entries e WHERE list_id=$1 AND steam_id=$2 AND removed_at IS NULL FOR UPDATE").bind(&list).bind(steam).fetch_optional(&mut *tx).await?;
    if old.as_ref().is_some_and(|e| {
        e["expires_at"].is_null() || date(&e["expires_at"]).is_some_and(|d| d > Utc::now())
    }) {
        let applied = finish_tx(
            state,
            &mut tx,
            row,
            "skipped",
            "Player already has a reserved slot.",
        )
        .await?;
        return commit_finished(tx, applied, "skipped").await;
    }
    if let Some(old) = old {
        sqlx::query("UPDATE list_entries SET removed_at=now(),removal='expired' WHERE id=$1")
            .bind(old["id"].as_str())
            .execute(&mut *tx)
            .await?;
    }
    let expires = Utc::now() + chrono::Duration::milliseconds((days * 86400000.) as i64);
    sqlx::query("INSERT INTO list_entries(id,list_id,steam_id,reason,expires_at,added_by_name) VALUES($1,$2,$3,$4,$5,$6)").bind(uuid::Uuid::new_v4().to_string()).bind(&list).bind(steam).bind(p["reason"].as_str().unwrap_or("")).bind(expires).bind(format!("trigger: {}",row["triggerName"].as_str().unwrap_or(""))).execute(&mut *tx).await?;
    sqlx::query("UPDATE lists SET updated_at=now() WHERE id=$1")
        .bind(list)
        .execute(&mut *tx)
        .await?;
    let applied = finish_tx(
        state,
        &mut tx,
        row,
        "delivered",
        &format!("Reserved a slot until {}.", expires.format("%Y-%m-%d")),
    )
    .await?;
    commit_finished(tx, applied, "delivered").await
}
async fn execute(client: &game::Client, row: &Value) -> game::Result<Value> {
    let action = row["action"].as_str().unwrap_or("");
    let params = &row["params"];
    if action == "empty_reset" {
        let rotation = actions::run(client, "rotation", &json!({}))
            .await
            .is_ok_and(|r| r["enabled"] == true);
        if rotation {
            let changed = async {
                actions::run(client, "setNextMap", params).await?;
                actions::run(client, "endMatch", &json!({})).await
            }
            .await;
            match changed {
                Ok(result) => return Ok(result),
                Err(game::Error::Game(e)) if e.code == "no_route" || e.status == 405 => {}
                Err(e) => return Err(e),
            }
        }
        actions::run(client, "changeMap", params).await?;
        return if rotation {
            Ok(json!({"message":"Map changed directly."}))
        } else {
            actions::run(client, "endMatch", &json!({})).await
        };
    }
    actions::run(client, action, params).await
}
pub async fn deliver(state: &AppState, row: &Value, lease: i64, max_age: i64) -> Result<()> {
    state.runtime.check().await?;
    if row["action"] == "seed_reward" {
        return seed(state, row).await;
    }
    if ["name_flag", "kill_rate_flag"].contains(&row["action"].as_str().unwrap_or("")) {
        return finish(
            state,
            row,
            "delivered",
            row["okMessage"].as_str().unwrap_or(""),
        )
        .await;
    }
    if let Some((result, message)) = disposition(state, row, max_age).await? {
        return if result == "wait" {
            wait(state, row).await
        } else {
            finish(state, row, result, &message).await
        };
    }
    let server = row["serverId"].as_str().unwrap_or("");
    let guard =
        match crate::dispatcher::acquire(server, 1, Duration::from_millis(lease.max(1) as u64))
            .await
        {
            Ok(g) => g,
            Err(e) => return finish(state, row, "skipped", &e.message).await,
        };
    state.runtime.check().await?;
    if let Some((result, message)) = disposition(state, row, max_age).await? {
        drop(guard);
        return if result == "wait" {
            wait(state, row).await
        } else {
            finish(state, row, result, &message).await
        };
    }
    // A recovered lease is never dispatched by the old claimant, even if it waited for a lane.
    let owned:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM outbox WHERE id=$1 AND state='sending' AND attempts=$2 AND lease_until=$3 AND lease_until>clock_timestamp())").bind(row["id"].as_i64()).bind(row["attempts"].as_i64().unwrap_or(0) as i32).bind(date(&row["leaseUntil"])).fetch_one(&state.db).await?;
    if !owned {
        return Ok(());
    }
    let result = match game::Client::for_server(state, server).await {
        Ok(client) => execute(&client, row).await,
        Err(e) => Err(e),
    };
    drop(guard);
    state.runtime.check().await?;
    match result {
        Ok(result) => {
            finish(
                state,
                row,
                "delivered",
                result["message"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(row["okMessage"].as_str().unwrap_or("")),
            )
            .await
        }
        Err(game::Error::Game(e)) if e.code == "unreachable" => {
            finish(
                state,
                row,
                "unknown",
                "No answer from the server; the action was not retried.",
            )
            .await
        }
        Err(game::Error::Game(e)) => finish(state, row, "failed", &e.message).await,
        Err(game::Error::Api(e)) if e.code == "worker_ownership_lost" => Err(e),
        Err(game::Error::Api(e)) => finish(state, row, "failed", &e.message).await,
    }
}
pub async fn pass(state: &AppState) -> Result<usize> {
    let settings = crate::settings::load(&state.db).await?;
    let lease = crate::observer::number(&settings, "outboxLeaseMs", 60000);
    let age = crate::observer::number(&settings, "outboxMaxAgeMs", 120000);
    let rows = claim(state, lease).await?;
    let total = rows.len();
    let mut servers: HashMap<String, Vec<Value>> = HashMap::new();
    for row in rows {
        servers
            .entry(row["serverId"].as_str().unwrap_or("").into())
            .or_default()
            .push(row);
    }
    let results = futures_util::future::join_all(servers.into_values().map(|rows| async move {
        for row in rows {
            let _active = crate::diagnostics::delivery_active();
            deliver(state, &row, lease, age).await?;
        }
        Ok::<_, ApiError>(())
    }))
    .await;
    for result in results {
        result?;
    }
    Ok(total)
}
pub async fn run(state: AppState) -> anyhow::Result<()> {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),_=tick.tick()=>{state.runtime.check().await?;crate::diagnostics::delivery_pass();if pass(&state).await.is_err(){tracing::warn!("RCON outbox pass failed; claimed actions remain protected by their lease");}}}
    }
}
