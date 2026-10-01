//! OpenAPI/OneBot HTTP transport with bounded responses and no redirect or send retry.
use crate::{
    error::{ApiError, Result},
    qq_config::Configuration,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::OnceLock};
use tokio::sync::Mutex;
static TOKENS: OnceLock<Mutex<HashMap<String, (String, i64)>>> = OnceLock::new();
async fn http(
    url: &str,
    authorization: Option<&str>,
    app: Option<&str>,
    body: Option<&Value>,
) -> Result<Value> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| ApiError::bad("QQ 网络连接不可用。"))?;
    let mut request = if body.is_some() {
        client.post(url)
    } else {
        client.get(url)
    };
    if let Some(a) = authorization {
        request = request.header("authorization", a)
    }
    if let Some(a) = app {
        request = request.header("X-Union-Appid", a)
    }
    if let Some(b) = body {
        request = request.json(b)
    }
    let mut r = request.send().await.map_err(|_| {
        ApiError::new(
            axum::http::StatusCode::BAD_GATEWAY,
            "qq_unavailable",
            "QQ 接口超时或无法连接。",
        )
    })?;
    let status = r.status();
    let mut bytes = Vec::new();
    while let Some(c) = r
        .chunk()
        .await
        .map_err(|_| ApiError::bad("QQ 响应未完成。"))?
    {
        if bytes.len() + c.len() > 65536 {
            return Err(ApiError::bad("QQ 响应过大。"));
        }
        bytes.extend_from_slice(&c)
    }
    let v: Value =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::bad("QQ 返回了无效数据。"))?;
    if !status.is_success() {
        return Err(ApiError::new(
            axum::http::StatusCode::BAD_GATEWAY,
            if status.as_u16() == 401 {
                "qq_token_expired"
            } else {
                "qq_rejected"
            },
            format!("QQ 接口返回 HTTP {}。", status.as_u16()),
        ));
    }
    Ok(v)
}
pub async fn token(c: &Configuration) -> Result<String> {
    let mut tokens = TOKENS.get_or_init(Default::default).lock().await;
    let now = chrono::Utc::now().timestamp();
    tokens.retain(|_, (_, until)| *until > now);
    let key = crate::crypto::hash_token(&format!("{}:{}", c.stored.self_id, c.secret));
    if let Some((token, _)) = tokens.get(&key) {
        return Ok(token.clone());
    }
    let value = http(
        "https://api.bot.qq.com/app/getAppAccessToken",
        None,
        None,
        Some(&json!({"appId":c.stored.self_id,"clientSecret":c.secret})),
    )
    .await?;
    let expires = crate::live::finite(&value["expires_in"]).filter(|n| *n > 0.);
    if value["code"].as_i64().is_some_and(|n| n != 0)
        || !value["access_token"].is_string()
        || expires.is_none()
    {
        return Err(ApiError::bad("官方 QQ 鉴权失败。"));
    }
    let value = value["access_token"].as_str().unwrap().to_owned();
    tokens.insert(
        key,
        (value.clone(), now + (expires.unwrap() as i64 - 60).max(1)),
    );
    Ok(value)
}
pub async fn official(c: &Configuration, path: &str, body: Option<&Value>) -> Result<Value> {
    let t = token(c).await?;
    let response = http(
        &format!("https://api.bot.qq.com{path}"),
        Some(&format!("QQBot {t}")),
        Some(&c.stored.self_id),
        body,
    )
    .await;
    if response
        .as_ref()
        .is_err_and(|e| e.code == "qq_token_expired")
    {
        TOKENS
            .get_or_init(Default::default)
            .lock()
            .await
            .remove(&crate::crypto::hash_token(&format!(
                "{}:{}",
                c.stored.self_id, c.secret
            )));
    }
    let v = response?;
    if v["code"].as_i64().is_some_and(|n| n != 0) {
        return Err(ApiError::bad("官方 QQ 拒绝了请求。"));
    }
    Ok(v)
}
pub async fn logged_in(c: &Configuration) -> bool {
    if c.stored.provider == "official" {
        return token(c).await.is_ok();
    }
    http(
        &format!("{}/get_login_info", c.stored.url),
        Some(&format!("Bearer {}", c.token)),
        None,
        Some(&json!({})),
    )
    .await
    .is_ok_and(|v| {
        v["status"] == "ok"
            && v["retcode"] == 0
            && (v["data"]["user_id"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| v["data"]["user_id"].as_u64().map(|n| n.to_string()))
                .as_deref()
                == Some(c.stored.self_id.as_str()))
    })
}
pub async fn reply(c: &Configuration, group: &str, id: &str, content: &str) -> Result<()> {
    let content = crate::feed::truncate(content, 3500);
    if c.stored.provider == "official" {
        use base64::Engine;
        if !crate::qq_config::open_id(group) {
            return Err(ApiError::bad("官方 QQ 群标识无效。"));
        }
        let mut body = json!({"content":content,"msg_type":0});
        let prefix = format!("official:{}:{group}:", c.stored.self_id);
        if let Some(encoded) = id.strip_prefix(&prefix) {
            if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded) {
                if let Ok(message) = String::from_utf8(bytes) {
                    body["msg_id"] = json!(message);
                    body["msg_seq"] = json!(1)
                }
            }
        }
        let r = official(c, &format!("/v2/groups/{group}/messages"), Some(&body)).await?;
        if r["id"].as_str().is_none_or(str::is_empty) {
            return Err(ApiError::bad("官方 QQ 未确认发送结果。"));
        }
    } else {
        if !crate::qq_config::number_id(group, 5, 16) {
            return Err(ApiError::bad("QQ 群号无效。"));
        }
        let v = http(
            &format!("{}/send_group_msg", c.stored.url),
            Some(&format!("Bearer {}", c.token)),
            None,
            Some(&json!({"group_id":group,"message":[{"type":"text","data":{"text":content}}]})),
        )
        .await?;
        if v["status"] != "ok" || v["retcode"] != 0 || v["data"]["message_id"].is_null() {
            return Err(ApiError::bad("QQ 未确认发送结果。"));
        }
    }
    Ok(())
}
