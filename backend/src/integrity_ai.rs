//! OpenAI-compatible JSON assistant: encrypted settings, quota, cache, pinned HTTPS.
use crate::{
    ai_protocol as protocol, audit,
    auth::Actor,
    config::AppState,
    crypto,
    error::{ApiError, Result},
    integrity_enforcement::date,
};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{future::Future, net::SocketAddr, time::Duration};
pub async fn settings(state: &AppState, org: &str) -> Result<Option<Value>> {
    Ok(
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_ai_settings s WHERE org_id=$1")
            .bind(org)
            .fetch_optional(&state.db)
            .await?,
    )
}
pub fn view(c: Option<&Value>) -> Value {
    let Some(c) = c else { return Value::Null };
    let mut c = crate::ai_evidence::row(c.clone());
    let present = c["keyEnc"].as_str().is_some_and(|s| !s.is_empty());
    if let Some(m) = c.as_object_mut() {
        m.remove("keyEnc");
        m.insert("hasKey".into(), json!(present));
    }
    c
}
pub async fn save(
    state: &AppState,
    actor: &Actor,
    headers: &HeaderMap,
    org: &str,
    input: &Value,
) -> Result<Value> {
    let parsed = protocol::settings(input)?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("integrity-ai:{org}"))
        .execute(&mut *tx)
        .await?;
    let old: Option<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM integrity_ai_settings s WHERE org_id=$1 FOR UPDATE",
    )
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    if old
        .as_ref()
        .is_none_or(|s| s["base_url"] != parsed.base_url)
        && parsed.api_key.is_empty()
    {
        return Err(ApiError::bad("首次配置或更换 API 地址时必须重新填写密钥。"));
    }
    let key = if !parsed.api_key.is_empty() {
        crypto::encrypt_secret(&state.config.encryption_key, &parsed.api_key)
            .map_err(|_| ApiError::bad("密钥加密失败。"))?
    } else {
        old.as_ref()
            .and_then(|r| r["key_enc"].as_str())
            .unwrap_or("")
            .into()
    };
    let row:Value=sqlx::query_scalar("INSERT INTO integrity_ai_settings(org_id,base_url,model,key_enc,auto_enabled,auto_close_enabled,delete_low_risk,daily_limit,max_tokens,token_parameter,updated_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,date_trunc('milliseconds',clock_timestamp())) ON CONFLICT(org_id) DO UPDATE SET base_url=excluded.base_url,model=excluded.model,key_enc=excluded.key_enc,auto_enabled=excluded.auto_enabled,auto_close_enabled=excluded.auto_close_enabled,delete_low_risk=excluded.delete_low_risk,daily_limit=excluded.daily_limit,max_tokens=excluded.max_tokens,token_parameter=excluded.token_parameter,updated_at=GREATEST(excluded.updated_at,integrity_ai_settings.updated_at+interval '1 millisecond') RETURNING to_jsonb(integrity_ai_settings)").bind(org).bind(parsed.base_url).bind(parsed.model).bind(key).bind(parsed.auto_enabled).bind(parsed.auto_close_enabled).bind(parsed.delete_low_risk).bind(parsed.daily_limit).bind(parsed.max_tokens).bind(parsed.token_parameter).fetch_one(&mut *tx).await?;
    audit::org_event(
        &mut tx,
        actor,
        org,
        headers,
        "integrity.ai.configure",
        "",
        view(Some(&row)),
    )
    .await?;
    tx.commit().await?;
    Ok(view(Some(&row)))
}
fn failure() -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "ai_connection",
        "模型连接失败、超时或响应过大；请检查服务商配置。",
    )
}
pub async fn request(
    base: String,
    key: String,
    path: String,
    body: Option<Value>,
) -> Result<Value> {
    let url = url::Url::parse(&format!("{}/{path}", protocol::api_base(&base)?))
        .map_err(|_| failure())?;
    let target = tokio::time::timeout(
        Duration::from_secs(10),
        crate::rcon::resolve_target(
            url.host_str().ok_or_else(failure)?,
            url.port_or_known_default().unwrap_or(443),
            "https",
            false,
        ),
    )
    .await
    .map_err(|_| failure())?
    .map_err(|_| failure())?;
    let address = *target
        .addresses
        .as_ref()
        .and_then(|v| v.first())
        .ok_or_else(failure)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve(
            url.host_str().unwrap(),
            SocketAddr::new(address, target.port),
        )
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| failure())?;
    let mut req = client
        .request(
            if body.is_some() {
                reqwest::Method::POST
            } else {
                reqwest::Method::GET
            },
            url,
        )
        .bearer_auth(key)
        .header("accept", "application/json")
        .header("content-type", "application/json");
    if let Some(body) = body {
        req = req.json(&body)
    }
    let response = req.send().await.map_err(|_| failure())?;
    if !response.status().is_success() {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "ai_provider",
            format!(
                "模型接口返回 HTTP {}。请检查地址、权限、模型名称或额度。",
                response.status().as_u16()
            ),
        ));
    }
    if response.content_length().is_some_and(|n| n > 1048576) {
        return Err(failure());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| failure())?;
        if bytes.len() + chunk.len() > 1048576 {
            return Err(failure());
        }
        bytes.extend_from_slice(&chunk)
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "ai_json",
            "模型接口没有返回有效 JSON。",
        )
    })
}
pub struct Call {
    pub response: Value,
    pub settings_version: DateTime<Utc>,
}
pub async fn call(
    state: &AppState,
    org: &str,
    op: &str,
    bundle: Option<&Value>,
    automatic: bool,
) -> Result<Call> {
    call_with(state, org, op, bundle, automatic, request).await
}
/// Request injection is used only by contract tests. Production always pins public HTTPS.
pub async fn call_with<F, Fut>(
    state: &AppState,
    org: &str,
    op: &str,
    bundle: Option<&Value>,
    automatic: bool,
    requester: F,
) -> Result<Call>
where
    F: FnOnce(String, String, String, Option<Value>) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    if !["models", "test", "review"].contains(&op) || op == "review" && bundle.is_none() {
        return Err(ApiError::bad("未知 AI 操作或缺少案件。"));
    }
    let config = settings(state, org)
        .await?
        .ok_or_else(|| ApiError::bad("请先保存 API 地址和密钥。"))?;
    let version = date(&config["updated_at"]).ok_or_else(|| ApiError::bad("AI 配置时间无效。"))?;
    if automatic && config["auto_enabled"] != true {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ai_paused",
            "自动初审已暂停。",
        ));
    }
    let base = config["base_url"].as_str().unwrap_or("");
    let model = config["model"].as_str().unwrap_or("");
    if op != "models" && model.is_empty() {
        return Err(ApiError::bad("请选择模型并保存。"));
    }
    let input = bundle.map(Value::to_string);
    if input.as_ref().is_some_and(|s| s.len() > 256000) {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "ai_evidence_limit",
            "案件 JSON 超过 256 KB，本次未发送。请先导出核对；不会静默删减日志。",
        ));
    }
    let fingerprint = crypto::hash_token(
        &json!([
            protocol::SYSTEM,
            base,
            model,
            config["max_tokens"],
            config["token_parameter"],
            input
        ])
        .to_string(),
    );
    let case = bundle.and_then(|v| v["case"]["id"].as_str());
    if op == "review" {
        let cached:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('result',r.result,'cached',true,'createdAt',r.created_at) FROM integrity_ai_reviews r JOIN integrity_cases c ON c.id=r.case_id WHERE fingerprint=$1 AND case_id=$2 AND c.org_id=$3 AND c.status<>'AI_CLEARED'").bind(&fingerprint).bind(case).bind(org).fetch_optional(&state.db).await?;
        if let Some(c) = cached {
            if protocol::review(&c["result"]).is_ok() {
                return Ok(Call {
                    response: crate::ai_evidence::row(c),
                    settings_version: version,
                });
            }
        }
    }
    let mut tx = if automatic {
        state.worker_transaction().await?
    } else {
        state.db.begin().await?
    };
    let claimed:Option<String>=sqlx::query_scalar("UPDATE integrity_ai_settings SET last_request_at=clock_timestamp(),budget_day=CASE WHEN $3 THEN to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD') ELSE budget_day END,daily_requests=CASE WHEN $3 THEN CASE WHEN budget_day=to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD') THEN daily_requests+1 ELSE 1 END ELSE daily_requests END WHERE org_id=$1 AND updated_at=$2 AND (NOT $3 OR(auto_enabled AND (budget_day<>to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD') OR daily_requests<daily_limit))) AND(last_request_at IS NULL OR last_request_at<now()-interval '65 seconds') AND EXISTS(SELECT 1 FROM organizations o WHERE o.id=$1 AND o.suspended_at IS NULL) RETURNING org_id").bind(org).bind(version).bind(automatic).fetch_optional(&mut *tx).await?;
    if claimed.is_none() {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "ai_quota",
            "同一组织每65秒最多请求一次；自动初审还受每日额度限制，请稍后再试。",
        ));
    }
    tx.commit().await?;
    let key = crypto::decrypt_secret(
        &state.config.encryption_key,
        config["key_enc"].as_str().unwrap_or(""),
    )
    .map_err(|_| ApiError::bad("AI 密钥无法解密，请重新保存。"))?;
    let body = if op == "models" {
        None
    } else {
        let mut body = json!({"model":model,"stream":false,"messages":[{"role":"system","content":if op=="test"{"请简短回复连接成功。"}else{protocol::SYSTEM}},{"role":"user","content":if op=="test"{"连接测试，不含玩家数据。"}else{input.as_deref().unwrap_or("")}}]});
        body[config["token_parameter"]
            .as_str()
            .filter(|s| ["max_tokens", "max_completion_tokens"].contains(s))
            .unwrap_or("max_tokens")] = config["max_tokens"].clone();
        Some(body)
    };
    let response = requester(
        base.into(),
        key,
        if op == "models" {
            "models"
        } else {
            "chat/completions"
        }
        .into(),
        body,
    )
    .await?;
    if op == "models" {
        return Ok(Call {
            response: json!({"models":response["data"].as_array().map(|a|a.iter().filter_map(|v|v["id"].as_str()).filter(|s|protocol::length(s)<=200).take(500).collect::<Vec<_>>()).unwrap_or_default()}),
            settings_version: version,
        });
    }
    let choice = &response["choices"][0];
    let content = choice["message"]["content"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_GATEWAY, "ai_text", "模型未返回完整文本。"))?;
    if choice["finish_reason"] == "length" {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "ai_truncated",
            "模型未返回完整文本，请增加输出上限。",
        ));
    }
    if op == "test" {
        return Ok(Call {
            response: json!({"message":"连接成功，所选模型已返回文本。"}),
            settings_version: version,
        });
    }
    let content = if let Some(s) = content.strip_prefix("```") {
        s.strip_prefix("json").unwrap_or(s).trim_start()
    } else {
        content
    };
    let content = content
        .trim_end()
        .strip_suffix("```")
        .unwrap_or(content)
        .trim_end();
    let parsed: Value = serde_json::from_str(content).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "ai_output_json",
            "模型没有返回约定的审核 JSON，请更换模型后重试。",
        )
    })?;
    let mut result = serde_json::to_value(protocol::review(&parsed)?).unwrap();
    result["promptVersion"] = json!(protocol::PROMPT_VERSION);
    result["model"] = json!(model);
    result["usage"] = response["usage"].clone();
    result["advisoryOnly"] = json!(true);
    let mut tx = if automatic {
        state.worker_transaction().await?
    } else {
        state.db.begin().await?
    };
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM integrity_cases WHERE id=$1 AND org_id=$2 FOR SHARE",
    )
    .bind(case)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    if status.is_none_or(|s| s == "AI_CLEARED") {
        return Err(ApiError::new(
            StatusCode::GONE,
            "ai_cleared",
            "案件已清理，未重新保存详细模型结果。",
        ));
    }
    sqlx::query("INSERT INTO integrity_ai_reviews(fingerprint,case_id,result) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(&fingerprint).bind(case).bind(&result).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Call {
        response: json!({"result":result,"cached":false,"createdAt":Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}),
        settings_version: version,
    })
}
