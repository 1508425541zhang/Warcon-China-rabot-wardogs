//! Versioned organization rules. Read paths never materialize default database rows.
use crate::{
    error::{ApiError, Result},
    integrity_score,
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};

pub const MODES: &[&str] = &[
    "disabled",
    "legacy",
    "statistical_shadow",
    "statistical",
    "model_only",
    "long_only",
    "short_only",
];
pub fn committee_enabled(mode: &str) -> bool {
    matches!(mode, "statistical" | "statistical_shadow")
}
pub fn long_enabled(mode: &str) -> bool {
    matches!(mode, "model_only" | "long_only")
}
pub fn short_enabled(mode: &str) -> bool {
    matches!(mode, "model_only" | "short_only")
}
pub fn enforcement_defaults() -> Value {
    json!({"autoKickEnabled":false,"autoQuarantine24hEnabled":false,"autoQuarantine7dEnabled":false,"autoActionMaxPerHour":10,"autoActionMaxPercentOnline":10,"autoSuspendedAt":null})
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rules {
    pub version: i32,
    pub config: Value,
    pub enforcement: Value,
    pub assessment_mode: String,
}
pub fn from_row(row: Option<&Value>) -> Result<Rules> {
    let config = if let Some(row) = row {
        integrity_score::validate(&row["config"], &integrity_score::defaults())?
    } else {
        integrity_score::defaults()
    };
    let mut enforcement = enforcement_defaults();
    if let Some(row) = row {
        for (key, col) in [
            ("autoKickEnabled", "auto_kick_enabled"),
            ("autoQuarantine24hEnabled", "auto_quarantine_24h_enabled"),
            ("autoQuarantine7dEnabled", "auto_quarantine_7d_enabled"),
            ("autoActionMaxPerHour", "auto_action_max_per_hour"),
            (
                "autoActionMaxPercentOnline",
                "auto_action_max_percent_online",
            ),
            ("autoSuspendedAt", "auto_suspended_at"),
        ] {
            enforcement[key] = row[col].clone();
        }
        if let Some(t) = row["auto_suspended_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        {
            enforcement["autoSuspendedAt"] =
                json!(t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
    }
    Ok(Rules {
        version: row.and_then(|r| r["version"].as_i64()).unwrap_or(1) as i32,
        config,
        enforcement,
        assessment_mode: row
            .and_then(|r| r["assessment_mode"].as_str())
            .unwrap_or("statistical_shadow")
            .into(),
    })
}
pub async fn load(pool: &PgPool, org: &str) -> Result<Rules> {
    let row: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_rules r WHERE org_id=$1")
            .bind(org)
            .fetch_optional(pool)
            .await?;
    from_row(row.as_ref())
}
pub async fn lock(tx: &mut Transaction<'_, Postgres>, org: &str) -> Result<Option<Value>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("integrity-rules:{org}"))
        .execute(&mut **tx)
        .await?;
    Ok(
        sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_rules r WHERE org_id=$1 FOR UPDATE")
            .bind(org)
            .fetch_optional(&mut **tx)
            .await?,
    )
}
pub async fn changed(tx: &mut Transaction<'_, Postgres>, org: &str) -> Result<()> {
    sqlx::query("SELECT pg_notify('warcon_integrity_changed',$1)")
        .bind(org)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub fn validate_enforcement(before: &Value, values: &Value) -> Result<Value> {
    let values = values
        .as_object()
        .ok_or_else(|| ApiError::bad("Enforcement settings must be an object."))?;
    let allowed = [
        "autoKickEnabled",
        "autoQuarantine24hEnabled",
        "autoQuarantine7dEnabled",
        "autoActionMaxPerHour",
        "autoActionMaxPercentOnline",
        "resume",
        "confirmation",
    ];
    for key in values.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ApiError::bad(format!(
                "Unknown enforcement setting '{key}'."
            )));
        }
    }
    if !values.keys().any(|key| key != "confirmation") {
        return Err(ApiError::bad("No enforcement setting changes supplied."));
    }
    let mut next = before.clone();
    let mut enabling = false;
    for key in &allowed[..3] {
        if let Some(value) = values.get(*key) {
            let flag = value
                .as_bool()
                .ok_or_else(|| ApiError::bad(format!("{key} must be boolean.")))?;
            enabling |= flag && !before[*key].as_bool().unwrap_or(false);
            next[*key] = json!(flag);
        }
    }
    for key in &allowed[3..5] {
        if let Some(value) = values.get(*key) {
            let n = value
                .as_f64()
                .filter(|n| n.fract() == 0. && (1. ..=100.).contains(n))
                .ok_or_else(|| ApiError::bad(format!("{key} must be between 1 and 100.")))?;
            next[*key] = json!(n as i64);
        }
    }
    let resume = values.get("resume") == Some(&Value::Bool(true));
    if (enabling || resume)
        && values.get("confirmation").and_then(Value::as_str)
            != Some("ENABLE_EXPERIMENTAL_INTEGRITY")
    {
        return Err(ApiError::bad(
            "Explicit experimental enforcement confirmation is required.",
        ));
    }
    if resume {
        next["autoSuspendedAt"] = Value::Null;
    }
    Ok(next)
}
