use crate::{auth::Actor, error::Result};
use axum::http::HeaderMap;
use serde_json::Value;
use sqlx::{Postgres, Transaction};

pub async fn org_event(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    org_id: &str,
    headers: &HeaderMap,
    action: &str,
    target: &str,
    detail: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,org_id,category,action,target,detail,outcome,user_agent) VALUES($1,$2,$3,'org',$4,$5,$6,'ok',$7)")
        .bind(&actor.id).bind(&actor.name).bind(org_id).bind(action).bind(target).bind(detail)
        .bind(headers.get("user-agent").and_then(|v|v.to_str().ok()).unwrap_or("")).execute(&mut **tx).await?;
    Ok(())
}
