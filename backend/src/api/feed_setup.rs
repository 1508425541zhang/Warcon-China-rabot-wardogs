use super::roles::require_org_owner;
use crate::{
    auth::{authenticate, server_scope},
    config::AppState,
    crypto::{decrypt_secret, encrypt_secret, hash_token, mint_token},
    error::{ApiError, Result},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::Row;
async fn view(state: &AppState, id: &str, reveal: bool) -> Result<Value> {
    let row=sqlx::query("SELECT s.feed_token_hash,s.feed_token_enc,l.feed_at FROM servers s LEFT JOIN server_live l ON l.server_id=s.id WHERE s.id=$1").bind(id).fetch_one(&state.db).await?;
    let enc: Option<String> = row.try_get("feed_token_enc")?;
    let token = if reveal {
        enc.map(|v| decrypt_secret(&state.config.encryption_key, &v))
            .transpose()
            .map_err(|_| {
                ApiError::new(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "secret_unavailable",
                    "Unable to read the stored feed token.",
                )
            })?
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(
        json!({"ok":true,"configured":row.try_get::<Option<String>,_>("feed_token_hash")?.is_some_and(|s|!s.is_empty()),"url":state.config.origin,"token":token,"feedAt":row.try_get::<Option<DateTime<Utc>>,_>("feed_at")?.map(|t|t.to_rfc3339_opts(SecondsFormat::Millis,true))}),
    )
}
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    let server = server_scope(&state, &actor, &id, "server.view").await?;
    let reveal=actor.key.is_none() && (actor.owner || sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM org_members WHERE org_id=$1 AND user_id=$2 AND role='owner')").bind(&server.org_id).bind(&actor.id).fetch_one(&state.db).await?);
    Ok(Json(view(&state, &id, reveal).await?))
}
pub async fn post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    edit(state, id, headers, false).await
}
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    edit(state, id, headers, true).await
}
async fn edit(
    state: AppState,
    id: String,
    headers: HeaderMap,
    remove: bool,
) -> Result<Json<Value>> {
    let method = if remove { Method::DELETE } else { Method::POST };
    let actor = authenticate(&state, &headers, &method).await?;
    let server = server_scope(&state, &actor, &id, "server.view").await?;
    require_org_owner(&state, &actor, &server.org_id).await?;
    let mut tx = state.db.begin().await?;
    let old: Option<String> =
        sqlx::query_scalar("SELECT feed_token_hash FROM servers WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
    if remove && old.is_none() {
        return Err(ApiError::bad("This server has no kill feed token."));
    }
    let token = if remove {
        None
    } else {
        Some(mint_token().replacen("wck_", "wkf_", 1))
    };
    let enc = token
        .as_ref()
        .map(|t| encrypt_secret(&state.config.encryption_key, t))
        .transpose()
        .map_err(|_| ApiError::bad("Invalid encryption configuration."))?;
    sqlx::query("UPDATE servers SET feed_token_hash=$2,feed_token_enc=$3 WHERE id=$1")
        .bind(&id)
        .bind(token.as_ref().map(|t| hash_token(t)))
        .bind(enc)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,org_id,server_id,server_name,category,action,outcome,user_agent) VALUES($1,$2,$3,$4,$5,'server',$6,'ok',$7)").bind(&actor.id).bind(&actor.name).bind(&server.org_id).bind(&id).bind(&server.name).bind(if remove {"feed.disable"}else if old.is_some(){"feed.rotate"}else{"feed.enable"}).bind(headers.get("user-agent").and_then(|v|v.to_str().ok()).unwrap_or("")).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(if remove {
        json!({"ok":true,"configured":false})
    } else {
        view(&state, &id, true).await?
    }))
}
