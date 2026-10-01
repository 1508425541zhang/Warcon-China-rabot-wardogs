pub mod analytics;
pub mod automation;
pub mod feed_setup;
pub mod ingest;
pub mod keys;
pub mod kills;
pub mod matches;
pub mod notes;
pub mod outbox;
pub mod roles;
pub mod settings;
use crate::config::AppState;
use axum::{
    Json, Router,
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
};
use serde_json::{Value, json};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health",get(health))
        .route("/api/settings",get(settings::get).put(settings::put))
        .route("/api/ingest/events",post(ingest::post))
        .route("/api/orgs/{id}/roles",get(roles::list).post(roles::create))
        .route("/api/orgs/{id}/keys",get(keys::list).post(keys::create))
        .route("/api/orgs/{id}/keys/{key_id}",delete(keys::revoke))
        .route("/api/servers/{id}/outbox",get(outbox::list))
        .route("/api/servers/{id}/analytics",get(analytics::analytics))
        .route("/api/servers/{id}/cash",get(analytics::cash))
        .route("/api/servers/{id}/feed",get(feed_setup::get).post(feed_setup::post).delete(feed_setup::delete))
        .route("/api/servers/{id}/numeric-limits",post(automation::numeric))
        .route("/api/servers/{id}/faction-lock",post(automation::faction_lock))
        .route("/api/servers/{id}/skill-balance",post(automation::skill_balance))
        .route("/api/servers/{id}/weapon-restrictions",post(automation::weapons))
        .route("/api/orgs/{id}/roles/{role_id}",axum::routing::patch(roles::update).delete(roles::delete))
        .route("/api/orgs/{id}/roles/{role_id}/reset",post(roles::reset))
        .route("/api/servers/{id}/kills",get(kills::list))
        .route("/api/servers/{id}/matches",get(matches::list))
        .route("/api/servers/{id}/matches/{match_id}",get(matches::detail))
        .route("/api/servers/{id}/players/{steam_id}/notes",post(notes::add))
        .route("/api/servers/{id}/players/{steam_id}/watch",axum::routing::put(notes::watch))
        .route("/api/servers/{id}/players/{steam_id}/notes/{note_id}",delete(notes::delete))
        .fallback(||async{(StatusCode::NOT_FOUND,Json(json!({"ok":false,"error":{"code":"not_found","message":"Route is not implemented by the Rust backend."}})))})
        .layer(middleware::from_fn(response_headers))
        .with_state(state)
}
async fn response_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}
async fn health() -> Json<Value> {
    Json(json!({"ok":true,"service":"warcon"}))
}
