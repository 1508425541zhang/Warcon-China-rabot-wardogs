//! A single durable QQ sender. Unknown outcomes are terminal until an operator reconciles them.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    qq_community as community,
    qq_config::{self, Configuration},
    webhooks::text,
};
use serde_json::{Value, json};
pub async fn recover(state: &AppState) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE qq_orders SET state='unknown',outcome='服务中断，需人工核实，禁止自动重发或重复扣款' WHERE state='processing'").execute(&mut *tx).await?;
    sqlx::query("UPDATE qq_deliveries SET state='unknown',outcome='服务中断，发送结果未知' WHERE state='sending'").execute(&mut *tx).await?;
    sqlx::query("UPDATE qq_inbox SET state='done',reply_state='unknown',reply='服务中断，请查询订单；不要重复兑换' WHERE state='processing'").execute(&mut *tx).await?;
    sqlx::query("UPDATE qq_inbox SET reply_state='unknown' WHERE reply_state='sending'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
fn age(v: &Value) -> Option<i64> {
    crate::integrity_enforcement::date(v).map(|t| (chrono::Utc::now() - t).num_milliseconds())
}
pub async fn message(state: &AppState, c: &Configuration) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    let m:Option<Value>=sqlx::query_scalar("UPDATE qq_inbox SET state='processing',started_at=now() WHERE id=(SELECT id FROM qq_inbox WHERE state='pending' ORDER BY created_at,id LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING to_jsonb(qq_inbox)").fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    let Some(m) = m else { return Ok(()) };
    let valid = text(&m["id"]).starts_with(&format!(
        "{}:{}:",
        if c.stored.provider == "official" {
            "official"
        } else {
            "ob11"
        },
        c.stored.self_id
    )) && qq_config::identity(text(&m["member_id"]))
        && age(&m["created_at"]).is_some_and(|n| (0..=240000).contains(&n))
        && c.policy(text(&m["server_id"]))
            .is_some_and(|p| p.groups.iter().any(|g| g == text(&m["group_id"])));
    if !valid {
        let mut tx = state.worker_transaction().await?;
        sqlx::query("UPDATE qq_inbox SET state='done',reply_state='expired',reply='指令已过期或群授权已撤销，未执行业务操作' WHERE id=$1 AND state='processing'").bind(text(&m["id"])).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(());
    }
    let reply = match community::command(state, c, &m).await {
        Ok(reply) => reply,
        Err(error) => {
            if ["database", "worker_ownership_lost"].contains(&error.code) {
                "服务暂时不可用，请稍后查看订单或联系管理员。".into()
            } else {
                error.message
            }
        }
    };
    let mut tx = state.worker_transaction().await?;
    let changed=sqlx::query("UPDATE qq_inbox SET state='done',reply=$2,reply_state='sending' WHERE id=$1 AND state='processing'").bind(text(&m["id"])).bind(&reply).execute(&mut *tx).await?;
    tx.commit().await?;
    if changed.rows_affected() != 1 {
        return Ok(());
    }
    let current = qq_config::load(state).await?;
    let expired = age(&m["created_at"]).is_none_or(|n| n > 240000)
        || current.stored.revision != c.stored.revision
        || !current.active()
        || current
            .policy(text(&m["server_id"]))
            .is_none_or(|p| !p.groups.iter().any(|g| g == text(&m["group_id"])));
    let outcome = if expired {
        "expired"
    } else {
        state.runtime.check().await?;
        if crate::qq_transport::reply(&current, text(&m["group_id"]), text(&m["id"]), &reply)
            .await
            .is_ok()
        {
            "done"
        } else {
            "unknown"
        }
    };
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE qq_inbox SET reply_state=$2 WHERE id=$1 AND reply_state='sending'")
        .bind(text(&m["id"]))
        .bind(outcome)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
async fn delivery_state(
    state: &AppState,
    id: &str,
    steam: &str,
    status: &str,
    outcome: &str,
) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE qq_deliveries SET state=$3,outcome=$4 WHERE order_id=$1 AND steam_id=$2 AND state IN ('pending','sending')").bind(id).bind(steam).bind(status).bind(outcome).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
async fn order_state(state: &AppState, id: &str, status: &str, outcome: &str) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE qq_orders SET state=$2,outcome=$3 WHERE id=$1 AND state='processing'")
        .bind(id)
        .bind(status)
        .bind(outcome)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn deliver(state: &AppState, order: &Value) -> Result<()> {
    let server = text(&order["server_id"]);
    let id = text(&order["id"]);
    let s = community::server(state, server).await?;
    match text(&order["kind"]) {
        "friendly" => {
            let targets:Vec<String>=sqlx::query_scalar("SELECT steam_id FROM qq_deliveries WHERE order_id=$1 AND state='pending' ORDER BY steam_id").bind(id).fetch_all(&state.db).await?;
            for steam in targets {
                let lane =
                    crate::dispatcher::acquire(server, 0, std::time::Duration::from_secs(30))
                        .await?;
                let current = qq_config::load(state).await?;
                if current.policy(server).is_none() {
                    return Err(ApiError::missing());
                }
                let roster =
                    crate::gateway::run_held(state, server, "players", &json!({}), 0, None)
                        .await
                        .map_err(|_| ApiError::bad("游戏玩家列表不可用。"))?;
                let ps = roster["players"].as_array().cloned().unwrap_or_default();
                let from = text(&order["params"]["faction"]);
                let valid = age(&order["created_at"]).is_some_and(|n| (0..=120000).contains(&n))
                    && !from.is_empty()
                    && ps
                        .iter()
                        .any(|p| p["steamId"] == order["steam_id"] && p["faction"] == from)
                    && ps
                        .iter()
                        .any(|p| p["steamId"] == steam && p["faction"] == from);
                if !valid {
                    delivery_state(state, id, &steam, "skipped", "离线、阵营改变或广播已过期")
                        .await?;
                    continue;
                }
                let mut tx = state.worker_transaction().await?;
                let changed=sqlx::query("UPDATE qq_deliveries SET state='sending' WHERE order_id=$1 AND steam_id=$2 AND state='pending'").bind(id).bind(&steam).execute(&mut *tx).await?;
                tx.commit().await?;
                if changed.rows_affected() != 1 {
                    continue;
                }
                state.runtime.check().await?;
                let delivered = crate::gateway::run_held(
                    state,
                    server,
                    "whisper",
                    &json!({"steamId":steam,"message":order["params"]["message"]}),
                    0,
                    None,
                )
                .await
                .is_ok();
                drop(lane);
                delivery_state(
                    state,
                    id,
                    &steam,
                    if delivered { "done" } else { "unknown" },
                    if delivered {
                        "游戏接口确认"
                    } else {
                        "发送结果需人工核实，不自动重发"
                    },
                )
                .await?;
            }
            let rows:Vec<(String,i64)>=sqlx::query_as("SELECT state,count(*) FROM qq_deliveries WHERE order_id=$1 GROUP BY state ORDER BY state").bind(id).fetch_all(&state.db).await?;
            order_state(
                state,
                id,
                if rows.len() == 1 && rows[0].0 == "done" {
                    "done"
                } else {
                    "partial"
                },
                &rows
                    .iter()
                    .map(|(s, n)| format!("{s}: {n}人"))
                    .collect::<Vec<_>>()
                    .join("；"),
            )
            .await?;
        }
        "map" => {
            if age(&order["created_at"]).is_none_or(|n| n > 120000) {
                return Err(ApiError::bad("投票结果已过期，未修改地图。"));
            }
            community::action(state, server, "setNextMap", &order["params"], None).await?;
            order_state(
                state,
                id,
                "done",
                &format!("已设置下一张地图：{}", text(&order["params"]["map"])),
            )
            .await?;
        }
        "reserve" => {
            let hours = order["params"]["hours"]
                .as_i64()
                .filter(|n| (1..=720).contains(n))
                .ok_or_else(|| ApiError::bad("预留位时长无效。"))?;
            let mut tx = state.worker_transaction().await?;
            let c = qq_config::load_conn(state, &mut tx).await?;
            if c.policy(server).is_none() {
                return Err(ApiError::missing());
            }
            let list =
                crate::api::lists::ensure(&mut tx, text(&s["orgId"]), Some(server), "reserve")
                    .await?;
            sqlx::query("UPDATE list_entries SET removed_at=now(),removal='expired',removed_by_name='expiry' WHERE list_id=$1 AND steam_id=$2 AND removed_at IS NULL AND expires_at<=now()").bind(&list).bind(text(&order["steam_id"])).execute(&mut *tx).await?;
            let (entry, added) = crate::api::lists::insert(
                &mut tx,
                &list,
                text(&order["steam_id"]),
                &format!("QQ积分订单 {id}"),
                Some(chrono::Utc::now() + chrono::Duration::hours(hours)),
                None,
                "QQ积分兑换",
            )
            .await?;
            sqlx::query("UPDATE lists SET updated_at=now() WHERE id=$1")
                .bind(&list)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO audit_log(actor_name,server_id,server_name,org_id,category,action,target,detail,outcome) VALUES('QQ积分兑换',$1,$2,$3,'list','list.add',$4,$5,'ok')").bind(server).bind(text(&s["name"])).bind(text(&s["orgId"])).bind(text(&order["steam_id"])).bind(json!({"kind":"reserve","entryId":entry,"orderId":id,"added":added})).execute(&mut *tx).await?;
            sqlx::query("UPDATE qq_orders SET state='done',outcome='限时预留位已登记，等待 WARCON 同步；尚未确认游戏生效' WHERE id=$1 AND state='processing'").bind(id).execute(&mut *tx).await?;
            sqlx::query("SELECT pg_notify('warcon_observe',$1)")
                .bind(json!({"serverId":server,"lists":true}).to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            let ok = crate::list_sync::reconcile(state, server, 0)
                .await
                .is_ok_and(|v| v["failed"].as_i64() == Some(0) && text(&v["error"]).is_empty());
            if ok {
                let mut tx = state.worker_transaction().await?;
                sqlx::query("UPDATE qq_orders SET outcome='限时预留位已同步到游戏' WHERE id=$1 AND state='done'").bind(id).execute(&mut *tx).await?;
                tx.commit().await?;
            }
        }
        _ => return Err(ApiError::bad("Unknown community order kind.")),
    }
    Ok(())
}
pub async fn pass(state: &AppState, c: &Configuration) -> Result<()> {
    recover(state).await?;
    message(state, c).await?;
    crate::qq_notices::pass(state, c).await?;
    community::close_votes(state, c).await?;
    let mut tx = state.worker_transaction().await?;
    let order:Option<Value>=sqlx::query_scalar("UPDATE qq_orders SET state='processing',started_at=now() WHERE id=(SELECT id FROM qq_orders WHERE state='pending' ORDER BY created_at,id LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING to_jsonb(qq_orders)").fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    if let Some(o) = order {
        if deliver(state, &o).await.is_err() {
            order_state(
                state,
                text(&o["id"]),
                "unknown",
                "结果待核实，请管理员检查游戏状态及订单后结算，不自动重发。",
            )
            .await?
        }
    }
    Ok(())
}
pub async fn run(state: AppState) -> anyhow::Result<()> {
    // This connection is destroyed on cancellation, so its session lock cannot leak into the pool.
    let mut lock = state.db.acquire().await?;
    lock.close_on_drop();
    loop {
        state.runtime.check().await?;
        let held: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(1869754673,1)")
            .fetch_one(&mut *lock)
            .await?;
        if held {
            break;
        }
        tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),_=tokio::time::sleep(std::time::Duration::from_secs(5))=>{}}
    }
    loop {
        state.runtime.check().await?;
        sqlx::query("SELECT 1").execute(&mut *lock).await?;
        let c = qq_config::load(&state).await?;
        if c.active() {
            if pass(&state, &c).await.is_err() {
                tracing::warn!("QQ pass failed; unconfirmed delivery is not retried");
            }
        }
        tokio::select! {_=state.runtime.stop.cancelled()=>break,_=tokio::time::sleep(std::time::Duration::from_secs(1))=>{}}
    }
    sqlx::query("SELECT pg_advisory_unlock(1869754673,1)")
        .execute(&mut *lock)
        .await?;
    Ok(())
}
