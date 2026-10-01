//! Authenticated worker relay. All game operations use the worker's single dispatcher.
use crate::{
    actions, auth,
    config::AppState,
    dispatcher,
    error::{ApiError, Result},
    game, gateway,
    http::ApiJson,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::time::Duration;
use subtle::ConstantTimeEq;
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { Json(json!({"ok":true})) }))
        .route("/relay/run", post(run))
        .route("/relay/test", post(test))
        .route("/relay/live", post(live))
        .route("/relay/interest", post(interest))
        .route("/relay/observe-soon", post(observe_soon))
        .route("/relay/sync-org", post(sync_org))
        .route("/relay/sync-server", post(sync_server))
        .route("/relay/health", post(health))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .with_state(state)
}
async fn authorize(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response> {
    if request.uri().path() == "/health" {
        return Ok(next.run(request).await);
    }
    let supplied = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let valid = state
        .config
        .worker
        .relay_secret
        .as_deref()
        .is_some_and(|secret| {
            supplied
                .as_bytes()
                .ct_eq(format!("Bearer {secret}").as_bytes())
                .into()
        });
    if !valid {
        return Err(ApiError::unauthorized());
    }
    state.runtime.check().await?;
    Ok(next.run(request).await)
}
fn response(result: game::Result<Value>) -> Response {
    let body = match result {
        Ok(v) => json!({"ok":true,"result":v}),
        Err(game::Error::Api(e)) => {
            json!({"ok":false,"error":{"kind":"api","status":e.status.as_u16(),"code":e.code,"message":e.message}})
        }
        Err(game::Error::Game(e)) => {
            json!({"ok":false,"error":{"kind":"game","status":e.status,"code":e.code,"message":e.message,"body":e.body,"retryAfterMs":e.retry_after_ms}})
        }
    };
    Json(body).into_response()
}
fn id(p: &Value, key: &str) -> Result<String> {
    p[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 64)
        .map(str::to_owned)
        .ok_or_else(|| ApiError::bad(format!("{key} is required.")))
}
async fn actor_check(
    state: &AppState,
    p: &Value,
    id: &str,
    cap: &str,
    manager: bool,
) -> Result<()> {
    let Some(a) = p["auth"].as_object().filter(|a| !a.is_empty()) else {
        return Ok(());
    };
    let mut headers = HeaderMap::new();
    for key in ["cookie", "authorization", "origin"] {
        if let Some(v) = a.get(key).and_then(Value::as_str) {
            headers.insert(
                key,
                HeaderValue::from_str(v).map_err(|_| ApiError::bad("Invalid relay credential."))?,
            );
        }
    }
    let actor = auth::authenticate(state, &headers, &Method::POST).await?;
    let scope = auth::server_scope(state, &actor, id, cap).await?;
    if manager && !scope.manager {
        return Err(ApiError::forbidden());
    }
    Ok(())
}
async fn run(State(state): State<AppState>, ApiJson(p): ApiJson<Value>) -> Result<Response> {
    let id = id(&p, "serverId")?;
    let name = p["action"].as_str().unwrap_or("");
    let (cap, _) = actions::definition(name).ok_or_else(ApiError::missing)?;
    let priority = p["priority"].as_u64().unwrap_or(0).min(2) as u8;
    let _lane = dispatcher::acquire(&id, priority, Duration::from_secs(30)).await?;
    state.runtime.check().await?;
    actor_check(&state, &p, &id, cap, false).await?;
    Ok(response(
        gateway::run_held(&state, &id, name, &p["params"], priority, None).await,
    ))
}
async fn test(State(state): State<AppState>, ApiJson(p): ApiJson<Value>) -> Result<Response> {
    let id = id(&p, "serverId")?;
    let _lane = dispatcher::acquire(&id, 0, Duration::from_secs(30)).await?;
    state.runtime.check().await?;
    actor_check(&state, &p, &id, "server.view", true).await?;
    Ok(response(gateway::test_held(&state,&id,None).await.map(|(status,capabilities,server_id)|json!({"status":status,"capabilities":capabilities,"serverId":server_id}))))
}
fn ids(p: &Value) -> Result<Vec<String>> {
    let a = p["ids"]
        .as_array()
        .ok_or_else(|| ApiError::bad("ids must be an array."))?;
    if a.len() > 2000 {
        return Err(ApiError::bad("Too many servers."));
    }
    a.iter()
        .map(|s| {
            s.as_str()
                .filter(|s| !s.is_empty() && s.len() <= 64)
                .map(str::to_owned)
                .ok_or_else(|| ApiError::bad("Invalid server ID."))
        })
        .collect()
}
async fn live(State(state): State<AppState>, ApiJson(p): ApiJson<Value>) -> Result<Response> {
    Ok(response(Ok(json!(
        crate::live::read(&state, &ids(&p)?).await?
    ))))
}
async fn interest(State(state): State<AppState>, ApiJson(p): ApiJson<Value>) -> Result<Response> {
    let lease = crate::settings::load(&state.db)
        .await?
        .get("watchLeaseMs")
        .and_then(Value::as_u64)
        .unwrap_or(15000);
    state.runtime.interest(&ids(&p)?, lease);
    Ok(response(Ok(json!({}))))
}
async fn observe_soon(
    State(state): State<AppState>,
    ApiJson(p): ApiJson<Value>,
) -> Result<Response> {
    let id = id(&p, "serverId")?;
    sqlx::query("SELECT pg_notify('warcon_observe',$1)")
        .bind(json!({"serverId":id,"lists":p["lists"]==true}).to_string())
        .execute(&state.db)
        .await?;
    Ok(response(Ok(json!({}))))
}
async fn sync_server(
    State(state): State<AppState>,
    ApiJson(p): ApiJson<Value>,
) -> Result<Response> {
    let id = id(&p, "serverId")?;
    Ok(response(
        crate::list_sync::reconcile(
            &state,
            &id,
            p["priority"].as_u64().unwrap_or(0).min(2) as u8,
        )
        .await
        .map_err(Into::into),
    ))
}
async fn sync_org(State(state): State<AppState>, ApiJson(p): ApiJson<Value>) -> Result<Response> {
    let org = id(&p, "orgId")?;
    Ok(response(
        crate::list_sync::sync_org(&state, &org)
            .await
            .map_err(Into::into),
    ))
}
async fn health(State(state): State<AppState>) -> Result<Response> {
    Ok(response(Ok(
        json!({"owner":state.runtime.leader.get().is_some(),"stopping":state.runtime.stop.is_cancelled()}),
    )))
}
