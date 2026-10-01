use crate::{
    api::roles::require_org_owner, auth::authenticate, config::AppState, error::Result,
    http::ApiJson,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
pub async fn label(
    State(state): State<AppState>,
    Path((org, case)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::POST).await?;
    require_org_owner(&state, &actor, &org).await?;
    Ok(Json(
        json!({"ok":true,"label":crate::integrity_cases::label(&state,&headers,&actor,&org,&case,&body["label"],&body["reason"]).await?}),
    ))
}
