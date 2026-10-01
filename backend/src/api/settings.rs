use crate::{
    auth::authenticate,
    config::AppState,
    error::{ApiError, Result},
    http::ApiJson,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
pub async fn get(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    if !actor.owner || actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    Ok(Json(
        json!({"ok":true,"settings":crate::settings::view(&state.db).await?}),
    ))
}
pub async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::PUT).await?;
    if !actor.owner || actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    let mut tx = state.db.begin().await?;
    let (changed, reset) = crate::settings::update(&mut tx, &actor.id, &body).await?;
    if !reset.is_empty() || changed.as_object().is_some_and(|m| !m.is_empty()) {
        let mut target = changed
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        target.extend(reset.iter().map(|s| format!("{s} (reset)")));
        sqlx::query("INSERT INTO audit_log(actor_id,actor_name,category,action,target,outcome,detail,user_agent) VALUES($1,$2,'system','settings.update',$3,'ok',$4,$5)").bind(&actor.id).bind(&actor.name).bind(target.join(", ")).bind(json!({"changed":changed,"reset":reset})).bind(headers.get("user-agent").and_then(|h|h.to_str().ok()).unwrap_or("")).execute(&mut *tx).await?;
        sqlx::query("SELECT pg_notify('warcon_settings','changed')")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"ok":true,"settings":crate::settings::view(&state.db).await?,"changed":changed,"reset":reset}),
    ))
}
