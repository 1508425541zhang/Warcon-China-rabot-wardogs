use crate::{
    api::roles::require_org_owner,
    audit,
    auth::authenticate,
    config::AppState,
    crypto,
    error::{ApiError, Result},
    http::{ApiJson, string, truthy},
    webhooks as hooks,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
async fn row(tx: &mut Transaction<'_, Postgres>, org: &str, id: &str) -> Result<Value> {
    sqlx::query_scalar("SELECT to_jsonb(w) FROM webhooks w WHERE org_id=$1 AND id=$2 FOR UPDATE")
        .bind(org)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(ApiError::missing)
}
async fn scope(tx: &mut Transaction<'_, Postgres>, org: &str, v: &Value) -> Result<Option<Value>> {
    if v.is_null() {
        return Ok(None);
    }
    let raw = v
        .as_array()
        .filter(|a| a.iter().all(Value::is_string))
        .ok_or_else(|| ApiError::bad("请选择服务器列表或全部服务器。"))?;
    let mut ids: Vec<_> = raw
        .iter()
        .map(|v| string(v, 64))
        .filter(|s| !s.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err(ApiError::bad("请选择至少一个服务器。"));
    }
    let known: Vec<String> =
        sqlx::query_scalar("SELECT id FROM servers WHERE org_id=$1 AND id=ANY($2) FOR SHARE")
            .bind(org)
            .bind(&ids)
            .fetch_all(&mut **tx)
            .await?;
    if ids.len() != known.len() {
        return Err(ApiError::bad("选择的服务器不属于该组织。"));
    }
    Ok(Some(json!(ids)))
}
fn interval(v: &Value) -> i32 {
    if v.is_null() || v == "" {
        60
    } else {
        crate::live::finite(v)
            .map(|n| n.round().clamp(30., 300.) as i32)
            .unwrap_or(60)
    }
}
pub async fn list(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    require_org_owner(&state, &actor, &org).await?;
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(w) FROM webhooks w WHERE org_id=$1 ORDER BY created_at",
    )
    .bind(org)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        json!({"ok":true,"webhooks":rows.into_iter().map(hooks::view).collect::<Vec<_>>()}),
    ))
}
async fn save(
    state: &AppState,
    org: &str,
    id: Option<&str>,
    headers: &HeaderMap,
    body: &Value,
) -> Result<Value> {
    let actor = authenticate(
        state,
        headers,
        &if id.is_some() {
            Method::PATCH
        } else {
            Method::POST
        },
    )
    .await?;
    require_org_owner(state, &actor, org).await?;
    let body = body
        .as_object()
        .ok_or_else(|| ApiError::bad("Webhook 配置必须是对象。"))?;
    let mut tx = state.db.begin().await?;
    let old = if let Some(id) = id {
        row(&mut tx, org, id).await?
    } else {
        json!({"label":"Discord","events":[],"server_ids":null,"enabled":true,"status_enabled":false,"status_style":"banner","status_interval_s":60,"link_status":true,"link_leaderboard":true,"link_panel":false,"url_enc":"","url_hint":""})
    };
    let mut next = old.clone();
    let mut changes = json!({});
    for (camel, snake) in [
        ("enabled", "enabled"),
        ("statusEnabled", "status_enabled"),
        ("linkStatus", "link_status"),
        ("linkLeaderboard", "link_leaderboard"),
        ("linkPanel", "link_panel"),
    ] {
        if let Some(v) = body.get(camel) {
            next[snake] = json!(truthy(v));
            changes[camel] = next[snake].clone()
        }
    }
    if let Some(v) = body.get("label") {
        let label = string(v, 60);
        if !label.is_empty() {
            next["label"] = json!(label);
            changes["label"] = next["label"].clone()
        }
    }
    if let Some(v) = body.get("statusStyle") {
        if !["banner", "compact", "scoreboard"].contains(&hooks::text(v)) {
            return Err(ApiError::bad("请选择横幅、紧凑或计分板样式。"));
        }
        next["status_style"] = v.clone();
        changes["statusStyle"] = v.clone()
    }
    if let Some(v) = body.get("statusIntervalS") {
        next["status_interval_s"] = json!(interval(v));
        changes["statusIntervalS"] = next["status_interval_s"].clone()
    }
    if let Some(v) = body.get("events") {
        next["events"] = json!(
            hooks::EVENTS
                .iter()
                .filter(|e| v.as_array().is_some_and(|a| a.iter().any(|v| v == **e)))
                .collect::<Vec<_>>()
        );
        changes["events"] = next["events"].clone()
    }
    if next["status_enabled"] != true && next["events"].as_array().is_none_or(Vec::is_empty) {
        return Err(ApiError::bad("请选择至少一种事件，或启用状态卡片。"));
    }
    if let Some(v) = body.get("serverIds") {
        next["server_ids"] = json!(scope(&mut tx, org, v).await?);
        changes["serverIds"] = next["server_ids"].clone()
    }
    let mut drop = false;
    if let Some(v) = body
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        let (url, hint) = hooks::validate_url(v)?;
        next["url_enc"] = json!(
            crypto::encrypt_secret(&state.config.encryption_key, &url)
                .map_err(|_| ApiError::bad("无法保存 Webhook 密钥。"))?
        );
        next["url_hint"] = json!(hint);
        changes["hint"] = next["url_hint"].clone();
        drop = true;
    }
    if next["url_enc"] == "" {
        return Err(ApiError::bad("请输入 Discord Webhook 地址。"));
    }
    if id.is_some() && changes.as_object().unwrap().is_empty() {
        return Err(ApiError::bad("没有需要更新的配置。"));
    }
    drop |= next["enabled"] != true || next["status_enabled"] != true;
    if drop && id.is_some() {
        hooks::cleanup(&mut tx, &old, 0).await?;
    }
    let id = id
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let saved:Value=sqlx::query_scalar("INSERT INTO webhooks(id,org_id,label,url_enc,url_hint,events,server_ids,enabled,status_enabled,status_style,status_interval_s,link_status,link_leaderboard,link_panel,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) ON CONFLICT(id) DO UPDATE SET label=excluded.label,url_enc=excluded.url_enc,url_hint=excluded.url_hint,events=excluded.events,server_ids=excluded.server_ids,enabled=excluded.enabled,status_enabled=excluded.status_enabled,status_style=excluded.status_style,status_interval_s=excluded.status_interval_s,link_status=excluded.link_status,link_leaderboard=excluded.link_leaderboard,link_panel=excluded.link_panel,updated_at=clock_timestamp(),status_messages=CASE WHEN $16 THEN NULL ELSE webhooks.status_messages END,status_sent_at=CASE WHEN $16 THEN NULL ELSE webhooks.status_sent_at END,last_error=CASE WHEN $17 THEN '' ELSE webhooks.last_error END,last_status=CASE WHEN $17 THEN NULL ELSE webhooks.last_status END RETURNING to_jsonb(webhooks)")
        .bind(&id).bind(org).bind(hooks::text(&next["label"])).bind(hooks::text(&next["url_enc"])).bind(hooks::text(&next["url_hint"])).bind(&next["events"]).bind(if next["server_ids"].is_null(){None}else{Some(&next["server_ids"])}).bind(next["enabled"]==true).bind(next["status_enabled"]==true).bind(hooks::text(&next["status_style"])).bind(next["status_interval_s"].as_i64().unwrap_or(60) as i32).bind(next["link_status"]==true).bind(next["link_leaderboard"]==true).bind(next["link_panel"]==true).bind(&actor.id).bind(drop).bind(body.contains_key("url")).fetch_one(&mut *tx).await?;
    audit::org_event(
        &mut tx,
        &actor,
        org,
        headers,
        if old["id"].is_null() {
            "org.webhook.create"
        } else {
            "org.webhook.update"
        },
        hooks::text(&next["label"]),
        json!({"webhookId":id,"changes":changes}),
    )
    .await?;
    tx.commit().await?;
    Ok(hooks::view(saved))
}
pub async fn create(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(json!({"ok":true,"webhook":save(&state,&org,None,&headers,&body).await?})),
    ))
}
pub async fn update(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        json!({"ok":true,"webhook":save(&state,&org,Some(&id),&headers,&body).await?}),
    ))
}
pub async fn delete(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::DELETE).await?;
    require_org_owner(&state, &actor, &org).await?;
    let mut tx = state.db.begin().await?;
    let row = row(&mut tx, &org, &id).await?;
    hooks::cleanup(&mut tx, &row, 0).await?;
    sqlx::query("DELETE FROM webhooks WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "org.webhook.delete",
        hooks::text(&row["label"]),
        json!({"webhookId":id,"hint":row["url_hint"]}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn probe(
    state: AppState,
    org: String,
    id: String,
    headers: HeaderMap,
    server: Option<String>,
) -> Result<Response> {
    let actor = authenticate(&state, &headers, &Method::POST).await?;
    require_org_owner(&state, &actor, &org).await?;
    crate::ratelimit::allow(format!("webhook-test:{}", actor.id), 10, true)?;
    let mut tx = state.db.begin().await?;
    let row = row(&mut tx, &org, &id).await?;
    tx.commit().await?;
    let embed = if let Some(server) = server.as_deref() {
        if row["status_enabled"] != true || !hooks::scope(&row["server_ids"], Some(server)) {
            return Err(ApiError::bad("该 Webhook 未开启此服务器的状态卡片。"));
        }
        let name: String = sqlx::query_scalar("SELECT name FROM servers WHERE id=$1 AND org_id=$2")
            .bind(server)
            .bind(&org)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(ApiError::missing)?;
        crate::webhook_status::card(&state, &row, server, &name).await?
    } else {
        json!({"title":"Webhook 连接测试","description":format!("**{}** 已连接此频道。事件：{}",actor.name,row["events"]),"color":0xd4a843,"timestamp":hooks::stamp(),"footer":{"text":state.config.identity.app_name}})
    };
    let result = hooks::discord_call(
        &state,
        hooks::text(&row["url_enc"]),
        "POST",
        "?wait=true",
        Some(&hooks::payload(&state, embed)),
    )
    .await;
    let mut tx = state.db.begin().await?;
    hooks::record(&mut tx, &id, &result).await?;
    if server.is_some() {
        if let Some(message) = &result.message_id {
            let cleanup =
                json!({"id":id,"url_enc":row["url_enc"],"status_messages":{"test":message}});
            hooks::cleanup(&mut tx, &cleanup, 60000).await?
        }
    }
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        if server.is_some() {
            "org.webhook.testcard"
        } else {
            "org.webhook.test"
        },
        hooks::text(&row["label"]),
        json!({"webhookId":id,"status":result.status,"ok":result.ok}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        if result.ok {
            StatusCode::OK
        } else {
            StatusCode::BAD_GATEWAY
        },
        Json(json!({"ok":result.ok,"result":result})),
    )
        .into_response())
}
pub async fn test(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response> {
    probe(state, org, id, headers, None).await
}
pub async fn card(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    probe(state, org, id, headers, Some(string(&body["serverId"], 64))).await
}
