use crate::{
    auth::{Actor, ServerScope, authenticate, server_scope},
    automation_policy::{NumericLimits, integer_range},
    config::AppState,
    error::{ApiError, Result},
    http::ApiJson,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
async fn access(
    state: &AppState,
    id: &str,
    headers: &HeaderMap,
    chat: bool,
) -> Result<(Actor, ServerScope)> {
    let actor = authenticate(state, headers, &Method::POST).await?;
    let server = server_scope(state, &actor, id, "automation.manage").await?;
    if !server.caps.iter().any(|c| c == "players.moderate")
        || (chat && !server.caps.iter().any(|c| c == "chat.send"))
    {
        return Err(ApiError::forbidden());
    }
    Ok((actor, server))
}
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    server: &ServerScope,
    headers: &HeaderMap,
    action: &str,
    detail: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,server_id,server_name,org_id,category,action,outcome,detail,user_agent) VALUES($1,$2,$3,$4,$5,'trigger',$6,'ok',$7,$8)")
        .bind(&actor.id).bind(&actor.name).bind(&server.id).bind(&server.name).bind(&server.org_id).bind(action).bind(detail).bind(headers.get("user-agent").and_then(|v|v.to_str().ok()).unwrap_or("")).execute(&mut **tx).await?;
    Ok(())
}
pub async fn numeric(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (actor, server) = access(&state, &id, &headers, false).await?;
    let config = NumericLimits::parse(&body).ok_or_else(|| {
        ApiError::bad(
            "窗口为60～900秒，最低击杀数为1～1000；上限需填写正数，不启用的项目设为null。",
        )
    })?;
    let config = json!(config);
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO numeric_limit_rules(server_id,config,updated_at) VALUES($1,$2,now()) ON CONFLICT(server_id) DO UPDATE SET config=excluded.config,updated_at=excluded.updated_at").bind(&id).bind(&config).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        &server,
        &headers,
        "numeric_limit.settings",
        config,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn faction_lock(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (actor, server) = access(&state, &id, &headers, true).await?;
    let enabled = body["enabled"]
        .as_bool()
        .ok_or_else(|| ApiError::bad("保护时间应为30～600秒。"))?;
    if !integer_range(&body["graceSeconds"], 30, 600) {
        return Err(ApiError::bad("保护时间应为30～600秒。"));
    }
    let grace = body["graceSeconds"].as_f64().unwrap() as i32;
    let capacities = body["capacities"]
        .as_object()
        .ok_or_else(|| ApiError::bad("阵营容量格式错误。"))?;
    if capacities.iter().any(|(name, v)| {
        name.is_empty() || name.encode_utf16().count() > 100 || !integer_range(v, 1, 1000)
    }) {
        return Err(ApiError::bad("阵营容量应为1～1000的整数；未知请留空。"));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO faction_lock_rules(server_id,enabled,grace_seconds,capacities,updated_at) VALUES($1,$2,$3,$4,now()) ON CONFLICT(server_id) DO UPDATE SET enabled=excluded.enabled,grace_seconds=excluded.grace_seconds,capacities=excluded.capacities,updated_at=excluded.updated_at").bind(&id).bind(enabled).bind(grace).bind(json!(capacities)).execute(&mut *tx).await?;
    if !enabled {
        sqlx::query("UPDATE faction_lock_events SET state='skipped',reason='管理员已关闭功能',updated_at=now() WHERE server_id=$1 AND state IN ('pending','warned')").bind(&id).execute(&mut *tx).await?;
    }
    audit(
        &mut tx,
        &actor,
        &server,
        &headers,
        "faction_lock.settings",
        json!({"serverId":id,"enabled":enabled,"graceSeconds":grace,"capacities":capacities}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn skill_balance(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (actor, server) = access(&state, &id, &headers, false).await?;
    let enabled = body["enabled"]
        .as_bool()
        .ok_or_else(|| ApiError::bad("开局保护应为180～1800秒；领先分差应为40～10000分。"))?;
    if !integer_range(&body["graceSeconds"], 180, 1800)
        || !integer_range(&body["leadPoints"], 40, 10000)
    {
        return Err(ApiError::bad(
            "开局保护应为180～1800秒；领先分差应为40～10000分。",
        ));
    }
    let grace = body["graceSeconds"].as_f64().unwrap() as i32;
    let lead = body["leadPoints"].as_f64().unwrap() as i32;
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO skill_balance_rules(server_id,enabled,grace_seconds,lead_points,updated_at) VALUES($1,$2,$3,$4,now()) ON CONFLICT(server_id) DO UPDATE SET enabled=excluded.enabled,grace_seconds=excluded.grace_seconds,lead_points=excluded.lead_points,updated_at=excluded.updated_at").bind(&id).bind(enabled).bind(grace).bind(lead).execute(&mut *tx).await?;
    sqlx::query("UPDATE skill_balance_runs SET state='cancelled',reason=$2,updated_at=now() WHERE server_id=$1 AND state IN ('waiting_safe','waiting_death')").bind(&id).bind(if enabled {"配置已变更，本局计划停止，等待下一局"}else{"管理员已关闭平衡；剩余玩家停止调队"}).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        &server,
        &headers,
        "skill_balance.settings",
        json!({"serverId":id,"enabled":enabled,"graceSeconds":grace,"leadPoints":lead}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn weapons(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (actor, server) = access(&state, &id, &headers, false).await?;
    let bad = || ApiError::bad("来源列表格式不正确，请每项填写完整来源ID。");
    let enabled = body["enabled"].as_bool().ok_or_else(bad)?;
    let causes = body["causes"]
        .as_array()
        .filter(|v| v.len() <= 1000)
        .ok_or_else(bad)?;
    let groups = body["groups"]
        .as_array()
        .filter(|v| v.len() <= 3)
        .ok_or_else(bad)?;
    let mut cleaned = Vec::new();
    for cause in causes {
        let s = cause.as_str().ok_or_else(bad)?.trim();
        if s.is_empty()
            || s.encode_utf16().count() > 200
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || [b'_', b'.', b'-'].contains(&b))
        {
            return Err(bad());
        }
        if !cleaned.contains(&s.to_owned()) {
            cleaned.push(s.to_owned());
        }
    }
    let mut categories = Vec::new();
    for group in groups {
        let s = group
            .as_str()
            .filter(|s| ["items", "vehicles", "buildables"].contains(s))
            .ok_or_else(bad)?;
        if !categories.contains(&s) {
            categories.push(s);
        }
    }
    if enabled && cleaned.is_empty() && categories.is_empty() {
        return Err(ApiError::bad("请至少选择一个受限来源。"));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO weapon_restriction_rules(server_id,enabled,causes,groups,updated_at) VALUES($1,$2,$3,$4,now()) ON CONFLICT(server_id) DO UPDATE SET enabled=excluded.enabled,causes=excluded.causes,groups=excluded.groups,updated_at=excluded.updated_at").bind(&id).bind(enabled).bind(json!(cleaned)).bind(json!(categories)).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        &server,
        &headers,
        "weapon_restriction.settings",
        json!({"serverId":id,"enabled":enabled,"causes":cleaned,"groups":categories}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
