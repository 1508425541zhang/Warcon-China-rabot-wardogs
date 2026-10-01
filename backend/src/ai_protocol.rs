//! Original JSON review contract. Recommendations never become human labels or penalties.
use crate::error::{ApiError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub const PROMPT_VERSION: &str = "integrity-triage-v4.1-auto-close";
pub const SYSTEM: &str = include_str!("../prompts/integrity-ai.txt");
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInput {
    #[serde(default = "yes")]
    pub auto_enabled: bool,
    #[serde(default = "yes")]
    pub auto_close_enabled: bool,
    #[serde(default = "yes")]
    pub delete_low_risk: bool,
    #[serde(default = "daily")]
    pub daily_limit: i32,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "tokens")]
    pub max_tokens: i32,
    #[serde(default = "parameter")]
    pub token_parameter: String,
}
fn yes() -> bool {
    true
}
fn daily() -> i32 {
    100
}
fn tokens() -> i32 {
    1200
}
fn parameter() -> String {
    "max_tokens".into()
}
pub fn length(s: &str) -> usize {
    s.encode_utf16().count()
}
pub fn api_base(s: &str) -> Result<String> {
    let url = url::Url::parse(s).map_err(|_| ApiError::bad("API 地址格式不正确。"))?;
    let path = url.path().trim_end_matches('/');
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || path.ends_with("/chat/completions")
        || path.ends_with("/models")
    {
        return Err(ApiError::bad(
            "请填写不含账号、查询参数的 HTTPS API 基础地址，不要包含 /chat/completions 或 /models。",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').into())
}
pub fn settings(input: &Value) -> Result<SettingsInput> {
    let mut c: SettingsInput =
        serde_json::from_value(input.clone()).map_err(|_| ApiError::bad("AI 配置格式不正确。"))?;
    c.base_url = c.base_url.trim().into();
    c.model = c.model.trim().into();
    c.api_key = c.api_key.trim().into();
    if !(1..=1000).contains(&c.daily_limit)
        || c.base_url.is_empty()
        || length(&c.base_url) > 500
        || length(&c.model) > 200
        || length(&c.api_key) > 4096
        || c.api_key.contains(['\r', '\n'])
        || !(256..=4096).contains(&c.max_tokens)
        || !["max_tokens", "max_completion_tokens"].contains(&c.token_parameter.as_str())
    {
        return Err(ApiError::bad(
            "配置格式不正确：请检查地址、模型、每日额度和输出上限（256–4096）。",
        ));
    }
    c.base_url = api_base(&c.base_url)?;
    Ok(c)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reason {
    pub text: String,
    pub evidence: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub verdict: String,
    pub suspicion_percent: Option<i32>,
    pub evidence_quality: String,
    pub alternatives: Vec<String>,
    pub summary: String,
    pub reasons: Vec<Reason>,
    pub contradictions: Vec<String>,
    pub missing_evidence: Vec<String>,
}
pub fn review(value: &Value) -> Result<Review> {
    // Option alone would accept a missing field, unlike the existing mandatory nullable field.
    if value.get("suspicionPercent").is_none() {
        return Err(invalid());
    }
    let r: Review = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    let strings = |a: &[String], count: usize, max: usize| {
        a.len() <= count && a.iter().all(|s| length(s) <= max)
    };
    if !["建议通过", "建议复核", "证据不足"].contains(&r.verdict.as_str())
        || !["低", "中", "高"].contains(&r.evidence_quality.as_str())
        || r.suspicion_percent
            .is_some_and(|n| !(0..=100).contains(&n) || n % 5 != 0)
        || (r.verdict == "证据不足" && r.suspicion_percent.is_some())
        || (r.evidence_quality == "低" && r.suspicion_percent.is_some_and(|n| n > 60))
        || r.summary.is_empty()
        || length(&r.summary) > 2000
        || !strings(&r.alternatives, 5, 800)
        || !strings(&r.contradictions, 20, 1000)
        || !strings(&r.missing_evidence, 20, 1000)
        || r.reasons.len() > 20
        || r.reasons
            .iter()
            .any(|r| length(&r.text) > 1000 || length(&r.evidence) > 300)
    {
        return Err(invalid());
    }
    Ok(r)
}
fn invalid() -> ApiError {
    ApiError::new(
        axum::http::StatusCode::BAD_GATEWAY,
        "ai_output",
        "模型审核结果字段不完整或数值无效，未保存为有效结论。",
    )
}
pub fn triage(r: &Review, delete_low: bool) -> &'static str {
    if r.suspicion_percent.is_some_and(|n| n >= 65) {
        "ADMIN_REVIEW"
    } else if r.suspicion_percent.is_none()
        || r.verdict == "证据不足"
        || r.evidence_quality == "低"
        || !r.contradictions.is_empty()
        || r.reasons.is_empty()
        || r.reasons.iter().any(|r| r.evidence.trim().is_empty())
    {
        "AI_ARCHIVED_UNRESOLVED"
    } else if delete_low
        && r.suspicion_percent.is_some_and(|n| n <= 25)
        && r.verdict == "建议通过"
        && r.missing_evidence.is_empty()
    {
        "AI_CLEARED"
    } else {
        "AI_ARCHIVED"
    }
}
pub fn numeric_checks(snapshot: &Value) -> Value {
    let kills = snapshot["infantryKills"].as_f64().filter(|n| n.is_finite());
    let kpm = snapshot["kpm180"].as_f64().filter(|n| n.is_finite());
    json!({"scope":"仅复核冻结窗口的算术，不从触发事件推断完整步兵击杀；不改变评分","kpm180":{"infantryKills":kills,"recorded":kpm,"recomputed":kills.map(|n|n/3.),"matches":kills.zip(kpm).map(|(n,k)|(k-n/3.).abs()<0.011)}})
}
