use crate::{auth, config::AppState, error::Result, http::ApiJson, webhooks::text};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
pub async fn post(
    State(s): State<AppState>,
    h: HeaderMap,
    ApiJson(v): ApiJson<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>)> {
    let a = auth::authenticate(&s, &h, &Method::POST).await?;
    let report = crate::integrity_reports::submit(
        &s,
        &a,
        &h,
        text(&v["serverId"]),
        text(&v["target"]),
        text(&v["reason"]),
        None,
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"ok":true,"report":report})),
    ))
}
