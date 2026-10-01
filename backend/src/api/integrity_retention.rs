use crate::{
    api::roles::require_org_owner,
    auth::{authenticate, server_scope},
    config::AppState,
    error::{ApiError, Result},
    http::ApiJson,
    integrity_retention,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
pub async fn get(
    State(state): State<AppState>,
    Path(server): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let a = authenticate(&state, &headers, &Method::GET).await?;
    server_scope(&state, &a, &server, "integrity.view").await?;
    Ok(Json(integrity_retention::policy(&state, &server).await?))
}
pub async fn put(
    State(state): State<AppState>,
    Path(server): Path<String>,
    headers: HeaderMap,
    ApiJson(input): ApiJson<Value>,
) -> Result<Json<Value>> {
    let a = authenticate(&state, &headers, &Method::PUT).await?;
    let scope = server_scope(&state, &a, &server, "integrity.view").await?;
    require_org_owner(&state, &a, &scope.org_id).await?;
    let revision = input["revision"]
        .as_str()
        .ok_or_else(|| ApiError::bad("缺少保留设置版本。"))?;
    let mut value = integrity_retention::save(
        &state,
        &a,
        &headers,
        &scope.org_id,
        &server,
        &input["policy"],
        revision,
    )
    .await?;
    value["ok"] = json!(true);
    Ok(Json(value))
}
