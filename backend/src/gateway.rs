//! Whole-operation relay. Never retry a command after a lost response.
use crate::{
    actions,
    config::AppState,
    error::ApiError,
    game::{self, Client},
};
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
pub fn credentials(headers: Option<&HeaderMap>) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(headers) = headers {
        for name in ["cookie", "authorization", "origin"] {
            if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) {
                out.insert(name.into(), json!(v));
            }
        }
    }
    Value::Object(out)
}
pub fn remote(state: &AppState) -> bool {
    state.config.worker.relay_url.is_some()
}
pub async fn call(state: &AppState, path: &str, body: Value) -> game::Result<Value> {
    let Some(url) = state.config.worker.relay_url.as_deref() else {
        return Err(ApiError::bad("Worker relay is not configured.").into());
    };
    let Some(secret) = state.config.worker.relay_secret.as_deref() else {
        return Err(unavailable());
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| unavailable())?;
    let mut response = client
        .post(format!("{}/relay/{path}", url.trim_end_matches('/')))
        .bearer_auth(secret)
        .json(&body)
        .send()
        .await
        .map_err(|_| unavailable())?;
    let http_status = response.status().as_u16();
    if response
        .content_length()
        .is_some_and(|n| n > 8 * 1024 * 1024)
    {
        return Err(unavailable());
    }
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err(unavailable());
        }
        bytes.extend(chunk)
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    if body["ok"] == true {
        return Ok(body["result"].clone());
    }
    let e = &body["error"];
    let status = e["status"]
        .as_u64()
        .and_then(|n| u16::try_from(n).ok())
        .unwrap_or(if http_status >= 400 { http_status } else { 503 });
    let message = e["message"]
        .as_str()
        .unwrap_or("Worker relay unavailable.")
        .to_owned();
    if e["kind"] == "game" {
        return Err(game::Error::Game(game::GameError {
            status,
            code: e["code"].as_str().unwrap_or("unreachable").into(),
            message,
            body: e["body"].clone(),
            retry_after_ms: e["retryAfterMs"].as_u64().unwrap_or(0),
        }));
    }
    // Wire codes are strings; map only known native codes instead of leaking arbitrary text.
    let code = match e["code"].as_str() {
        Some("forbidden") => "forbidden",
        Some("unauthenticated") => "unauthenticated",
        Some("not_found") => "not_found",
        Some("bad_request") => "bad_request",
        Some("worker_ownership_lost") => "worker_ownership_lost",
        _ => "worker_unavailable",
    };
    Err(ApiError::new(
        StatusCode::from_u16(status).unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
        code,
        message,
    )
    .into())
}
fn unavailable() -> game::Error {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "worker_unavailable",
        "Worker is unavailable; the command was not retried.",
    )
    .into()
}
/// The caller holds its local lane; a remote worker takes its own authoritative lane.
pub async fn run_held(
    state: &AppState,
    id: &str,
    name: &str,
    params: &Value,
    priority: u8,
    headers: Option<&HeaderMap>,
) -> game::Result<Value> {
    if remote(state) {
        call(state,"run",json!({"serverId":id,"action":name,"params":params,"priority":priority,"auth":credentials(headers)})).await
    } else {
        state.runtime.check().await?;
        let c = Client::for_server(state, id).await?;
        actions::run(&c, name, params).await
    }
}
pub async fn test_held(
    state: &AppState,
    id: &str,
    headers: Option<&HeaderMap>,
) -> game::Result<(Value, Value, String)> {
    if remote(state) {
        let v = call(
            state,
            "test",
            json!({"serverId":id,"auth":credentials(headers)}),
        )
        .await?;
        return Ok((
            v["status"].clone(),
            v["capabilities"].clone(),
            v["serverId"].as_str().unwrap_or("").into(),
        ));
    }
    state.runtime.check().await?;
    let c = Client::for_server(state, id).await?;
    let status = actions::run(&c, "status", &json!({"raw":true})).await?;
    let caps = actions::run(&c, "capabilities", &json!({}))
        .await
        .unwrap_or(Value::Null);
    let sid = if caps["features"]["serverId"] == true {
        actions::run(&c, "serverId", &json!({}))
            .await
            .ok()
            .and_then(|v| v["serverId"].as_str().map(str::to_owned))
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok((status, caps, sid))
}
