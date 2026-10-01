use crate::http::ApiJson;
use crate::{
    audit::org_event,
    auth::{Actor, CAPS, authenticate},
    config::AppState,
    error::{ApiError, Result},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};

pub async fn require_org_owner(state: &AppState, actor: &Actor, id: &str) -> Result<()> {
    if actor.key.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "api_key_forbidden",
            "API keys cannot manage an organisation or the panel.",
        ));
    }
    let org:Option<(Option<chrono::DateTime<chrono::Utc>>,Option<String>)>=sqlx::query_as("SELECT suspended_at,(SELECT role FROM org_members m WHERE m.org_id=o.id AND m.user_id=$2) FROM organizations o WHERE id=$1").bind(id).bind(&actor.id).fetch_optional(&state.db).await?;
    let Some((suspended, member)) = org else {
        return Err(ApiError::missing());
    };
    if !actor.owner && member.is_none() {
        return Err(ApiError::missing());
    }
    if !actor.owner && suspended.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "suspended",
            "This organisation is suspended.",
        ));
    }
    if !actor.owner && member.as_deref() != Some("owner") {
        return Err(ApiError::forbidden());
    }
    Ok(())
}
fn capabilities(raw: &Value) -> Result<Value> {
    let values = raw
        .as_array()
        .ok_or_else(|| ApiError::bad("capabilities must be a list."))?;
    if values
        .iter()
        .any(|v| v.as_str().is_none_or(|s| !CAPS.contains(&s)))
    {
        return Err(ApiError::bad("Unknown capability."));
    }
    let caps: Vec<&str> = CAPS
        .iter()
        .copied()
        .filter(|s| values.iter().any(|v| v.as_str() == Some(s)))
        .collect();
    if !caps.contains(&"server.view") {
        return Err(ApiError::bad(
            "Every role must include 'View'; remove the member's grant instead.",
        ));
    }
    Ok(json!(caps))
}
fn name(raw: &Value) -> Result<String> {
    let value = raw
        .as_str()
        .unwrap_or("")
        .trim()
        .chars()
        .take(40)
        .collect::<String>();
    if value.chars().count() < 2 {
        return Err(ApiError::bad("Role name must be at least 2 characters."));
    }
    Ok(value)
}
pub(crate) fn builtin_caps(kind: &str) -> Value {
    match kind {
        "viewer" => json!(["server.view"]),
        "operator" => json!([
            "server.view",
            "chat.send",
            "players.moderate",
            "match.control",
            "rotation.edit",
            "players.notes"
        ]),
        "admin" => json!(CAPS),
        _ => Value::Null,
    }
}
async fn lock_role(tx: &mut Transaction<'_, Postgres>, org: &str, role: &str) -> Result<Value> {
    sqlx::query_scalar("SELECT to_jsonb(r) FROM org_roles r WHERE id=$1 AND org_id=$2 FOR UPDATE")
        .bind(role)
        .bind(org)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(ApiError::missing)
}
async fn check_name(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    label: &str,
    except: &str,
) -> Result<()> {
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM org_roles WHERE org_id=$1 AND lower(name)=lower($2) AND id<>$3)").bind(org).bind(label).bind(except).fetch_one(&mut **tx).await?;
    if exists {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "A role with this name already exists.",
        ));
    }
    Ok(())
}
fn write_error(error: sqlx::Error) -> ApiError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("23505") => ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "A role with this name already exists.",
        ),
        Some("23503") => {
            ApiError::new(StatusCode::CONFLICT, "in_use", "This role is still in use.")
        }
        _ => error.into(),
    }
}
async fn usage(tx: &mut Transaction<'_, Postgres>, org: &str, role: &str) -> Result<(i64, i64)> {
    Ok(sqlx::query_as("SELECT (SELECT count(*) FROM server_grants g JOIN servers s ON s.id=g.server_id WHERE s.org_id=$1 AND g.role_id=$2),(SELECT count(*) FROM org_invites WHERE org_id=$1 AND server_role_id=$2 AND revoked_at IS NULL)").bind(org).bind(role).fetch_one(&mut **tx).await?)
}
fn view(row: &Value, used: (i64, i64)) -> Value {
    let caps: Vec<_> = CAPS
        .iter()
        .copied()
        .filter(|c| {
            row["capabilities"]
                .as_array()
                .is_some_and(|v| v.iter().any(|v| v.as_str() == Some(c)))
        })
        .collect();
    let date = |name: &str| {
        row[name]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| {
                d.to_utc()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            })
    };
    json!({"id":row["id"],"name":row["name"],"capabilities":caps,"builtin":row["builtin"],"sortOrder":row["sort_order"],"inUse":{"grants":used.0,"invites":used.1},"createdAt":date("created_at"),"updatedAt":date("updated_at")})
}
pub async fn list(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    require_org_owner(&state, &actor, &org).await?;
    let mut tx = state.db.begin().await?;
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(r) FROM org_roles r WHERE org_id=$1 ORDER BY sort_order,name",
    )
    .bind(&org)
    .fetch_all(&mut *tx)
    .await?;
    let mut roles = Vec::new();
    for row in rows {
        let used = usage(&mut tx, &org, row["id"].as_str().unwrap_or("")).await?;
        roles.push(view(&row, used))
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"roles":roles})))
}
pub async fn create(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let actor = authenticate(&state, &headers, &Method::POST).await?;
    require_org_owner(&state, &actor, &org).await?;
    let label = name(&body["name"])?;
    let caps = capabilities(body.get("capabilities").unwrap_or(&json!(["server.view"])))?;
    let id = uuid::Uuid::new_v4().to_string();
    let mut tx = state.db.begin().await?;
    check_name(&mut tx, &org, &label, "").await?;
    let row:Value=sqlx::query_scalar("INSERT INTO org_roles(id,org_id,name,capabilities,sort_order) VALUES($1,$2,$3,$4,100) RETURNING to_jsonb(org_roles)").bind(&id).bind(&org).bind(&label).bind(&caps).fetch_one(&mut *tx).await.map_err(write_error)?;
    org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "org.role.create",
        &label,
        json!({"orgId":org,"roleId":id,"capabilities":caps}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"ok":true,"role":view(&row,(0,0))})),
    ))
}
async fn modify(
    state: AppState,
    org: String,
    id: String,
    headers: HeaderMap,
    body: Value,
    reset: bool,
) -> Result<Json<Value>> {
    let actor = authenticate(
        &state,
        &headers,
        if reset { &Method::POST } else { &Method::PATCH },
    )
    .await?;
    require_org_owner(&state, &actor, &org).await?;
    let mut tx = state.db.begin().await?;
    let old = lock_role(&mut tx, &org, &id).await?;
    let (label, caps) = if reset {
        let builtin = old["builtin"].as_str().ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "not_builtin",
                "Only built-in roles can be reset.",
            )
        })?;
        (builtin.to_owned(), builtin_caps(builtin))
    } else {
        (
            if let Some(n) = body.get("name") {
                name(n)?
            } else {
                old["name"].as_str().unwrap_or("").into()
            },
            if let Some(c) = body.get("capabilities") {
                capabilities(c)?
            } else {
                old["capabilities"].clone()
            },
        )
    };
    check_name(&mut tx, &org, &label, &id).await?;
    let row:Value=sqlx::query_scalar("UPDATE org_roles SET name=$3,capabilities=$4,updated_at=now() WHERE id=$1 AND org_id=$2 RETURNING to_jsonb(org_roles)").bind(&id).bind(&org).bind(&label).bind(&caps).fetch_one(&mut *tx).await.map_err(write_error)?;
    let action = if reset {
        "org.role.reset"
    } else {
        "org.role.update"
    };
    org_event(&mut tx,&actor,&org,&headers,action,&label,json!({"orgId":org,"roleId":id,"from":{"name":old["name"],"capabilities":old["capabilities"]},"to":{"name":label,"capabilities":caps}})).await?;
    let used = usage(&mut tx, &org, &id).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"role":view(&row,used)})))
}
pub async fn update(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    modify(state, org, id, headers, body, false).await
}
pub async fn reset(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    modify(state, org, id, headers, Value::Null, true).await
}
pub async fn delete(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::DELETE).await?;
    require_org_owner(&state, &actor, &org).await?;
    let mut tx = state.db.begin().await?;
    let old = lock_role(&mut tx, &org, &id).await?;
    if !old["builtin"].is_null() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "builtin",
            "Built-in roles cannot be deleted; reset it instead.",
        ));
    }
    let used = usage(&mut tx, &org, &id).await?;
    if used.0 + used.1 > 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "in_use",
            "This role is still used by grants or invite links.",
        ));
    }
    sqlx::query("DELETE FROM org_roles WHERE org_id=$1 AND id=$2")
        .bind(&org)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(write_error)?;
    org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "org.role.delete",
        old["name"].as_str().unwrap_or(""),
        json!({"orgId":org,"roleId":id,"capabilities":old["capabilities"]}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
