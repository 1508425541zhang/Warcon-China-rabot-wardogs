use crate::{
    audit,
    auth::{Actor, authenticate},
    config::AppState,
    error::{ApiError, Result},
    http::ApiJson,
    qq_config, qq_transport,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
async fn owner(state: &AppState, headers: &HeaderMap, method: &Method) -> Result<Actor> {
    let a = authenticate(state, headers, method).await?;
    if !a.owner || a.key.is_some() {
        return Err(ApiError::forbidden());
    }
    Ok(a)
}
pub async fn get(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    owner(&state, &headers, &Method::GET).await?;
    Ok(Json(
        json!({"ok":true,"config":qq_config::view(&qq_config::load(&state).await?)}),
    ))
}
pub async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(input): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = owner(&state, &headers, &Method::PUT).await?;
    let config = qq_config::save(&state, &actor, &headers, &input).await?;
    Ok(Json(json!({"ok":true,"config":config})))
}
pub async fn test(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let actor = owner(&state, &headers, &Method::POST).await?;
    crate::ratelimit::allow(format!("qq-probe:{}", actor.id), 10, true)?;
    let c = qq_config::load(&state).await?;
    if !c.active() {
        return Err(ApiError::bad("请先保存完整 QQ 连接设置。"));
    }
    if !qq_transport::logged_in(&c).await {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "qq_unavailable",
            "连接失败或机器人身份不匹配，请检查登录状态、地址和密钥。",
        ));
    }
    Ok(Json(
        json!({"ok":true,"selfId":c.stored.self_id,"message":"QQ 鉴权通过，机器人身份匹配。"}),
    ))
}
pub async fn get_vips(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    owner(&state, &headers, &Method::GET).await?;
    let mut db = state.db.acquire().await?;
    Ok(Json(
        json!({"ok":true,"vips":qq_config::vips(&mut db).await?}),
    ))
}
pub async fn put_vips(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(input): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = owner(&state, &headers, &Method::PUT).await?;
    let mut tx = state.db.begin().await?;
    let value = qq_config::save_vips(&mut tx, &actor.id, &input).await?;
    audit::event(
        &mut tx,
        &actor,
        None,
        &headers,
        "system",
        "qq.vip.update",
        "qqVip",
        value.clone(),
    )
    .await?;
    sqlx::query("SELECT pg_notify('warcon_wake',$1)")
        .bind(json!({"lists":true}).to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"vips":value})))
}
pub async fn webhook(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse;
    let c = qq_config::load(&state).await?;
    if !c.active() {
        return Err(ApiError::missing());
    }
    let header = if c.stored.provider == "official" {
        "x-bot-appid"
    } else {
        "x-self-id"
    };
    let headers = request.headers().clone();
    if headers.get(header).and_then(|v| v.to_str().ok()) != Some(c.stored.self_id.as_str()) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "wrong_bot",
            "机器人身份不匹配。",
        ));
    }
    let raw = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "QQ 事件上限为 64 KiB。",
            )
        })?;
    let now = chrono::Utc::now().timestamp_millis();
    if c.stored.provider != "official"
        && !crate::qq_protocol::verify_onebot(&c.secret, &headers, &raw)
    {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "QQ 事件签名无效。",
        ));
    }
    let v: Value =
        serde_json::from_slice(&raw).map_err(|_| ApiError::bad("QQ 事件 JSON 无效。"))?;
    if c.stored.provider == "official" {
        if v["op"] == 13 {
            if let (Some(token), Some(time)) =
                (v["d"]["plain_token"].as_str(), v["d"]["event_ts"].as_str())
            {
                if token.len() <= 2000
                    && time.len() == 10
                    && time.bytes().all(|b| b.is_ascii_digit())
                    && time
                        .parse::<i64>()
                        .is_ok_and(|at| (now / 1000 - at).abs() <= 300)
                {
                    return crate::qq_protocol::validation(&c.secret, token, time)
                        .map(|v| Json(v).into_response())
                        .ok_or_else(|| ApiError::bad("QQ 回调验证失败。"));
                }
            }
        }
        if !crate::qq_protocol::verify_official(&c.secret, &headers, &raw, now) {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_signature",
                "官方 QQ 签名无效。",
            ));
        }
        if let Some(m) = crate::qq_protocol::official(&v, &c.stored.self_id, now) {
            crate::qq_protocol::accept(&state, &c, &m).await?
        }
        return Ok(Json(json!({"op":12})).into_response());
    }
    if let Some(m) = crate::qq_protocol::onebot(&v, &c.stored.self_id, now) {
        crate::qq_protocol::accept(&state, &c, &m).await?
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
