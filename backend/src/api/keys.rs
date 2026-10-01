use super::roles::require_org_owner;
use crate::{
    audit::org_event,
    auth::{authenticate, known_capabilities, parse_capabilities},
    config::AppState,
    crypto::{hash_token, mint_token},
    error::{ApiError, Result},
    http::{ApiJson, string},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgRow};

fn iso(row: &PgRow, column: &str) -> Result<Option<String>> {
    Ok(row
        .try_get::<Option<DateTime<Utc>>, _>(column)?
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Millis, true)))
}
fn shape(row: &PgRow) -> Result<Value> {
    let creator: Option<String> = row.try_get("creator_name")?;
    let caps: Value = row.try_get("capabilities")?;
    let ids: Option<Value> = row.try_get("server_ids")?;
    Ok(
        json!({"id":row.try_get::<String,_>("id")?,"label":row.try_get::<String,_>("label")?,"hint":row.try_get::<String,_>("hint")?,"capabilities":known_capabilities(&caps),"serverIds":ids,
        "createdBy":creator.map(|name|json!({"name":name,"username":row.try_get::<Option<String>,_>("creator_username").ok().flatten().unwrap_or_default()})),
        "createdAt":iso(row,"created_at")?,"lastUsedAt":iso(row,"last_used_at")?,"expiresAt":iso(row,"expires_at")?,"revokedAt":iso(row,"revoked_at")?}),
    )
}
pub async fn list(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    require_org_owner(&state, &actor, &org).await?;
    let rows=sqlx::query("SELECT k.*,u.name creator_name,u.username creator_username FROM api_keys k LEFT JOIN \"user\" u ON u.id=k.created_by WHERE org_id=$1 ORDER BY k.created_at").bind(&org).fetch_all(&state.db).await?;
    Ok(Json(
        json!({"ok":true,"keys":rows.iter().map(shape).collect::<Result<Vec<_>>>()?}),
    ))
}
pub async fn create(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let actor = authenticate(&state, &headers, &Method::POST).await?;
    require_org_owner(&state, &actor, &org).await?;
    let label = string(&body["label"], 60);
    if label.encode_utf16().count() < 2 {
        return Err(ApiError::bad(
            "Give the key a label of at least 2 characters.",
        ));
    }
    let caps = parse_capabilities(body.get("capabilities").unwrap_or(&json!([])))?;
    if caps.is_empty() {
        return Err(ApiError::bad("Pick at least one capability for the key."));
    }
    let mut tx = state.db.begin().await?;
    // Serialise changes to the scope with server deletion or organisation administration.
    sqlx::query("SELECT id FROM organizations WHERE id=$1 FOR UPDATE")
        .bind(&org)
        .fetch_one(&mut *tx)
        .await?;
    let server_ids = if body["serverIds"].is_null() {
        None
    } else {
        let raw = body["serverIds"].as_array().ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_scope",
                "Pick at least one server, or choose every server.",
            )
        })?;
        if raw.iter().any(|s| !s.is_string()) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_scope",
                "Server scope must be a list of strings.",
            ));
        }
        let mut wanted = raw
            .iter()
            .map(|v| string(v, 64))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        wanted.sort();
        wanted.dedup();
        if wanted.is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_scope",
                "Pick at least one server, or choose every server.",
            ));
        }
        let known: Vec<String> =
            sqlx::query_scalar("SELECT id FROM servers WHERE org_id=$1 AND id=ANY($2) FOR SHARE")
                .bind(&org)
                .bind(&wanted)
                .fetch_all(&mut *tx)
                .await?;
        if known.len() != wanted.len() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_scope",
                "One of those servers is not in this organisation.",
            ));
        }
        if caps
            .iter()
            .any(|s| ["lists.ban", "lists.reserve"].contains(&s.as_str()))
        {
            return Err(ApiError::bad(
                "Organisation list capabilities cannot be assigned to a key limited to some servers.",
            ));
        }
        Some(json!(known))
    };
    let days = crate::http::integer(&body["expiresDays"], 0, 0, 3650);
    let expires = (days > 0).then(|| Utc::now() + chrono::Duration::days(days));
    let token = mint_token();
    let hint = format!("{}…", &token[..12]);
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO api_keys(id,org_id,label,key_hash,hint,capabilities,server_ids,created_by,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(&id).bind(&org).bind(&label).bind(hash_token(&token)).bind(&hint).bind(json!(caps)).bind(&server_ids).bind(&actor.id).bind(expires).execute(&mut *tx).await?;
    org_event(&mut tx,&actor,&org,&headers,"org.apikey.create",&label,json!({"orgId":org,"keyId":id,"hint":hint,"capabilities":caps,"serverIds":server_ids,"expiresAt":expires.map(|t|t.to_rfc3339_opts(SecondsFormat::Millis,true))})).await?;
    let row=sqlx::query("SELECT k.*,u.name creator_name,u.username creator_username FROM api_keys k LEFT JOIN \"user\" u ON u.id=k.created_by WHERE k.id=$1").bind(&id).fetch_one(&mut *tx).await?;
    let key = shape(&row)?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"ok":true,"key":key,"token":token})),
    ))
}
pub async fn revoke(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::DELETE).await?;
    require_org_owner(&state, &actor, &org).await?;
    let mut tx = state.db.begin().await?;
    let row=sqlx::query("UPDATE api_keys SET revoked_at=now() WHERE org_id=$1 AND id=$2 AND revoked_at IS NULL RETURNING label,hint").bind(&org).bind(&id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
    org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "org.apikey.revoke",
        &row.try_get::<String, _>("label")?,
        json!({"orgId":org,"keyId":id,"hint":row.try_get::<String,_>("hint")?}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
