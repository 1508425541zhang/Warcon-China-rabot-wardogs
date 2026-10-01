//! Trigger records and durable intentions share a transaction with their audit trail.
use crate::{
    auth::{Actor, ServerScope},
    config::AppState,
    error::{ApiError, Result},
    http::{string, truthy},
    trigger_policy as policy,
};
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
pub fn view(v: Value) -> Value {
    let v = crate::ai_evidence::row(v);
    json!({"id":v["id"],"kind":v["kind"],"name":v["name"],"enabled":v["enabled"],"config":v["config"],"lastFiredAt":v["lastFiredAt"],"lastResult":v["lastResult"],"fireCount":v["fireCount"],"createdAt":v["createdAt"]})
}
pub fn require_rule(server: &ServerScope, kind: &str, cfg: &Value) -> Result<()> {
    if server.caps.iter().any(|c| c == policy::needed(kind, cfg)) {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}
pub async fn list(state: &AppState, id: &str) -> Result<Vec<Value>> {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(t) FROM triggers t WHERE server_id=$1 ORDER BY created_at,id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(view).collect())
}
pub async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    server: &ServerScope,
    headers: &HeaderMap,
    action: &str,
    target: &str,
    detail: &Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,server_id,server_name,org_id,category,action,target,outcome,detail,user_agent)VALUES($1,$2,$3,$4,$5,'server',$6,$7,'ok',$8,$9)").bind(&actor.id).bind(&actor.name).bind(&server.id).bind(&server.name).bind(&server.org_id).bind(action).bind(target).bind(crate::audit::redact(detail,0)).bind(headers.get("user-agent").and_then(|v|v.to_str().ok()).unwrap_or("")).execute(&mut **tx).await?;
    sqlx::query("SELECT pg_notify('warcon_triggers',$1)")
        .bind(&server.id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn create(
    state: &AppState,
    actor: &Actor,
    server: &ServerScope,
    headers: &HeaderMap,
    body: &Value,
) -> Result<Value> {
    if actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    let kind = body["kind"]
        .as_str()
        .filter(|k| policy::KINDS.contains(k))
        .ok_or_else(|| ApiError::bad("未知自动化类型。"))?;
    let cfg = policy::validate(kind, &body["config"])?;
    require_rule(server, kind, &cfg)?;
    let name = string(&body["name"], 60);
    let name = if name.is_empty() {
        policy::label(kind).to_owned()
    } else {
        name
    };
    let id = uuid::Uuid::new_v4().to_string();
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('triggers:'||$1,0))")
        .bind(&server.id)
        .execute(&mut *tx)
        .await?;
    if kind == "seed_reward" {
        let found: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM triggers WHERE server_id=$1 AND kind='seed_reward')",
        )
        .bind(&server.id)
        .fetch_one(&mut *tx)
        .await?;
        if found {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "duplicate",
                "每个服务器只能保留一条种服奖励规则，请编辑已有规则。",
            ));
        }
    }
    let row:Value=sqlx::query_scalar("INSERT INTO triggers(id,server_id,org_id,kind,name,enabled,config,created_by)VALUES($1,$2,$3,$4,$5,$6,$7,$8)RETURNING to_jsonb(triggers)").bind(id).bind(&server.id).bind(&server.org_id).bind(kind).bind(&name).bind(truthy(&body["enabled"])).bind(&cfg).bind(&actor.id).fetch_one(&mut *tx).await?;
    audit(
        &mut tx,
        actor,
        server,
        headers,
        "trigger.create",
        &name,
        &json!({"triggerId":row["id"],"kind":kind,"enabled":row["enabled"],"config":cfg}),
    )
    .await?;
    tx.commit().await?;
    Ok(view(row))
}
pub async fn update(
    state: &AppState,
    actor: &Actor,
    server: &ServerScope,
    headers: &HeaderMap,
    id: &str,
    body: &Value,
) -> Result<Value> {
    if actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    let mut tx = state.db.begin().await?;
    let row: Value = sqlx::query_scalar(
        "SELECT to_jsonb(t) FROM triggers t WHERE server_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(&server.id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::missing)?;
    let kind = row["kind"].as_str().unwrap();
    let cfg = if let Some(c) = body.get("config") {
        policy::validate(kind, c)?
    } else {
        row["config"].clone()
    };
    require_rule(server, kind, &cfg)?;
    if !["name", "enabled", "config"]
        .iter()
        .any(|k| body.get(*k).is_some())
    {
        return Err(ApiError::bad("没有可更新的项目。"));
    }
    let name = body
        .get("name")
        .map(|v| string(v, 60))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| row["name"].as_str().unwrap_or("").into());
    let enabled = body
        .get("enabled")
        .map(truthy)
        .unwrap_or(row["enabled"] == true);
    let reset =
        kind == "ping_kick" && (body.get("config").is_some() || body.get("enabled").is_some());
    let updated:Value=sqlx::query_scalar("UPDATE triggers SET name=$3,enabled=$4,config=$5,state=CASE WHEN $6 THEN NULL ELSE state END,updated_at=GREATEST(date_trunc('milliseconds',clock_timestamp()),updated_at+interval '1 millisecond') WHERE server_id=$1 AND id=$2 RETURNING to_jsonb(triggers)").bind(&server.id).bind(id).bind(&name).bind(enabled).bind(&cfg).bind(reset).fetch_one(&mut *tx).await?;
    audit(
        &mut tx,
        actor,
        server,
        headers,
        "trigger.update",
        &name,
        &json!({"triggerId":id,"kind":kind,"name":name,"enabled":enabled,"config":cfg}),
    )
    .await?;
    tx.commit().await?;
    Ok(view(updated))
}
pub async fn delete(
    state: &AppState,
    actor: &Actor,
    server: &ServerScope,
    headers: &HeaderMap,
    id: &str,
) -> Result<()> {
    if actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    let mut tx = state.db.begin().await?;
    let row: Value = sqlx::query_scalar(
        "DELETE FROM triggers WHERE server_id=$1 AND id=$2 RETURNING to_jsonb(triggers)",
    )
    .bind(&server.id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::missing)?;
    audit(
        &mut tx,
        actor,
        server,
        headers,
        "trigger.delete",
        row["name"].as_str().unwrap_or(""),
        &json!({"triggerId":id,"kind":row["kind"]}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
#[derive(Clone)]
pub struct Intent {
    pub action: String,
    pub params: Value,
    pub target: String,
    pub detail: Value,
    pub steam: Option<String>,
    pub key: String,
    pub ok: String,
}
pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    server: &str,
    row: &Value,
    intents: &[Intent],
    state: Option<&Value>,
    ts: chrono::DateTime<chrono::Utc>,
) -> Result<u64> {
    let mut count = 0;
    for i in intents {
        let inserted=sqlx::query("INSERT INTO outbox(server_id,trigger_id,trigger_name,trigger_kind,action,params,target,detail,steam_id,dedupe_key,ok_message)SELECT $1,id,name,kind,$3,$4,$5,$6,$7,$8,$9 FROM triggers WHERE id=$2 AND server_id=$1 AND enabled AND updated_at=$10 ON CONFLICT(dedupe_key) DO NOTHING").bind(server).bind(row["id"].as_str()).bind(&i.action).bind(&i.params).bind(crate::feed::truncate(&i.target,300)).bind(&i.detail).bind(&i.steam).bind(&i.key).bind(&i.ok).bind(crate::integrity_enforcement::date(&row["updated_at"])).execute(&mut **tx).await?;
        count += inserted.rows_affected()
    }
    if !intents.is_empty() || state.is_some() {
        sqlx::query("UPDATE triggers SET state=CASE WHEN $3 THEN $4 ELSE state END,last_fired_at=CASE WHEN $5 THEN $6 ELSE last_fired_at END,last_result=CASE WHEN $5 THEN $7 ELSE last_result END WHERE id=$1 AND server_id=$2 AND enabled AND updated_at=$8").bind(row["id"].as_str()).bind(server).bind(state.is_some()).bind(state).bind(!intents.is_empty()).bind(ts).bind(crate::feed::truncate(&format!("已排队 {} 项操作",intents.len()),300)).bind(crate::integrity_enforcement::date(&row["updated_at"])).execute(&mut **tx).await?;
    }
    Ok(count)
}
