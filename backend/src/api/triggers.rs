use crate::{
    auth::{self, Actor, ServerScope},
    config::AppState,
    error::Result,
    http::ApiJson,
    trigger_store,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
async fn access(
    state: &AppState,
    id: &str,
    h: &HeaderMap,
    m: &Method,
) -> Result<(Actor, ServerScope)> {
    let a = auth::authenticate(state, h, m).await?;
    let s = auth::server_scope(state, &a, id, "automation.manage").await?;
    Ok((a, s))
}
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    access(&state, &id, &h, &Method::GET).await?;
    Ok(Json(
        json!({"ok":true,"triggers":trigger_store::list(&state,&id).await?}),
    ))
}
pub async fn post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let (a, s) = access(&state, &id, &h, &Method::POST).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"ok":true,"trigger":trigger_store::create(&state,&a,&s,&h,&b).await?})),
    ))
}
pub async fn patch(
    State(state): State<AppState>,
    Path((id, trigger)): Path<(String, String)>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (a, s) = access(&state, &id, &h, &Method::PATCH).await?;
    Ok(Json(
        json!({"ok":true,"trigger":trigger_store::update(&state,&a,&s,&h,&trigger,&b).await?}),
    ))
}
pub async fn delete(
    State(state): State<AppState>,
    Path((id, trigger)): Path<(String, String)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let (a, s) = access(&state, &id, &h, &Method::DELETE).await?;
    trigger_store::delete(&state, &a, &s, &h, &trigger).await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn dry_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (_, s) = access(&state, &id, &h, &Method::POST).await?;
    let kind = b["kind"].as_str().unwrap_or("");
    let cfg = crate::trigger_policy::validate(kind, &b["config"])?;
    trigger_store::require_rule(&s, kind, &cfg)?;
    Ok(Json(
        json!({"ok":true,"result":crate::trigger_replay::replay(&state,&s,kind,&cfg).await?}),
    ))
}
