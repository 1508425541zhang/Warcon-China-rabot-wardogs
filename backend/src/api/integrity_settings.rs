use crate::{
    api::roles::require_org_owner,
    audit,
    auth::authenticate,
    config::AppState,
    error::{ApiError, Result},
    http::ApiJson,
    integrity_rules as rules, integrity_score, integrity_weapons as weapons,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};

pub async fn get_rules(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    require_org_owner(&state, &actor, &org).await?;
    Ok(Json(
        json!({"ok":true,"rules":rules::load(&state.db,&org).await?}),
    ))
}
pub async fn put_rules(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::PUT).await?;
    require_org_owner(&state, &actor, &org).await?;
    let empty = json!({});
    let patch = body
        .get("values")
        .filter(|v| !v.is_null())
        .unwrap_or(&empty);
    let fields = patch
        .as_object()
        .ok_or_else(|| ApiError::bad("Integrity rules must be an object."))?;
    if fields.is_empty() {
        return Err(ApiError::bad("No integrity rule changes supplied."));
    }
    if fields.contains_key("mode") || fields.contains_key("quarantineDays") {
        return Err(ApiError::bad(
            "Legacy mode and quarantineDays cannot be changed. Use enforcement switches for actions and duration.",
        ));
    }
    let mut tx = state.db.begin().await?;
    let before = rules::lock(&mut tx, &org).await?;
    let old = rules::from_row(before.as_ref())?;
    if old.assessment_mode != "legacy" && fields.keys().any(|k| k != "committeeKpmMinutes") {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "legacy_rules_locked",
            "Legacy score rules are editable only in legacy assessment mode.",
        ));
    }
    let config = integrity_score::validate(patch, &old.config)?;
    let row:Value=sqlx::query_scalar("INSERT INTO integrity_rules(org_id,config,updated_by) VALUES($1,$2,$3) ON CONFLICT(org_id) DO UPDATE SET config=excluded.config,version=integrity_rules.version+1,updated_by=excluded.updated_by,updated_at=now() RETURNING to_jsonb(integrity_rules)").bind(&org).bind(config).bind(&actor.id).fetch_one(&mut *tx).await?;
    let saved = rules::from_row(Some(&row))?;
    rules::changed(&mut tx, &org).await?;
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "integrity.rules.update",
        "",
        json!({"beforeVersion":old.version,"version":saved.version,"patch":patch}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"rules":saved})))
}
pub async fn put_mode(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::PUT).await?;
    require_org_owner(&state, &actor, &org).await?;
    let mode = body["mode"]
        .as_str()
        .filter(|m| rules::MODES.contains(m))
        .ok_or_else(|| ApiError::bad("Unknown Integrity assessment mode."))?;
    if mode == "statistical" && body["confirmation"] != "ENABLE_STATISTICAL_INTEGRITY" {
        return Err(ApiError::bad(
            "Explicit statistical enforcement confirmation is required.",
        ));
    }
    let mut tx = state.db.begin().await?;
    rules::lock(&mut tx, &org).await?;
    if rules::long_enabled(mode) {
        let enabled:bool=sqlx::query_scalar("SELECT coalesce((SELECT value->'developerEnabled'='true'::jsonb FROM site_settings WHERE key=$1),false)").bind(format!("integrityModel:{org}")).fetch_one(&mut *tx).await?;
        if !enabled {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "model_disabled",
                "请先配置模型 HTTP API 并启用开发者状态。",
            ));
        }
    }
    let row:Value=sqlx::query_scalar("INSERT INTO integrity_rules(org_id,config,assessment_mode,updated_by) VALUES($1,$2,$3,$4) ON CONFLICT(org_id) DO UPDATE SET assessment_mode=excluded.assessment_mode,version=integrity_rules.version+1,updated_by=excluded.updated_by,updated_at=now() RETURNING to_jsonb(integrity_rules)").bind(&org).bind(integrity_score::defaults()).bind(mode).bind(&actor.id).fetch_one(&mut *tx).await?;
    let saved = rules::from_row(Some(&row))?;
    rules::changed(&mut tx, &org).await?;
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "integrity.assessment_mode.update",
        "",
        json!({"mode":mode,"version":saved.version}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"rules":saved})))
}
pub async fn get_enforcement(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    require_org_owner(&state, &actor, &org).await?;
    Ok(Json(
        json!({"ok":true,"enforcement":rules::load(&state.db,&org).await?.enforcement}),
    ))
}
pub async fn put_enforcement(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::PUT).await?;
    require_org_owner(&state, &actor, &org).await?;
    let mut tx = state.db.begin().await?;
    let row = rules::lock(&mut tx, &org).await?;
    let old = rules::from_row(row.as_ref())?;
    let empty = json!({});
    let values = body
        .get("values")
        .filter(|v| !v.is_null())
        .unwrap_or(&empty);
    let next = rules::validate_enforcement(&old.enforcement, values)?;
    let row:Value=sqlx::query_scalar("INSERT INTO integrity_rules(org_id,config,auto_kick_enabled,auto_quarantine_24h_enabled,auto_quarantine_7d_enabled,auto_action_max_per_hour,auto_action_max_percent_online,auto_suspended_at,updated_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(org_id) DO UPDATE SET auto_kick_enabled=excluded.auto_kick_enabled,auto_quarantine_24h_enabled=excluded.auto_quarantine_24h_enabled,auto_quarantine_7d_enabled=excluded.auto_quarantine_7d_enabled,auto_action_max_per_hour=excluded.auto_action_max_per_hour,auto_action_max_percent_online=excluded.auto_action_max_percent_online,auto_suspended_at=excluded.auto_suspended_at,updated_by=excluded.updated_by,updated_at=now() RETURNING to_jsonb(integrity_rules)").bind(&org).bind(&old.config).bind(next["autoKickEnabled"].as_bool().unwrap()).bind(next["autoQuarantine24hEnabled"].as_bool().unwrap()).bind(next["autoQuarantine7dEnabled"].as_bool().unwrap()).bind(next["autoActionMaxPerHour"].as_i64().unwrap() as i32).bind(next["autoActionMaxPercentOnline"].as_i64().unwrap() as i32).bind(next["autoSuspendedAt"].as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t|t.with_timezone(&chrono::Utc))).bind(&actor.id).fetch_one(&mut *tx).await?;
    let enforcement = rules::from_row(Some(&row))?.enforcement;
    rules::changed(&mut tx, &org).await?;
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        if values["resume"] == true {
            "integrity.enforcement.resume"
        } else {
            "integrity.enforcement.update"
        },
        "",
        json!({"before":old.enforcement,"after":enforcement}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"enforcement":enforcement})))
}
fn mapping(row: Value) -> Value {
    json!({"orgId":row["org_id"],"cause":row["cause"],"category":row["category"],"updatedBy":row["updated_by"],"updatedAt":row["updated_at"].as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t|t.to_rfc3339_opts(chrono::SecondsFormat::Millis,true))})
}
fn cause(input: &Value) -> Result<String> {
    let cause = crate::feed::truncate(input.as_str().unwrap_or(""), 200);
    if cause.is_empty()
        || cause.len() > 180
        || !cause
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err(ApiError::bad(
            "Cause must be an exact server-feed tag (1–180 characters).",
        ));
    }
    Ok(cause)
}
async fn invalidate_weapons(tx: &mut Transaction<'_, Postgres>, org: &str) -> Result<()> {
    sqlx::query("INSERT INTO integrity_model_state(org_id,weapon_map_version,baseline_status) VALUES($1,2,'STALE') ON CONFLICT(org_id) DO UPDATE SET weapon_map_version=integrity_model_state.weapon_map_version+1,baseline_status='STALE',updated_at=now()").bind(org).execute(&mut **tx).await?;
    rules::changed(tx, org).await
}
pub async fn get_weapons(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    require_org_owner(&state, &actor, &org).await?;
    let rows: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(w) FROM integrity_weapon_map w WHERE org_id=$1")
            .bind(&org)
            .fetch_all(&state.db)
            .await?;
    Ok(Json(
        json!({"ok":true,"categories":weapons::CATEGORIES,"defaults":weapons::defaults(),"overrides":rows.into_iter().map(mapping).collect::<Vec<_>>()}),
    ))
}
pub async fn put_weapon(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::PUT).await?;
    require_org_owner(&state, &actor, &org).await?;
    let cause = cause(&body["cause"])?;
    let category = body["category"]
        .as_str()
        .filter(|c| weapons::CATEGORIES.contains(c))
        .ok_or_else(|| ApiError::bad("Unknown weapon category."))?;
    let mut tx = state.db.begin().await?;
    let row:Value=sqlx::query_scalar("INSERT INTO integrity_weapon_map(org_id,cause,category,updated_by) VALUES($1,$2,$3,$4) ON CONFLICT(org_id,cause) DO UPDATE SET category=excluded.category,updated_by=excluded.updated_by,updated_at=now() RETURNING to_jsonb(integrity_weapon_map)").bind(&org).bind(&cause).bind(category).bind(&actor.id).fetch_one(&mut *tx).await?;
    invalidate_weapons(&mut tx, &org).await?;
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "integrity.weapon_map.set",
        &cause,
        json!({"cause":cause,"category":category}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"mapping":mapping(row)})))
}
pub async fn delete_weapon(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::DELETE).await?;
    require_org_owner(&state, &actor, &org).await?;
    let cause = cause(&body["cause"])?;
    let mut tx = state.db.begin().await?;
    let removed = sqlx::query("DELETE FROM integrity_weapon_map WHERE org_id=$1 AND cause=$2")
        .bind(&org)
        .bind(&cause)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if removed == 0 {
        return Err(ApiError::missing());
    }
    invalidate_weapons(&mut tx, &org).await?;
    audit::org_event(&mut tx,&actor,&org,&headers,"integrity.weapon_map.remove",&cause,json!({"cause":cause,"fallback":weapons::defaults().get(&cause).unwrap_or(&json!("UNKNOWN"))})).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
