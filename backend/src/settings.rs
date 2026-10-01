use crate::{
    error::{ApiError, Result},
    http::js_number,
};
use serde_json::{Map, Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::OnceLock;
pub fn specs() -> &'static Vec<Value> {
    static SPECS: OnceLock<Vec<Value>> = OnceLock::new();
    SPECS.get_or_init(|| {
        serde_json::from_str(include_str!("../fixtures/settings.json"))
            .expect("Generated settings contract must be valid")
    })
}
fn clamp(spec: &Value, n: f64) -> f64 {
    (n + 0.5)
        .floor()
        .clamp(spec["min"].as_f64().unwrap(), spec["max"].as_f64().unwrap())
}
pub fn defaults() -> Map<String, Value> {
    let mut out = Map::new();
    for spec in specs() {
        out.insert(
            spec["key"].as_str().unwrap().into(),
            spec["default"].clone(),
        );
    }
    for (env, key, multiplier, integer) in [
        ("POLL_SECONDS", "sampleMs", 1000., false),
        ("POLL_CONCURRENCY", "concurrency", 1., true),
    ] {
        if let Ok(raw) = std::env::var(env) {
            if let Some(n) =
                js_number(&json!(raw)).filter(|n| *n > 0. && (!integer || n.fract() == 0.))
            {
                let spec = specs().iter().find(|v| v["key"] == key).unwrap();
                out.insert(key.into(), json!(clamp(spec, n * multiplier)));
            }
        }
    }
    out
}
pub fn effective(rows: &[(String, Value)]) -> Map<String, Value> {
    let mut out = defaults();
    for (key, raw) in rows {
        if let Some(spec) = specs().iter().find(|s| s["key"].as_str() == Some(key)) {
            let n = if raw.is_object() {
                raw.get("n").and_then(js_number)
            } else {
                js_number(raw)
            };
            if let Some(n) = n {
                out.insert(key.clone(), json!(clamp(spec, n)));
            }
        }
    }
    out
}
pub async fn load(db: &PgPool) -> Result<Map<String, Value>> {
    let names = specs()
        .iter()
        .map(|s| s["key"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let rows: Vec<(String, Value)> =
        sqlx::query_as("SELECT key,value FROM site_settings WHERE key=ANY($1)")
            .bind(names)
            .fetch_all(db)
            .await?;
    Ok(effective(&rows))
}
pub async fn view(db: &PgPool) -> Result<Vec<Value>> {
    let rows: Vec<(String, Value)> = sqlx::query_as("SELECT key,value FROM site_settings")
        .fetch_all(db)
        .await?;
    let values = effective(&rows);
    Ok(specs()
        .iter()
        .map(|s| {
            let mut s = s.clone();
            let key = s["key"].as_str().unwrap().to_owned();
            s["value"] = values[&key].clone();
            s["stored"] = json!(rows.iter().any(|(k, _)| k == &key));
            s
        })
        .collect())
}
pub async fn update(
    tx: &mut Transaction<'_, Postgres>,
    actor: &str,
    body: &Value,
) -> Result<(Value, Vec<String>)> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('warcon:runtime-settings',0))")
        .execute(&mut **tx)
        .await?;
    let reset = body["reset"]
        .as_array()
        .map(|v| {
            v.iter()
                .map(|v| crate::http::string(v, usize::MAX))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let empty = Map::new();
    let values = if body["values"].is_null() {
        &empty
    } else {
        body["values"]
            .as_object()
            .ok_or_else(|| ApiError::bad("values must be an object."))?
    };
    let spec_for = |key: &str| {
        specs()
            .iter()
            .find(|s| s["key"].as_str() == Some(key))
            .ok_or_else(|| {
                ApiError::new(
                    axum::http::StatusCode::BAD_REQUEST,
                    "bad_setting",
                    format!("Unknown setting '{key}'."),
                )
            })
    };
    for key in &reset {
        spec_for(key)?;
    }
    let mut validated = Map::new();
    for (key, raw) in values {
        let spec = spec_for(key)?;
        let n = js_number(raw)
            .filter(|v| *v >= spec["min"].as_f64().unwrap() && *v <= spec["max"].as_f64().unwrap())
            .ok_or_else(|| {
                ApiError::new(
                    axum::http::StatusCode::BAD_REQUEST,
                    "bad_setting",
                    format!(
                        "{} must be between {} and {} {}.",
                        spec["label"].as_str().unwrap(),
                        spec["min"],
                        spec["max"],
                        spec["unit"].as_str().unwrap()
                    ),
                )
            })?;
        validated.insert(key.clone(), json!((n + 0.5).floor()));
    }
    for key in &reset {
        sqlx::query("DELETE FROM site_settings WHERE key=$1")
            .bind(key)
            .execute(&mut **tx)
            .await?;
    }
    let before: Vec<(String, Value)> = sqlx::query_as("SELECT key,value FROM site_settings")
        .fetch_all(&mut **tx)
        .await?;
    let before = effective(&before);
    let mut changed = Map::new();
    for (key, n) in validated {
        if before.get(&key).and_then(Value::as_f64) == n.as_f64() {
            continue;
        }
        sqlx::query("INSERT INTO site_settings(key,value,updated_by,updated_at) VALUES($1,$2,$3,now()) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_by=excluded.updated_by,updated_at=excluded.updated_at").bind(&key).bind(json!({"n":n})).bind(actor).execute(&mut **tx).await?;
        changed.insert(key, n);
    }
    Ok((Value::Object(changed), reset))
}
