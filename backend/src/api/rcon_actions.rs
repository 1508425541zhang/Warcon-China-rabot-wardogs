use crate::{
    actions, audit,
    auth::{self, Actor, ServerScope},
    config::AppState,
    dispatcher,
    error::{ApiError, Result},
    game::{self, Client},
    http::{ApiJson, ApiQuery},
    ratelimit,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
pub async fn get(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<HashMap<String, String>>,
) -> Result<Response> {
    execute(state, id, name, headers, Method::GET, json!(query)).await
}
pub async fn post(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(params): ApiJson<Value>,
) -> Result<Response> {
    execute(state, id, name, headers, Method::POST, params).await
}
async fn trail(
    state: &AppState,
    actor: &Actor,
    scope: &ServerScope,
    headers: &HeaderMap,
    name: &str,
    p: &Value,
    outcome: &str,
    status: u16,
    message: &str,
    elapsed: u128,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,org_id,server_id,server_name,category,action,target,detail,outcome,status,message,user_agent,duration_ms) VALUES($1,$2,$3,$4,$5,'rcon',$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(&actor.id).bind(&actor.name).bind(&scope.org_id).bind(&scope.id).bind(&scope.name).bind(format!("rcon.{name}")).bind(crate::feed::truncate(&actions::target(name,p),300)).bind(audit::redact(&actions::audit_detail(name,p),0)).bind(outcome).bind(status as i32).bind(crate::feed::truncate(message.into(),1000)).bind(crate::feed::truncate(headers.get("user-agent").and_then(|v|v.to_str().ok()).unwrap_or("").into(),300)).bind(elapsed.min(i32::MAX as u128) as i32).execute(&state.db).await?;
    Ok(())
}
async fn execute(
    state: AppState,
    id: String,
    name: String,
    headers: HeaderMap,
    method: Method,
    mut params: Value,
) -> Result<Response> {
    if !params.is_object() {
        return Err(ApiError::bad("Action parameters must be a JSON object."));
    }
    let (cap, mutating) = actions::definition(&name).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "unknown_action",
            "Unknown RCON action.",
        )
    })?;
    if method == Method::GET && mutating {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "post_required",
            "Mutating actions must be POSTed.",
        ));
    }
    let actor = auth::authenticate(&state, &headers, &method).await?;
    let scope = auth::server_scope(&state, &actor, &id, "server.view").await?;
    if !scope.caps.iter().any(|c| c == cap) {
        trail(
            &state,
            &actor,
            &scope,
            &headers,
            &name,
            &params,
            "denied",
            403,
            &format!("Needs '{cap}'"),
            0,
        )
        .await?;
        return Err(ApiError::forbidden());
    }
    if name == "raw" {
        ratelimit::allow(format!("raw:{}", actor.id), 30, true)?
    }
    ratelimit::allow(format!("rcon:{}", actor.id), 120, true)?;
    let started = Instant::now();
    let _lane = dispatcher::acquire(&id, 0, Duration::from_secs(30)).await?;
    // Permission may have changed while the request waited in its server's lane.
    let actor = auth::authenticate(&state, &headers, &method).await?;
    let scope = auth::server_scope(&state, &actor, &id, cap).await?;
    let unshaped = scope.caps.iter().any(|c| c == "config.apply");
    if name == "status" && !unshaped {
        params.as_object_mut().map(|p| p.remove("raw"));
    }
    let role = if actor.key.is_some() {
        "API key".into()
    } else if actor.owner {
        "owner".into()
    } else if scope.manager {
        "owner".into()
    } else {
        sqlx::query_scalar::<_,String>("SELECT r.name FROM server_grants g JOIN org_roles r ON r.id=g.role_id WHERE g.server_id=$1 AND g.user_id=$2").bind(&id).bind(&actor.id).fetch_optional(&state.db).await?.unwrap_or_default()
    };
    let result = match Client::for_server(&state, &id).await {
        Ok(client) => actions::run(&client, &name, &params).await,
        Err(e) => Err(e),
    };
    let audit_reads = std::env::var("AUDIT_LOG_READS").is_ok_and(|v| v == "true" || v == "1");
    let elapsed = started.elapsed().as_millis();
    match result {
        Ok(mut result) => {
            if name == "capabilities" && !unshaped {
                result.as_object_mut().map(|r| r.remove("raw"));
            }
            if name == "serverLog" && !actor.owner {
                if let Some(entries) = result["entries"].as_array_mut() {
                    for e in entries {
                        e["peer"] = json!("");
                    }
                }
            }
            if mutating {
                // The game has already applied the action. A cache/notification failure must
                // not return failure and encourage replay of a successful mutation.
                if mirror(&state, &id, &name, &params).await.is_err() {
                    tracing::error!(server_id=%id,action=%name,"RCON mirror failed after successful delivery");
                }
                let _=sqlx::query("SELECT pg_notify('warcon_observe',$1)").bind(json!({"serverId":id,"lists":(["ban","unban","reservedAdd","reservedRemove"].contains(&name.as_str())),"identity":name=="configApply"}).to_string()).execute(&state.db).await;
            }
            if mutating || audit_reads {
                let mut detail = params.clone();
                if let Some(via) = result.get("via") {
                    detail["via"] = via.clone();
                    detail["revision"] = result["revision"].clone();
                }
                trail(
                    &state,
                    &actor,
                    &scope,
                    &headers,
                    &name,
                    &detail,
                    "ok",
                    200,
                    result["message"].as_str().unwrap_or(""),
                    elapsed,
                )
                .await?;
            }
            Ok(Json(
                json!({"ok":true,"action":name,"role":role,"result":result,"durationMs":elapsed}),
            )
            .into_response())
        }
        Err(game::Error::Api(e)) => {
            if mutating {
                trail(
                    &state,
                    &actor,
                    &scope,
                    &headers,
                    &name,
                    &params,
                    "error",
                    e.status.as_u16(),
                    &e.message,
                    elapsed,
                )
                .await?;
            }
            Err(e)
        }
        Err(game::Error::Game(e)) => {
            if mutating || audit_reads || e.status == 401 {
                trail(
                    &state, &actor, &scope, &headers, &name, &params, "error", e.status,
                    &e.message, elapsed,
                )
                .await?;
            }
            let status = if e.status == 401 || e.status >= 500 {
                502
            } else {
                e.status
            };
            let mut error = json!({"message":if e.status==401{format!("The game server rejected the stored RCON password: {}",e.message)}else{e.message},"code":e.code,"upstreamStatus":e.status});
            if ["config.apply", "rcon.raw"].contains(&cap) {
                error["body"] = e.body;
            }
            let mut response = (
                StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
                Json(json!({"ok":false,"action":name,"error":error})),
            )
                .into_response();
            if e.retry_after_ms > 0 {
                response.headers_mut().insert(
                    "retry-after",
                    e.retry_after_ms.div_ceil(1000).to_string().parse().unwrap(),
                );
            }
            Ok(response)
        }
    }
}
pub async fn system(
    state: &AppState,
    id: &str,
    name: &str,
    params: &Value,
    priority: u8,
) -> game::Result<Value> {
    if actions::definition(name).is_none() {
        return Err(ApiError::missing().into());
    }
    let _lane = dispatcher::acquire(id, priority, Duration::from_secs(30)).await?;
    let c = Client::for_server(state, id).await?;
    actions::run(&c, name, params).await
}
async fn mirror(state: &AppState, id: &str, name: &str, params: &Value) -> Result<()> {
    let steam = params["steamId"].as_str().unwrap_or("");
    if crate::api::notes::steam_id(steam).is_err() {
        return Ok(());
    }
    match name {
        "ban" => {
            sqlx::query("INSERT INTO server_bans(server_id,steam_id,reason,seen_at) VALUES($1,$2,$3,now()) ON CONFLICT(server_id,steam_id) DO UPDATE SET reason=excluded.reason,seen_at=excluded.seen_at").bind(id).bind(steam).bind(crate::http::string(&params["reason"],200)).execute(&state.db).await?;
        }
        "unban" => {
            sqlx::query("DELETE FROM server_bans WHERE server_id=$1 AND steam_id=$2")
                .bind(id)
                .bind(steam)
                .execute(&state.db)
                .await?;
        }
        "reservedAdd" => {
            sqlx::query("INSERT INTO server_reserved(server_id,steam_id,seen_at) VALUES($1,$2,now()) ON CONFLICT(server_id,steam_id) DO UPDATE SET seen_at=excluded.seen_at").bind(id).bind(steam).execute(&state.db).await?;
        }
        "reservedRemove" => {
            sqlx::query("DELETE FROM server_reserved WHERE server_id=$1 AND steam_id=$2")
                .bind(id)
                .bind(steam)
                .execute(&state.db)
                .await?;
        }
        _ => (),
    }
    Ok(())
}
