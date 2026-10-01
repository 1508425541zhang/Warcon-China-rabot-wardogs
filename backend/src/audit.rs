use crate::{auth::Actor, error::Result};
use axum::http::HeaderMap;
use serde_json::Value;
use sqlx::{Postgres, Transaction};

pub fn redact(value: &Value, depth: usize) -> Value {
    if depth > 6 {
        return Value::String("[deep]".into());
    }
    match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(50)
                .map(|v| redact(v, depth + 1))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(k, v)| {
                    let lower = k.to_lowercase();
                    let hidden = ["pass", "secret", "token", "key", "authorization", "cookie"]
                        .iter()
                        .any(|s| lower.contains(s));
                    (
                        k.clone(),
                        if hidden && crate::http::truthy(v) {
                            Value::String("[redacted]".into())
                        } else {
                            redact(v, depth + 1)
                        },
                    )
                })
                .collect(),
        ),
        Value::String(text) => {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            let safe = text
                .split('\n')
                .map(|line| {
                    let Some((key, _)) = line.split_once('=') else {
                        return line.to_owned();
                    };
                    let key = key.trim();
                    if key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                        && ["pass", "secret", "token", "key"]
                            .iter()
                            .any(|s| key.to_lowercase().contains(s))
                    {
                        format!("{}[redacted]", &line[..=line.find('=').unwrap()])
                    } else {
                        line.into()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            Value::String(crate::feed::truncate(&safe, 4000))
        }
        _ => value.clone(),
    }
}

pub async fn event(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    org_id: Option<&str>,
    headers: &HeaderMap,
    category: &str,
    action: &str,
    target: &str,
    detail: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,org_id,category,action,target,detail,outcome,user_agent) VALUES($1,$2,$3,$4,$5,$6,$7,'ok',$8)")
        .bind(&actor.id).bind(&actor.name).bind(org_id).bind(category).bind(action).bind(target).bind(redact(&detail,0))
        .bind(crate::feed::truncate(headers.get("user-agent").and_then(|v|v.to_str().ok()).unwrap_or("").into(), 300)).execute(&mut **tx).await?;
    Ok(())
}

pub async fn org_event(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    org_id: &str,
    headers: &HeaderMap,
    action: &str,
    target: &str,
    detail: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,org_id,category,action,target,detail,outcome,user_agent) VALUES($1,$2,$3,'org',$4,$5,$6,'ok',$7)")
        .bind(&actor.id).bind(&actor.name).bind(org_id).bind(action).bind(target).bind(redact(&detail,0))
        .bind(headers.get("user-agent").and_then(|v|v.to_str().ok()).unwrap_or("")).execute(&mut **tx).await?;
    Ok(())
}
