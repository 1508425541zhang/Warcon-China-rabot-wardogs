//! Paid commands and warmth credits share their ledger transaction. Uncertain sends never refund themselves.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    qq_config,
};
use axum::http::StatusCode;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
pub async fn warmth(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    server: &str,
    at: DateTime<Utc>,
    ids: &[String],
    gap: i64,
    count: usize,
    trusted: bool,
) -> Result<()> {
    if !trusted || gap <= 0 || gap > 60000 || ids.is_empty() {
        return Ok(());
    }
    let config = qq_config::load_conn(state, tx).await?;
    let Some(p) = config.policy(server).filter(|p| count <= p.low_at as usize) else {
        return Ok(());
    };
    let mut ids = ids
        .iter()
        .filter(|id| id.len() == 17 && id.bytes().all(|c| c.is_ascii_digit()))
        .cloned()
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(());
    }
    let changed=sqlx::query("INSERT INTO qq_warm_ticks(server_id,observed_at) VALUES($1,$2) ON CONFLICT(server_id) DO UPDATE SET observed_at=excluded.observed_at WHERE qq_warm_ticks.observed_at<excluded.observed_at").bind(server).bind(at).execute(&mut **tx).await?;
    if changed.rows_affected() == 0 {
        return Ok(());
    }
    sqlx::query("INSERT INTO qq_wallets(server_id,steam_id) SELECT $1,unnest FROM unnest($2::text[]) ORDER BY unnest ON CONFLICT DO NOTHING").bind(server).bind(&ids).execute(&mut **tx).await?;
    sqlx::query("WITH previous AS(SELECT steam_id,balance,warm_ms FROM qq_wallets WHERE server_id=$1 AND steam_id=ANY($2) ORDER BY steam_id FOR UPDATE),changed AS(UPDATE qq_wallets w SET warm_ms=p.warm_ms+$3,balance=p.balance+(((p.warm_ms+$3)/60000-p.warm_ms/60000)*$4) FROM previous p WHERE w.server_id=$1 AND w.steam_id=p.steam_id RETURNING w.steam_id,w.balance-p.balance AS delta) INSERT INTO qq_ledger(id,server_id,steam_id,delta,reason) SELECT $5||steam_id,$1,steam_id,delta,'暖服时长' FROM changed WHERE delta>0").bind(server).bind(&ids).bind(gap).bind(p.points_per_minute).bind(format!("warm:{server}:{}:",at.to_rfc3339_opts(SecondsFormat::Millis,true))).execute(&mut **tx).await?;
    Ok(())
}
pub async fn debit(
    tx: &mut Transaction<'_, Postgres>,
    server: &str,
    steam: &str,
    cost: i64,
    id: &str,
    reason: &str,
) -> Result<()> {
    if cost <= 0 || cost > 9007199254740991 {
        return Err(ApiError::bad("积分价格无效。"));
    }
    let changed=sqlx::query("UPDATE qq_wallets SET balance=balance-$3 WHERE server_id=$1 AND steam_id=$2 AND balance>=$3").bind(server).bind(steam).bind(cost).execute(&mut **tx).await?;
    if changed.rows_affected() != 1 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "insufficient_points",
            "积分不足，请先暖服。",
        ));
    }
    sqlx::query("INSERT INTO qq_ledger(id,server_id,steam_id,delta,reason) VALUES($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(server)
        .bind(steam)
        .bind(-cost)
        .bind(reason)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn wallet(state: &AppState, server: &str, steam: &str) -> Result<Value> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('balance',balance,'warmMinutes',warm_ms/60000) FROM qq_wallets WHERE server_id=$1 AND steam_id=$2").bind(server).bind(steam).fetch_optional(&state.db).await?.unwrap_or_else(||json!({"balance":0,"warmMinutes":0})))
}
pub async fn reconcile(
    tx: &mut Transaction<'_, Postgres>,
    server: &str,
    id: &str,
    refund: bool,
    actor: &str,
) -> Result<()> {
    let order: Value = sqlx::query_scalar(
        "SELECT to_jsonb(o) FROM qq_orders o WHERE id=$1 AND server_id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(server)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| ApiError::bad("订单不在可人工处理状态。"))?;
    if !["unknown", "failed", "partial"].contains(&crate::webhooks::text(&order["state"])) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "order_state",
            "订单不在可人工处理状态。",
        ));
    }
    if refund && order["cost"].as_i64().unwrap_or(0) > 0 {
        let steam = crate::webhooks::text(&order["steam_id"]);
        let cost = order["cost"].as_i64().unwrap();
        credit(
            tx,
            server,
            steam,
            cost,
            &format!("refund:{id}"),
            &format!("人工退款:{actor}"),
        )
        .await?;
    }
    if refund && order["kind"] == "map" {
        let vote = id
            .strip_prefix("map:")
            .ok_or_else(|| ApiError::bad("地图订单无效。"))?;
        let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steam',l.steam_id,'amount',-l.delta) FROM qq_ledger l JOIN qq_ballots b ON b.steam_id=l.steam_id AND b.vote_id=$1 WHERE l.server_id=$2 AND l.id=('vote:'||$1||':'||b.steam_id) ORDER BY l.steam_id").bind(vote).bind(server).fetch_all(&mut **tx).await?;
        for row in rows {
            let steam = crate::webhooks::text(&row["steam"]);
            credit(
                tx,
                server,
                steam,
                row["amount"].as_i64().unwrap(),
                &format!("refund:{id}:{steam}"),
                &format!("地图投票退款:{actor}"),
            )
            .await?;
        }
    }
    sqlx::query("UPDATE qq_orders SET state=$2,outcome=$3 WHERE id=$1")
        .bind(id)
        .bind(if refund { "refunded" } else { "done" })
        .bind(format!("人工核实:{actor}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn credit(
    tx: &mut Transaction<'_, Postgres>,
    server: &str,
    steam: &str,
    cost: i64,
    id: &str,
    reason: &str,
) -> Result<()> {
    let changed =
        sqlx::query("UPDATE qq_wallets SET balance=balance+$3 WHERE server_id=$1 AND steam_id=$2")
            .bind(server)
            .bind(steam)
            .bind(cost)
            .execute(&mut **tx)
            .await?;
    if changed.rows_affected() != 1 {
        return Err(ApiError::bad("积分钱包不存在，订单未退款。"));
    }
    sqlx::query("INSERT INTO qq_ledger(id,server_id,steam_id,delta,reason) VALUES($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(server)
        .bind(steam)
        .bind(cost)
        .bind(reason)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
