use crate::ratelimit::allow;
use crate::{
    config::AppState,
    crypto::hash_token,
    error::{ApiError, Result},
    feed,
};
use axum::{
    Json,
    body::to_bytes,
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
};
use serde_json::{Value, json};
use std::net::SocketAddr;
pub async fn post(State(state): State<AppState>, request: Request) -> Result<Json<Value>> {
    // Socket metadata is authoritative. Untrusted Forwarded/X-Forwarded-For is ignored.
    let ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|p| p.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into());
    allow(format!("ip:{ip}"), 6000, true)?;
    allow(format!("bad:{ip}"), 100, false)?;
    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(feed::bearer);
    let server: Option<String> = if let Some(token) = token {
        sqlx::query_scalar("SELECT id FROM servers WHERE feed_token_hash=$1")
            .bind(hash_token(token))
            .fetch_optional(&state.db)
            .await?
    } else {
        None
    };
    let Some(server) = server else {
        allow(format!("bad:{ip}"), 100, true)?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Unknown kill feed token.",
        ));
    };
    allow(format!("server:{server}"), 1200, true)?;
    let bytes = to_bytes(request.into_body(), 65536).await.map_err(|_| {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body_size",
            "Batch too large.",
        )
    })?;
    let body: Value =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::bad("Malformed JSON body."))?;
    let receipt = feed::ingest(&state.db, &server, body, chrono::Utc::now()).await?;
    crate::diagnostics::feed(
        receipt.accepted as u64,
        receipt.skipped as u64,
        receipt.duplicates as u64,
    );
    Ok(Json(
        json!({"ok":true,"accepted":receipt.accepted,"skipped":receipt.skipped,"duplicates":receipt.duplicates}),
    ))
}
