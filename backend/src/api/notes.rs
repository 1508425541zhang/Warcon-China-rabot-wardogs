use crate::http::ApiJson;
use crate::{
    auth::{Actor, ServerScope, authenticate, server_scope},
    config::AppState,
    error::{ApiError, Result},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::Row;

pub fn steam_id(value: &str) -> Result<()> {
    if value.len() != 17 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApiError::bad("Invalid SteamID64."));
    }
    Ok(())
}
async fn audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &Actor,
    server: &ServerScope,
    steam: &str,
    action: &str,
    detail: Value,
    message: &str,
    headers: &HeaderMap,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,server_id,server_name,org_id,category,action,target,detail,outcome,message,user_agent) VALUES($1,$2,$3,$4,$5,'player',$6,$7,$8,'ok',$9,$10)")
        .bind(&actor.id).bind(&actor.name).bind(&server.id).bind(&server.name).bind(&server.org_id).bind(action).bind(steam).bind(detail).bind(message).bind(headers.get("user-agent").and_then(|h|h.to_str().ok()).unwrap_or(""))
        .execute(&mut **tx).await?;
    Ok(())
}
pub async fn add(
    State(state): State<AppState>,
    Path((id, steam)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let actor = authenticate(&state, &headers, &Method::POST).await?;
    let server = server_scope(&state, &actor, &id, "players.notes").await?;
    steam_id(&steam)?;
    let text = crate::http::string(&body["body"], 2000);
    if text.is_empty() {
        return Err(ApiError::bad("The note is empty."));
    }
    let mut tx = state.db.begin().await?;
    let row=sqlx::query("INSERT INTO player_notes(org_id,steam_id,author_id,author_name,body) VALUES($1,$2,$3,$4,$5) RETURNING id,created_at")
        .bind(&server.org_id).bind(&steam).bind(&actor.id).bind(&actor.name).bind(&text).fetch_one(&mut *tx).await?;
    let note_id: i64 = row.try_get("id")?;
    let created: DateTime<Utc> = row.try_get("created_at")?;
    audit(
        &mut tx,
        &actor,
        &server,
        &steam,
        "player.note",
        json!({"noteId":note_id}),
        &text.chars().take(200).collect::<String>(),
        &headers,
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"ok":true,"note":{"id":note_id,"authorId":actor.id,"authorName":actor.name,"body":text,"createdAt":created.to_rfc3339_opts(SecondsFormat::Millis,true),"deletable":true}}),
        ),
    ))
}
pub async fn delete(
    State(state): State<AppState>,
    Path((id, steam, note_id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::DELETE).await?;
    let server = server_scope(&state, &actor, &id, "players.notes").await?;
    steam_id(&steam)?;
    let mut tx = state.db.begin().await?;
    let row=sqlx::query("SELECT author_id,author_name FROM player_notes WHERE id=$1 AND org_id=$2 AND steam_id=$3 FOR UPDATE").bind(note_id).bind(&server.org_id).bind(&steam).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
    if row.try_get::<Option<String>, _>("author_id")?.as_deref() != Some(actor.id.as_str())
        && !server.caps.iter().any(|c| c == "players.notes.manage")
    {
        return Err(ApiError::forbidden());
    }
    sqlx::query("DELETE FROM player_notes WHERE id=$1 AND org_id=$2 AND steam_id=$3")
        .bind(note_id)
        .bind(&server.org_id)
        .bind(&steam)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &actor,
        &server,
        &steam,
        "player.note.delete",
        json!({"noteId":note_id,"author":row.try_get::<String,_>("author_name")?}),
        "",
        &headers,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn watch(
    State(state): State<AppState>,
    Path((id, steam)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::PUT).await?;
    let server = server_scope(&state, &actor, &id, "players.notes").await?;
    steam_id(&steam)?;
    let watched = crate::http::truthy(&body["watched"]);
    let reason = if watched {
        crate::http::string(&body["reason"], 300)
    } else {
        String::new()
    };
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO player_marks(org_id,steam_id,watched,reason,updated_by,updated_by_name,updated_at) VALUES($1,$2,$3,$4,$5,$6,now()) ON CONFLICT(org_id,steam_id) DO UPDATE SET watched=excluded.watched,reason=excluded.reason,updated_by=excluded.updated_by,updated_by_name=excluded.updated_by_name,updated_at=excluded.updated_at").bind(&server.org_id).bind(&steam).bind(watched).bind(&reason).bind(&actor.id).bind(&actor.name).execute(&mut *tx).await?;
    let message = if watched {
        if reason.is_empty() {
            "Added to the watchlist".into()
        } else {
            format!("Added to the watchlist: {reason}")
        }
    } else {
        "Removed from the watchlist".into()
    };
    audit(
        &mut tx,
        &actor,
        &server,
        &steam,
        "player.watch",
        json!({"watched":watched,"reason":reason}),
        &message,
        &headers,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
