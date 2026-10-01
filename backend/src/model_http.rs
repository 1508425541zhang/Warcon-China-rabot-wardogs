//! Pinned long-window model contract; secrets never appear in the settings view.
use crate::{
    config::AppState,
    crypto,
    error::{ApiError, Result},
};
use axum::http::StatusCode;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
pub fn manifest() -> Value {
    serde_json::from_str(include_str!(
        "../../services/integrity-model-bun/artifacts-expanded30m/manifest.json"
    ))
    .unwrap()
}
pub fn calibration() -> Value {
    serde_json::from_str(include_str!(
        "../../services/integrity-model-bun/artifacts-expanded30m/calibration.json"
    ))
    .unwrap()
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelConfig {
    pub revision: String,
    pub developer_enabled: bool,
    pub url: String,
    pub token_enc: String,
    pub auto_punish_enabled: bool,
    pub max_actions_per_hour: i32,
    pub cooldown_seconds: i32,
    pub interval_seconds: i32,
}
impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            revision: "empty".into(),
            developer_enabled: false,
            url: String::new(),
            token_enc: String::new(),
            auto_punish_enabled: true,
            max_actions_per_hour: 10,
            cooldown_seconds: 600,
            interval_seconds: 1800,
        }
    }
}
pub async fn config(db: &PgPool, org: &str) -> Result<ModelConfig> {
    let row: Option<Value> = sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1")
        .bind(format!("integrityModel:{org}"))
        .fetch_optional(db)
        .await?;
    parse_config(row)
}
pub fn parse_config(row: Option<Value>) -> Result<ModelConfig> {
    if let Some(mut row) = row {
        if !row.is_object() {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "model_configuration",
                "模型配置无效，请重新保存。",
            ));
        }
        let interval = row
            .get("intervalSeconds")
            .and_then(crate::http::js_number)
            .filter(|n| *n != 0.)
            .unwrap_or(1800.)
            .max(1800.);
        row["intervalSeconds"] = json!(interval.min(i32::MAX as f64) as i32);
        serde_json::from_value(row).map_err(|_| {
            ApiError::new(
                StatusCode::CONFLICT,
                "model_configuration",
                "模型配置无效，请重新保存。",
            )
        })
    } else {
        Ok(ModelConfig::default())
    }
}
pub fn view(c: &ModelConfig) -> Value {
    let mut result = serde_json::to_value(c).unwrap();
    result.as_object_mut().unwrap().remove("tokenEnc");
    let m = manifest();
    let cal = calibration();
    result.as_object_mut().unwrap().extend(json!({"hasToken":!c.token_enc.is_empty(),"modelId":m["model_id"],"stage":"A测","bucketSeconds":30,"windowSeconds":1800,"windowSteps":60,"referenceSamples":cal["sample_count"],"p95":cal["p95"],"p97":cal["p97"],"p98":cal["p98"],"p99":cal["p99"]}).as_object().unwrap().clone());
    result
}
pub fn model_url(value: &str) -> Result<String> {
    let url = url::Url::parse(value.trim()).map_err(|_| ApiError::bad("模型 API 地址无效。"))?;
    if !["http", "https"].contains(&url.scheme())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !["", "/"].contains(&url.path())
        || url.host_str().is_none()
    {
        return Err(ApiError::bad(
            "填写 HTTP(S) 服务根地址，不包含路径、账号或查询参数。",
        ));
    }
    Ok(url.origin().ascii_serialization())
}
pub fn parse_input(input: &Value, old: &ModelConfig, state: &AppState) -> Result<ModelConfig> {
    let bad = || ApiError::bad("模型配置无效。");
    let string = |key: &str, max: usize| {
        input[key]
            .as_str()
            .filter(|s| s.encode_utf16().count() <= max)
            .ok_or_else(bad)
    };
    let revision = string("revision", 100)?;
    let developer = input["developerEnabled"].as_bool().ok_or_else(bad)?;
    let raw_url = string("url", 2000)?;
    let url = if raw_url.is_empty() {
        String::new()
    } else {
        model_url(raw_url)?
    };
    let token = if let Some(v) = input.get("token") {
        Some(
            v.as_str()
                .filter(|s| s.encode_utf16().count() <= 512)
                .ok_or_else(bad)?,
        )
    } else {
        None
    };
    if token.is_some_and(|s| {
        !s.is_empty() && (s.encode_utf16().count() < 32 || s.chars().any(char::is_whitespace))
    }) {
        return Err(ApiError::bad("令牌至少 32 个字符且不包含空白。"));
    }
    let auto = if let Some(v) = input.get("autoPunishEnabled") {
        v.as_bool().ok_or_else(bad)?
    } else {
        true
    };
    let integer = |key: &str, default: Option<i32>, min: i32, max: i32| -> Result<i32> {
        if let Some(v) = input.get(key) {
            v.as_f64()
                .filter(|n| n.fract() == 0. && *n >= min as f64 && *n <= max as f64)
                .map(|n| n as i32)
                .ok_or_else(bad)
        } else {
            default.ok_or_else(bad)
        }
    };
    let max = integer("maxActionsPerHour", Some(10), 1, 100)?;
    let cooldown = integer("cooldownSeconds", Some(600), 60, 86400)?;
    let interval = integer("intervalSeconds", None, 1800, 86400)?;
    if old.revision != revision {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "revision_conflict",
            "配置已变更，请重新加载。",
        ));
    }
    let token_enc = if let Some(token) = token.filter(|s| !s.is_empty()) {
        crypto::encrypt_secret(&state.config.encryption_key, token).map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "encryption",
                "无法保存模型令牌。",
            )
        })?
    } else {
        old.token_enc.clone()
    };
    if developer && (url.is_empty() || token_enc.is_empty()) {
        return Err(ApiError::bad("开发者模式需要 API 地址和令牌。"));
    }
    Ok(ModelConfig {
        revision: uuid::Uuid::new_v4().to_string(),
        developer_enabled: developer,
        url,
        token_enc,
        auto_punish_enabled: auto,
        max_actions_per_hour: max,
        cooldown_seconds: cooldown,
        interval_seconds: interval,
    })
}
pub async fn call(state: &AppState, c: &ModelConfig, body: Option<&Value>) -> Result<Value> {
    if !c.developer_enabled || c.token_enc.is_empty() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "model_disabled",
            "模型开发者模式未启用。",
        ));
    }
    let data = body.map(Value::to_string);
    if data.as_ref().is_some_and(|s| s.len() > 8 * 1024 * 1024) {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "model_input_size",
            "模型输入超限，未截断数据。",
        ));
    }
    let origin = model_url(&c.url)?;
    let failure = || {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "model_request",
            "模型 API 请求失败、超时或返回无效数据。",
        )
    };
    let token = crypto::decrypt_secret(&state.config.encryption_key, &c.token_enc)
        .map_err(|_| failure())?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| failure())?;
    let mut req = if data.is_some() {
        client.post(format!("{origin}/v1/assess"))
    } else {
        client.get(format!("{origin}/v1/health"))
    };
    req = req
        .bearer_auth(token)
        .header("content-type", "application/json");
    if let Some(data) = data {
        req = req.body(data);
    }
    let response = req.send().await.map_err(|_| failure())?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > 65536) {
        return Err(failure());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| failure())?;
        if bytes.len() + chunk.len() > 65536 {
            return Err(failure());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| failure())
}
pub fn verify_health(result: &Value) -> Result<()> {
    let m = manifest();
    if [
        "model_id",
        "checkpoint_sha256",
        "feature_schema",
        "calibration_sha256",
    ]
    .iter()
    .any(|key| result[*key] != m[*key])
    {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "model_fingerprint",
            "模型版本、权重或输入格式不匹配。",
        ));
    }
    Ok(())
}
pub fn verify_result(result: &Value, id: &str) -> Result<Option<f64>> {
    let m = manifest();
    if result["modelId"] != m["model_id"]
        || result["checkpointSha256"] != m["checkpoint_sha256"]
        || result["schema"] != m["feature_schema"]
        || result["requestId"] != id
        || result["calibrationSha256"] != m["calibration_sha256"]
        || result["windowSeconds"] != 1800
        || result["bucketSeconds"] != 30
    {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "model_identity",
            "模型响应身份不匹配。",
        ));
    }
    if result["status"] == "INSUFFICIENT_DATA" {
        return Ok(None);
    }
    let valid = result["status"] == "READY"
        && result["score"]
            .as_f64()
            .is_some_and(|n| n.is_finite() && n >= 0.)
        && result["pointScores"].as_array().is_some_and(|a| {
            a.len() == 60
                && a.iter()
                    .all(|v| v.as_f64().is_some_and(|n| n.is_finite() && n >= 0.))
        });
    if !valid {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "model_score",
            "模型评分格式无效。",
        ));
    }
    Ok(result["score"].as_f64())
}
pub async fn runs(db: &PgPool, org: &str, server: Option<&str>) -> Result<Vec<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(r) FROM (SELECT id,server_id,steam_id,match_id,state,score,threshold,created_at,finished_at,action,action_state,action_reason,expires_at,result->>'reason' AS data_reason,result->>'windowSeconds' AS window_seconds,result->>'observedBuckets' AS observed_buckets,result->>'requiredBuckets' AS required_buckets,CASE WHEN state='READY' THEN score>=threshold ELSE NULL END AS anomalous FROM integrity_model_runs WHERE org_id=$1 AND ($2::text IS NULL OR server_id=$2) ORDER BY created_at DESC LIMIT 50) r").bind(org).bind(server).fetch_all(db).await?;
    Ok(rows
        .into_iter()
        .map(|mut r| {
            for key in ["created_at", "finished_at", "expires_at"] {
                if let Some(t) = r[key]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                {
                    r[key] = json!(t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                }
            }
            r
        })
        .collect())
}
