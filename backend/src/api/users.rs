use crate::{
    audit,
    auth::{self, Actor},
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, string, truthy},
    identity, identity_crypto,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};

pub async fn owner(state: &AppState, headers: &HeaderMap, method: &Method) -> Result<Actor> {
    let actor = auth::authenticate(state, headers, method).await?;
    if !actor.owner || actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    Ok(actor)
}
/// All account/ownership writes use the same lock order as setup, recovery and self-delete.
pub async fn account_lock(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('identity:accounts'))")
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn revalidate(tx: &mut Transaction<'_, Postgres>, actor: &Actor) -> Result<()> {
    let enabled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM \"user\" WHERE id=$1 AND (NOT coalesce(banned,false) OR coalesce(ban_expires<=now(),false)) AND role='owner')").bind(&actor.id).fetch_one(&mut **tx).await?;
    if !enabled {
        return Err(ApiError::forbidden());
    }
    Ok(())
}
pub async fn revoke_minted(
    tx: &mut Transaction<'_, Postgres>,
    user: &str,
    org: Option<&str>,
) -> Result<Value> {
    let links=sqlx::query("UPDATE org_invites SET revoked_at=now() WHERE created_by=$1 AND revoked_at IS NULL AND ($2::text IS NULL OR org_id=$2)").bind(user).bind(org).execute(&mut **tx).await?.rows_affected();
    let keys=sqlx::query("UPDATE api_keys SET revoked_at=now() WHERE created_by=$1 AND revoked_at IS NULL AND ($2::text IS NULL OR org_id=$2)").bind(user).bind(org).execute(&mut **tx).await?.rows_affected();
    Ok(json!({"invitesRevoked":links,"keysRevoked":keys}))
}
pub async fn keep_owner(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<()> {
    let owners:Vec<String>=sqlx::query_scalar("SELECT id FROM \"user\" WHERE role='owner' AND NOT coalesce(banned,false) ORDER BY id FOR UPDATE").fetch_all(&mut **tx).await?;
    if owners.len() <= 1 && owners.iter().any(|o| o == id) {
        return Err(ApiError::bad("The panel needs at least one owner."));
    }
    Ok(())
}
fn label(v: &Value) -> String {
    v["display_username"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or(v["username"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            v["email"]
                .as_str()
                .unwrap_or("")
                .split('@')
                .next()
                .unwrap_or("")
                .into()
        })
}
async fn user_row(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<Value> {
    sqlx::query_scalar("SELECT to_jsonb(u) FROM \"user\" u WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(ApiError::missing)
}
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    owner(&state, &headers, &Method::GET).await?;
    let mut rows:Vec<Value>=sqlx::query_scalar(r#"SELECT jsonb_build_object('id',u.id,'username',coalesce(nullif(u.display_username,''),u.username,split_part(u.email,'@',1)),
        'name',coalesce(nullif(u.name,''),u.display_username,u.username,''),'role',CASE WHEN u.role='owner' THEN 'owner' ELSE 'member' END,
        'disabled',coalesce(u.banned,false),'mustChangePassword',u.must_change_password,'authComplete',u.auth_complete,'image',u.image,'createdAt',u.created_at,
        'lastLoginAt',(SELECT max(created_at) FROM session WHERE user_id=u.id),
        'grants',coalesce((SELECT jsonb_agg(jsonb_build_object('serverId',s.id,'serverName',s.name,'roleId',r.id,'roleName',r.name) ORDER BY s.sort_order,s.name) FROM server_grants g JOIN servers s ON s.id=g.server_id JOIN org_roles r ON r.id=g.role_id WHERE g.user_id=u.id),'[]'),
        'orgs',coalesce((SELECT jsonb_agg(jsonb_build_object('orgId',o.id,'orgName',o.name,'role',m.role) ORDER BY o.name) FROM org_members m JOIN organizations o ON o.id=m.org_id WHERE m.user_id=u.id),'[]'),
        'providers',coalesce((SELECT jsonb_agg(provider_id ORDER BY created_at,id) FROM account WHERE user_id=u.id),'[]'),
        'twoFactor',coalesce(u.two_factor_enabled,false),'passkeys',(SELECT count(*) FROM passkey WHERE user_id=u.id),'recovery',u.recovery_key_hash IS NOT NULL)
        FROM "user" u ORDER BY u.username,u.name"#).fetch_all(&state.db).await?;
    for row in &mut rows {
        let mut sign_in = vec![];
        let providers = row["providers"].as_array().cloned().unwrap_or_default();
        if providers.iter().any(|p| p == "credential") {
            sign_in.push(json!(if row["twoFactor"] == true {
                "password + authenticator"
            } else {
                "password"
            }))
        }
        let n = row["passkeys"].as_i64().unwrap_or(0);
        if n > 0 {
            sign_in.push(json!(if n == 1 {
                "passkey".into()
            } else {
                format!("passkey ×{n}")
            }))
        }
        for p in providers {
            if p != "credential" {
                sign_in.push(p)
            }
        }
        if row["recovery"] == true {
            sign_in.push(json!("recovery key"))
        }
        row["signIn"] = json!(sign_in);
        for key in ["providers", "twoFactor", "passkeys", "recovery"] {
            row.as_object_mut().unwrap().remove(key);
        }
    }
    Ok(Json(json!({"ok":true,"users":rows})))
}
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let actor = owner(&state, &headers, &Method::POST).await?;
    let username = identity_crypto::username_valid(&string(&body["username"], 32))?;
    let password = body["password"]
        .as_str()
        .ok_or_else(|| ApiError::bad("Password is required."))?;
    identity_crypto::password_valid(password)?;
    let hash = identity_crypto::hash_password(password.into()).await?;
    let role = if body["role"] == "owner" {
        "owner"
    } else {
        "member"
    };
    let name = string(&body["displayName"], 80);
    let name = if name.is_empty() {
        username.clone()
    } else {
        name
    };
    let id = uuid::Uuid::new_v4().to_string();
    let email = format!("{}@warcon.invalid", username.to_lowercase());
    let mut tx = state.db.begin().await?;
    account_lock(&mut tx).await?;
    revalidate(&mut tx, &actor).await?;
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM \"user\" WHERE lower(username)=lower($1) OR email=$2)",
    )
    .bind(&username)
    .bind(&email)
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "username_taken",
            "That username is taken.",
        ));
    }
    sqlx::query("INSERT INTO \"user\"(id,name,email,username,display_username,role,must_change_password) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(&id).bind(name).bind(email).bind(username.to_lowercase()).bind(&username).bind(role).bind(body["mustChangePassword"]!=false).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO account(id,account_id,provider_id,user_id,password) VALUES($1,$2,'credential',$2,$3)").bind(uuid::Uuid::new_v4().to_string()).bind(&id).bind(hash).execute(&mut *tx).await?;
    identity::refresh(&mut tx, &id).await?;
    audit::event(
        &mut tx,
        &actor,
        None,
        &headers,
        "user",
        "user.create",
        &username,
        json!({"role":role,"mustChangePassword":body["mustChangePassword"]!=false}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"ok":true,"id":id}))))
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = owner(&state, &headers, &Method::PATCH).await?;
    let hash = if let Some(p) = body.get("password") {
        let p = p
            .as_str()
            .ok_or_else(|| ApiError::bad("Invalid password."))?;
        identity_crypto::password_valid(p)?;
        Some(identity_crypto::hash_password(p.into()).await?)
    } else {
        None
    };
    let mut tx = state.db.begin().await?;
    account_lock(&mut tx).await?;
    revalidate(&mut tx, &actor).await?;
    let mut u = user_row(&mut tx, &id).await?;
    let username = label(&u);
    let was_owner = u["role"] == "owner";
    let mut changes = json!({});
    let mut sign_out = false;
    if let Some(v) = body.get("displayName") {
        let name = string(v, 80);
        u["name"] = json!(if name.is_empty() {
            username.clone()
        } else {
            name
        });
        changes["displayName"] = json!(true)
    }
    if let Some(v) = body.get("role") {
        let role = if v == "owner" { "owner" } else { "member" };
        if role != "owner" && id == actor.id {
            return Err(ApiError::bad("You cannot demote yourself."));
        }
        if was_owner && role != "owner" {
            keep_owner(&mut tx, &id).await?
        }
        u["role"] = json!(role);
        changes["role"] = json!(role)
    }
    if let Some(v) = body.get("disabled") {
        let disabled = truthy(v);
        if disabled && id == actor.id {
            return Err(ApiError::bad("You cannot disable yourself."));
        }
        if disabled && was_owner {
            keep_owner(&mut tx, &id).await?
        }
        u["banned"] = json!(disabled);
        u["ban_reason"] = if disabled {
            json!("Disabled by owner")
        } else {
            Value::Null
        };
        u["ban_expires"] = Value::Null;
        sign_out |= disabled;
        changes["disabled"] = json!(disabled)
    }
    if let Some(hash) = hash {
        let n=sqlx::query("UPDATE account SET password=$2,updated_at=now() WHERE user_id=$1 AND provider_id='credential'").bind(&id).bind(&hash).execute(&mut *tx).await?.rows_affected();
        if n == 0 {
            sqlx::query("INSERT INTO account(id,account_id,provider_id,user_id,password) VALUES($1,$2,'credential',$2,$3)").bind(uuid::Uuid::new_v4().to_string()).bind(&id).bind(hash).execute(&mut *tx).await?;
        }
        u["must_change_password"] = json!(body["mustChangePassword"] != false);
        sign_out = true;
        changes["passwordReset"] = json!(true);
    } else if let Some(v) = body.get("mustChangePassword") {
        u["must_change_password"] = json!(truthy(v));
        changes["mustChangePassword"] = u["must_change_password"].clone()
    }
    if truthy(&body["resetAuth"]) {
        if id == actor.id {
            return Err(ApiError::bad(
                "Reset your own methods from the account page.",
            ));
        }
        for table in ["two_factor", "passkey"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE user_id=$1"))
                .bind(&id)
                .execute(&mut *tx)
                .await?;
        }
        u["two_factor_enabled"] = json!(false);
        u["recovery_key_hash"] = Value::Null;
        u["recovery_key_at"] = Value::Null;
        changes["resetAuth"] = json!(true);
        sign_out = true
    }
    if changes.as_object().unwrap().is_empty() {
        return Err(ApiError::bad("Nothing to update."));
    }
    sqlx::query("UPDATE \"user\" SET name=$2,role=$3,banned=$4,ban_reason=$5,ban_expires=$6,must_change_password=$7,two_factor_enabled=$8,recovery_key_hash=$9,recovery_key_at=$10,updated_at=now() WHERE id=$1")
        .bind(&id).bind(u["name"].as_str().unwrap_or("")).bind(u["role"].as_str()).bind(u["banned"].as_bool()).bind(u["ban_reason"].as_str()).bind(date(&u["ban_expires"]))
        .bind(u["must_change_password"].as_bool().unwrap_or(false)).bind(u["two_factor_enabled"].as_bool()).bind(u["recovery_key_hash"].as_str()).bind(date(&u["recovery_key_at"])).execute(&mut *tx).await?;
    if sign_out {
        sqlx::query("DELETE FROM session WHERE user_id=$1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM verification WHERE value=$1 AND (identifier LIKE 'trust-device-%' OR identifier LIKE '2fa-%')").bind(&id).execute(&mut *tx).await?;
    }
    if changes["disabled"] == true || (was_owner && changes["role"] == "member") {
        for (key, val) in revoke_minted(&mut tx, &id, None)
            .await?
            .as_object()
            .unwrap()
        {
            changes[key] = val.clone()
        }
    }
    identity::refresh(&mut tx, &id).await?;
    audit::event(
        &mut tx,
        &actor,
        None,
        &headers,
        "user",
        "user.update",
        &username,
        changes,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
fn date(v: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.to_utc())
}
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = owner(&state, &headers, &Method::DELETE).await?;
    if id == actor.id {
        return Err(ApiError::bad("You cannot delete yourself."));
    }
    let mut tx = state.db.begin().await?;
    account_lock(&mut tx).await?;
    revalidate(&mut tx, &actor).await?;
    let u = user_row(&mut tx, &id).await?;
    if u["role"] == "owner" {
        keep_owner(&mut tx, &id).await?
    }
    let orgs:Vec<(String,String)>=sqlx::query_as("SELECT o.id,o.name FROM organizations o JOIN org_members m ON m.org_id=o.id WHERE m.user_id=$1 AND m.role='owner' ORDER BY o.id FOR UPDATE OF o").bind(&id).fetch_all(&mut *tx).await?;
    for (org, name) in orgs {
        let others:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM org_members WHERE org_id=$1 AND role='owner' AND user_id<>$2)").bind(org).bind(&id).fetch_one(&mut *tx).await?;
        if !others {
            return Err(ApiError::bad(format!(
                "Promote another owner of {name} first."
            )));
        }
    }
    let ended = revoke_minted(&mut tx, &id, None).await?;
    sqlx::query("DELETE FROM verification WHERE value=$1 AND (identifier LIKE 'trust-device-%' OR identifier LIKE '2fa-%')").bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM \"user\" WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    audit::event(
        &mut tx,
        &actor,
        None,
        &headers,
        "user",
        "user.delete",
        &label(&u),
        ended,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
/// Replace all grants or just one organisation. Roles are joined to the server's org in SQL.
pub async fn apply_grants(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    user: &str,
    org: Option<&str>,
    wanted: &Value,
) -> Result<Value> {
    let known:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('serverId',s.id,'orgId',s.org_id,'roleId',r.id,'roleName',r.name) FROM servers s JOIN org_roles r ON r.org_id=s.org_id WHERE ($1::text IS NULL OR s.org_id=$1) FOR SHARE OF s,r").bind(org).fetch_all(&mut **tx).await?;
    let mut applied = vec![];
    let mut seen = std::collections::HashSet::new();
    for g in wanted.as_array().into_iter().flatten() {
        let server = string(&g["serverId"], 64);
        let role = string(&g["roleId"], 64);
        if seen.contains(&server) {
            continue;
        }
        if let Some(row) = known
            .iter()
            .find(|k| k["serverId"] == server && k["roleId"] == role)
        {
            seen.insert(server);
            applied.push(row.clone());
        }
    }
    sqlx::query("DELETE FROM server_grants WHERE user_id=$1 AND ($2::text IS NULL OR server_id IN(SELECT id FROM servers WHERE org_id=$2))").bind(user).bind(org).execute(&mut **tx).await?;
    for row in &applied {
        sqlx::query("INSERT INTO org_members(org_id,user_id,role) VALUES($1,$2,'member') ON CONFLICT DO NOTHING").bind(row["orgId"].as_str()).bind(user).execute(&mut **tx).await?;
        sqlx::query(
            "INSERT INTO server_grants(server_id,user_id,role_id,granted_by) VALUES($1,$2,$3,$4)",
        )
        .bind(row["serverId"].as_str())
        .bind(user)
        .bind(row["roleId"].as_str())
        .bind(&actor.id)
        .execute(&mut **tx)
        .await?;
    }
    for row in &mut applied {
        row.as_object_mut().unwrap().remove("orgId");
    }
    Ok(json!(applied))
}
pub async fn grants(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = owner(&state, &headers, &Method::PUT).await?;
    let mut tx = state.db.begin().await?;
    account_lock(&mut tx).await?;
    revalidate(&mut tx, &actor).await?;
    let u = user_row(&mut tx, &id).await?;
    let applied = apply_grants(&mut tx, &actor, &id, None, &body["grants"]).await?;
    audit::event(
        &mut tx,
        &actor,
        None,
        &headers,
        "user",
        "user.grants",
        &label(&u),
        json!({"grants":applied}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"grants":applied})))
}
