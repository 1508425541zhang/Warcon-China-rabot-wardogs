use crate::{
    auth,
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
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let actor = auth::authenticate(&state, &h, &Method::GET).await?;
    auth::server_scope(&state, &actor, &id, "automation.manage").await?;
    Ok(Json(crate::faction_quota::view(&state, &id).await?))
}
pub async fn post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiJson(input): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = auth::authenticate(&state, &h, &Method::POST).await?;
    let server = auth::server_scope(&state, &actor, &id, "automation.manage").await?;
    if ["players.moderate", "config.apply"]
        .iter()
        .any(|c| !server.caps.iter().any(|cap| cap == c))
    {
        return Err(ApiError::forbidden());
    }
    Ok(Json(
        json!({"config":crate::faction_quota::save(&state,&actor,&server,&h,&input).await?}),
    ))
}
