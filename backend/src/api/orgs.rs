use super::{roles, users};
use crate::{
    audit,
    auth::{self, Actor},
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, integer, string, truthy},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use unicode_normalization::UnicodeNormalization;

async fn panel_user(state: &AppState, headers: &HeaderMap, method: &Method) -> Result<Actor> {
    let actor = auth::authenticate(state, headers, method).await?;
    if actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    Ok(actor)
}
/// Lock accounts, then org, then memberships. Re-check actor privileges after waiting.
pub async fn locked_org(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    id: &str,
) -> Result<Value> {
    users::account_lock(tx).await?;
    let user: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT role,(NOT coalesce(banned,false) OR coalesce(ban_expires<=now(),false)) FROM \"user\" WHERE id=$1",
    )
    .bind(&actor.id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((role, true)) = user else {
        return Err(ApiError::forbidden());
    };
    if actor.owner != (role.as_deref() == Some("owner")) {
        return Err(ApiError::forbidden());
    }
    let org: Value =
        sqlx::query_scalar("SELECT to_jsonb(o) FROM organizations o WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(ApiError::missing)?;
    if role.as_deref() != Some("owner") {
        let member: Option<String> =
            sqlx::query_scalar("SELECT role FROM org_members WHERE org_id=$1 AND user_id=$2")
                .bind(id)
                .bind(&actor.id)
                .fetch_optional(&mut **tx)
                .await?;
        if member.is_none() {
            return Err(ApiError::missing());
        }
        if !org["suspended_at"].is_null() {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "suspended",
                "This organisation is suspended.",
            ));
        }
        if member.as_deref() != Some("owner") {
            return Err(ApiError::forbidden());
        }
    }
    Ok(org)
}
fn name(value: &Value) -> Result<String> {
    let n = string(value, 60);
    if n.encode_utf16().count() < 2 {
        return Err(ApiError::bad(
            "Organisation name must be at least 2 characters.",
        ));
    }
    Ok(n)
}
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut separator = false;
    for c in name.to_lowercase().nfkd() {
        if c.is_ascii_alphanumeric() {
            if separator && !out.is_empty() {
                out.push('-')
            }
            out.push(c);
            separator = false
        } else {
            separator = true
        }
    }
    let out = out.chars().take(40).collect::<String>();
    if out.is_empty() { "org".into() } else { out }
}
async fn free_slug(tx: &mut Transaction<'_, Postgres>, base: &str, except: &str) -> Result<String> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('organization:slugs'))")
        .execute(&mut **tx)
        .await?;
    for i in 0..50 {
        let candidate = if i == 0 {
            base.into()
        } else {
            format!("{base}-{}", i + 1)
        };
        let used: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM organizations WHERE slug=$1 AND id<>$2)",
        )
        .bind(&candidate)
        .bind(except)
        .fetch_one(&mut **tx)
        .await?;
        if !used {
            return Ok(candidate);
        }
    }
    Ok(format!(
        "{base}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ))
}
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let actor = panel_user(&state, &headers, &Method::GET).await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',o.id,'name',o.name,'slug',o.slug,'memberCount',(SELECT count(*) FROM org_members WHERE org_id=o.id),'serverCount',(SELECT count(*) FROM servers WHERE org_id=o.id),'serverLimit',coalesce(o.server_limit,$3::bigint),'customServerLimit',o.server_limit,'suspended',CASE WHEN o.suspended_at IS NULL THEN NULL ELSE jsonb_build_object('at',o.suspended_at,'reason',o.suspended_reason) END,'allowPublicStatus',o.allow_public_status,'allowPublicLeaderboards',o.allow_public_leaderboards,'discordInviteUrl',o.discord_invite_url,'createdBy',CASE WHEN u.id IS NULL THEN NULL ELSE jsonb_build_object('username',coalesce(u.username,''),'name',u.name) END,'createdAt',o.created_at) FROM organizations o LEFT JOIN \"user\" u ON u.id=o.created_by WHERE $1 OR EXISTS(SELECT 1 FROM org_members m WHERE m.org_id=o.id AND m.user_id=$2 AND m.role='owner') ORDER BY o.name").bind(actor.owner).bind(&actor.id).bind(state.config.organizations.max_servers).fetch_all(&state.db).await?;
    Ok(Json(json!({"ok":true,"orgs":rows})))
}
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let actor = panel_user(&state, &headers, &Method::POST).await?;
    let n = name(&body["name"])?;
    let id = uuid::Uuid::new_v4().to_string();
    let mut tx = state.db.begin().await?;
    users::account_lock(&mut tx).await?;
    let enabled:Option<(Option<String>,bool)>=sqlx::query_as("SELECT role,(NOT coalesce(banned,false) OR coalesce(ban_expires<=now(),false)) FROM \"user\" WHERE id=$1 FOR UPDATE").bind(&actor.id).fetch_optional(&mut *tx).await?;
    let Some((role, true)) = enabled else {
        return Err(ApiError::forbidden());
    };
    if role.as_deref() != Some("owner") {
        if !state.config.identity.allow_signup {
            return Err(ApiError::forbidden());
        }
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM organizations WHERE created_by=$1")
                .bind(&actor.id)
                .fetch_one(&mut *tx)
                .await?;
        if count >= state.config.organizations.max_per_user {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "limit",
                "Organization creation limit reached.",
            ));
        }
    }
    let slug = free_slug(&mut tx, &slug(&n), "").await?;
    sqlx::query("INSERT INTO organizations(id,name,slug,created_by) VALUES($1,$2,$3,$4)")
        .bind(&id)
        .bind(&n)
        .bind(&slug)
        .bind(&actor.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO org_members(org_id,user_id,role) VALUES($1,$2,'owner')")
        .bind(&id)
        .bind(&actor.id)
        .execute(&mut *tx)
        .await?;
    for (i, kind) in ["viewer", "operator", "admin"].iter().enumerate() {
        sqlx::query("INSERT INTO org_roles(id,org_id,name,capabilities,builtin,sort_order) VALUES($1,$2,$3,$4,$3,$5)").bind(uuid::Uuid::new_v4().to_string()).bind(&id).bind(kind).bind(roles::builtin_caps(kind)).bind(i as i32).execute(&mut *tx).await?;
    }
    for kind in ["ban", "reserve"] {
        sqlx::query("INSERT INTO lists(id,org_id,kind,created_by) VALUES($1,$2,$3,$4)")
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&id)
            .bind(kind)
            .bind(&actor.id)
            .execute(&mut *tx)
            .await?;
    }
    audit::org_event(
        &mut tx,
        &actor,
        &id,
        &headers,
        "org.create",
        &n,
        json!({"orgId":id,"slug":slug}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"ok":true,"id":id}))))
}
pub fn discord_invite(raw: &str) -> Result<String> {
    if raw.is_empty() {
        return Ok(String::new());
    }
    let raw = if raw.to_ascii_lowercase().starts_with("http://")
        || raw.to_ascii_lowercase().starts_with("https://")
    {
        raw.into()
    } else {
        format!("https://{raw}")
    };
    let u = url::Url::parse(&raw).map_err(|_| ApiError::bad("Invalid Discord invite."))?;
    let parts: Vec<_> = u.path().split('/').filter(|p| !p.is_empty()).collect();
    let code = match u.host_str().unwrap_or("") {
        "discord.gg" if parts.len() == 1 => Some(parts[0]),
        "discord.com" | "www.discord.com" | "discordapp.com"
            if parts.len() == 2 && parts[0] == "invite" =>
        {
            Some(parts[1])
        }
        _ => None,
    }
    .ok_or_else(|| ApiError::bad("Invalid Discord invite."))?;
    if !(2..=64).contains(&code.len())
        || !code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(ApiError::bad("Invalid Discord invite."));
    }
    Ok(format!("https://discord.gg/{code}"))
}
pub fn ban_template(raw: &Value) -> Result<String> {
    let v = string(raw, 200)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let v = if v.is_empty() { "{reason}".into() } else { v };
    for (start, _) in v.match_indices('{') {
        let tail = &v[start + 1..];
        let Some(end) = tail.find('}') else { continue };
        let placeholder = &tail[..end];
        if placeholder.is_empty()
            || !placeholder
                .bytes()
                .all(|b| b.is_ascii_alphabetic() || b == b'_')
        {
            continue;
        }
        if !["reason", "duration", "expires", "banned", "uid", "admin"]
            .contains(&placeholder.to_ascii_lowercase().as_str())
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "unknown_placeholder",
                format!("Unknown placeholder {{{placeholder}}}."),
            ));
        }
    }
    Ok(v)
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = panel_user(&state, &headers, &Method::PATCH).await?;
    let mut tx = state.db.begin().await?;
    let mut o = locked_org(&mut tx, &actor, &id).await?;
    let mut changes = json!({"orgId":id});
    let action;
    let controls = [
        "serverLimit",
        "suspended",
        "allowPublicStatus",
        "allowPublicLeaderboards",
    ]
    .iter()
    .any(|k| body.get(*k).is_some());
    if controls {
        users::revalidate(&mut tx, &actor).await?;
        action = "org.controls";
        if let Some(v) = body.get("serverLimit") {
            let limit = if v.is_null() || v == "" {
                Value::Null
            } else {
                let n = integer(v, -1, 0, 1000);
                if n < 0 {
                    return Err(ApiError::bad("serverLimit must be 0-1000 or empty."));
                }
                json!(n)
            };
            o["server_limit"] = limit.clone();
            changes["serverLimit"] = limit
        }
        for (camel, snake) in [
            ("allowPublicStatus", "allow_public_status"),
            ("allowPublicLeaderboards", "allow_public_leaderboards"),
        ] {
            if let Some(v) = body.get(camel) {
                o[snake] = json!(truthy(v));
                changes[camel] = o[snake].clone();
            }
        }
        if let Some(v) = body.get("suspended") {
            let on = truthy(v);
            if on && o["suspended_at"].is_null() {
                o["suspended_at"] = json!(chrono::Utc::now());
                o["suspended_reason"] = json!(string(&body["reason"], 300));
                changes["suspended"] = json!(true);
                changes["reason"] = o["suspended_reason"].clone()
            } else if !on && !o["suspended_at"].is_null() {
                o["suspended_at"] = Value::Null;
                o["suspended_reason"] = json!("");
                changes["suspended"] = json!(false)
            }
        }
    } else if body.get("membersReserved").is_some() {
        let on = truthy(&body["membersReserved"]);
        sqlx::query("UPDATE organizations SET members_reserved=$2,updated_at=now() WHERE id=$1")
            .bind(&id)
            .bind(on)
            .execute(&mut *tx)
            .await?;
        audit::org_event(
            &mut tx,
            &actor,
            &id,
            &headers,
            "list.members",
            o["name"].as_str().unwrap_or(""),
            json!({"orgId":id,"membersReserved":on}),
        )
        .await?;
        tx.commit().await?;
        return Ok(Json(
            json!({"ok":true,"sync":crate::list_sync::sync_org(&state,&id).await?}),
        ));
    } else if let Some(v) = body.get("banMessage") {
        action = "list.ban_message";
        let template = ban_template(v)?;
        o["ban_message"] = json!(template);
        changes["banMessage"] = o["ban_message"].clone();
    } else {
        action = "org.update";
        if let Some(v) = body.get("name") {
            let n = name(v)?;
            o["slug"] = json!(free_slug(&mut tx, &slug(&n), &id).await?);
            changes["from"] = o["name"].clone();
            o["name"] = json!(n);
        }
        if let Some(v) = body.get("discordInviteUrl") {
            let link = discord_invite(&string(v, 200))?;
            o["discord_invite_url"] = json!(link);
            changes["discordInviteUrl"] = o["discord_invite_url"].clone();
        }
    }
    if changes.as_object().unwrap().len() == 1 {
        return Err(ApiError::bad("Nothing to update."));
    }
    sqlx::query("UPDATE organizations SET name=$2,slug=$3,server_limit=$4,suspended_at=$5,suspended_reason=$6,allow_public_status=$7,allow_public_leaderboards=$8,discord_invite_url=$9,ban_message=$10,updated_at=now() WHERE id=$1")
        .bind(&id).bind(o["name"].as_str()).bind(o["slug"].as_str()).bind(o["server_limit"].as_i64().map(|n|n as i32)).bind(date(&o["suspended_at"])).bind(o["suspended_reason"].as_str()).bind(o["allow_public_status"].as_bool()).bind(o["allow_public_leaderboards"].as_bool()).bind(o["discord_invite_url"].as_str()).bind(o["ban_message"].as_str()).execute(&mut *tx).await?;
    audit::org_event(
        &mut tx,
        &actor,
        &id,
        &headers,
        action,
        o["name"].as_str().unwrap_or(""),
        changes,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(if action == "list.ban_message" {
        json!({"ok":true,"banMessage":o["ban_message"]})
    } else {
        json!({"ok":true})
    }))
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
    let actor = panel_user(&state, &headers, &Method::DELETE).await?;
    let mut tx = state.db.begin().await?;
    let o = locked_org(&mut tx, &actor, &id).await?;
    let gone: Vec<(String, String, String, i32)> = sqlx::query_as(
        "SELECT id,name,host,port FROM servers WHERE org_id=$1 ORDER BY id FOR UPDATE",
    )
    .bind(&id)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM server_grants WHERE server_id IN(SELECT id FROM servers WHERE org_id=$1)",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM org_invites WHERE org_id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    for (sid, n, host, port) in &gone {
        sqlx::query("INSERT INTO audit_log(actor_id,actor_name,org_id,server_id,server_name,category,action,target,detail,outcome) VALUES($1,$2,$3,$4,$5,'server','server.delete',$6,$7,'ok')").bind(&actor.id).bind(&actor.name).bind(&id).bind(sid).bind(n).bind(format!("{host}:{port}")).bind(json!({"orgId":id,"org":o["name"],"reason":"org.delete"})).execute(&mut *tx).await?;
    }
    audit::org_event(
        &mut tx,
        &actor,
        &id,
        &headers,
        "org.delete",
        o["name"].as_str().unwrap_or(""),
        json!({"orgId":id,"servers":gone.iter().map(|s|&s.0).collect::<Vec<_>>()}),
    )
    .await?;
    sqlx::query("DELETE FROM organizations WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn members(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = panel_user(&state, &headers, &Method::GET).await?;
    roles::require_org_owner(&state, &actor, &id).await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('userId',u.id,'username',coalesce(nullif(u.display_username,''),u.username,split_part(u.email,'@',1)),'name',coalesce(nullif(u.name,''),u.display_username,u.username,''),'image',u.image,'siteOwner',u.role='owner','disabled',coalesce(u.banned,false),'role',m.role,'joinedAt',m.created_at,'grants',coalesce((SELECT jsonb_agg(jsonb_build_object('serverId',s.id,'serverName',s.name,'roleId',r.id,'roleName',r.name) ORDER BY s.sort_order,s.name) FROM server_grants g JOIN servers s ON s.id=g.server_id JOIN org_roles r ON r.id=g.role_id WHERE g.user_id=u.id AND s.org_id=$1),'[]')) FROM org_members m JOIN \"user\" u ON u.id=m.user_id WHERE m.org_id=$1 ORDER BY m.role,u.username,u.name").bind(&id).fetch_all(&state.db).await?;
    Ok(Json(json!({"ok":true,"members":rows})))
}
async fn member(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
) -> Result<(String, String)> {
    sqlx::query_as("SELECT m.role,coalesce(u.username,u.name) FROM org_members m JOIN \"user\" u ON u.id=m.user_id WHERE m.org_id=$1 AND m.user_id=$2 FOR UPDATE OF m").bind(org).bind(user).fetch_optional(&mut **tx).await?.ok_or_else(ApiError::missing)
}
async fn keep_owner(tx: &mut Transaction<'_, Postgres>, org: &str, user: &str) -> Result<()> {
    let owners:Vec<String>=sqlx::query_scalar("SELECT user_id FROM org_members WHERE org_id=$1 AND role='owner' ORDER BY user_id FOR UPDATE").bind(org).fetch_all(&mut **tx).await?;
    if owners.len() <= 1 && owners.iter().any(|o| o == user) {
        return Err(ApiError::bad("This organisation needs at least one owner."));
    }
    Ok(())
}
pub async fn member_role(
    State(state): State<AppState>,
    Path((org, user)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let role = body["role"]
        .as_str()
        .filter(|r| ["owner", "member"].contains(r))
        .ok_or_else(|| ApiError::bad("role must be owner or member."))?;
    let actor = panel_user(&state, &headers, &Method::PATCH).await?;
    let mut tx = state.db.begin().await?;
    locked_org(&mut tx, &actor, &org).await?;
    let (old, label) = member(&mut tx, &org, &user).await?;
    if role != old {
        if role == "member" {
            keep_owner(&mut tx, &org, &user).await?
        }
        sqlx::query("UPDATE org_members SET role=$3 WHERE org_id=$1 AND user_id=$2")
            .bind(&org)
            .bind(&user)
            .bind(role)
            .execute(&mut *tx)
            .await?;
        let mut detail = json!({"orgId":org,"role":role});
        if role == "member" {
            for (k, v) in users::revoke_minted(&mut tx, &user, Some(&org))
                .await?
                .as_object()
                .unwrap()
            {
                detail[k] = v.clone()
            }
        }
        audit::org_event(
            &mut tx,
            &actor,
            &org,
            &headers,
            "org.member.role",
            &label,
            detail,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn remove_member(
    State(state): State<AppState>,
    Path((org, user)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = panel_user(&state, &headers, &Method::DELETE).await?;
    let mut tx = state.db.begin().await?;
    locked_org(&mut tx, &actor, &org).await?;
    let (_, label) = member(&mut tx, &org, &user).await?;
    keep_owner(&mut tx, &org, &user).await?;
    sqlx::query("DELETE FROM server_grants WHERE user_id=$1 AND server_id IN(SELECT id FROM servers WHERE org_id=$2)").bind(&user).bind(&org).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM org_members WHERE org_id=$1 AND user_id=$2")
        .bind(&org)
        .bind(&user)
        .execute(&mut *tx)
        .await?;
    let mut ended = users::revoke_minted(&mut tx, &user, Some(&org)).await?;
    ended["orgId"] = json!(org);
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "org.member.remove",
        &label,
        ended,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn member_grants(
    State(state): State<AppState>,
    Path((org, user)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = panel_user(&state, &headers, &Method::PUT).await?;
    let mut tx = state.db.begin().await?;
    locked_org(&mut tx, &actor, &org).await?;
    let (_, label) = member(&mut tx, &org, &user).await?;
    let grants = users::apply_grants(&mut tx, &actor, &user, Some(&org), &body["grants"]).await?;
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "org.member.grants",
        &label,
        json!({"orgId":org,"grants":grants}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"grants":grants})))
}
const INVITE_SELECT: &str = "SELECT jsonb_build_object('id',i.id,'label',i.label,'orgRole',i.org_role,'serverRoleId',i.server_role_id,'serverRoleName',r.name,'maxUses',i.max_uses,'uses',i.uses,'expiresAt',i.expires_at,'revokedAt',i.revoked_at,'createdAt',i.created_at,'token',i.token) FROM org_invites i LEFT JOIN org_roles r ON r.id=i.server_role_id";
fn invite_shape(state: &AppState, mut inv: Value) -> Value {
    let status = if !inv["revokedAt"].is_null() {
        "revoked"
    } else if date(&inv["expiresAt"]).is_some_and(|t| t <= chrono::Utc::now()) {
        "expired"
    } else if inv["maxUses"]
        .as_i64()
        .is_some_and(|n| inv["uses"].as_i64().unwrap_or(0) >= n)
    {
        "used"
    } else {
        "live"
    };
    inv["status"] = json!(status);
    inv["problem"] = match status {
        "revoked" => json!("This invite link has been revoked."),
        "expired" => json!("This invite link has expired."),
        "used" => json!("This invite link has been used up."),
        _ => Value::Null,
    };
    inv["url"] = json!(format!(
        "{}/join/{}",
        state.config.origin,
        inv["token"].as_str().unwrap_or("")
    ));
    inv.as_object_mut().unwrap().remove("token");
    inv
}
pub async fn invites(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = panel_user(&state, &headers, &Method::GET).await?;
    roles::require_org_owner(&state, &actor, &id).await?;
    let rows: Vec<Value> = sqlx::query_scalar(&format!(
        "{INVITE_SELECT} WHERE i.org_id=$1 ORDER BY i.created_at"
    ))
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        json!({"ok":true,"invites":rows.into_iter().map(|i|invite_shape(&state,i)).collect::<Vec<_>>()}),
    ))
}
pub async fn create_invite(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let actor = panel_user(&state, &headers, &Method::POST).await?;
    let mut tx = state.db.begin().await?;
    let o = locked_org(&mut tx, &actor, &id).await?;
    let iid = uuid::Uuid::new_v4().to_string();
    let token = crate::identity_crypto::random_ascii(32);
    let role = if body["orgRole"] == "owner" {
        "owner"
    } else {
        "member"
    };
    let sr = string(&body["serverRoleId"], 64);
    if !sr.is_empty() {
        let valid: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM org_roles WHERE id=$1 AND org_id=$2)")
                .bind(&sr)
                .bind(&id)
                .fetch_one(&mut *tx)
                .await?;
        if !valid {
            return Err(ApiError::missing());
        }
    }
    let max = integer(&body["maxUses"], 0, 0, 100000);
    let days = integer(&body["expiresDays"], 0, 0, 3650);
    let expiry = if days > 0 {
        Some(chrono::Utc::now() + chrono::Duration::days(days))
    } else {
        None
    };
    sqlx::query("INSERT INTO org_invites(id,org_id,token,label,org_role,server_role_id,max_uses,expires_at,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(&iid).bind(&id).bind(token).bind(string(&body["label"],60)).bind(role).bind(if sr.is_empty(){None}else{Some(sr.clone())}).bind(if max>0{Some(max as i32)}else{None}).bind(expiry).bind(&actor.id).execute(&mut *tx).await?;
    let inv: Value = sqlx::query_scalar(&format!("{INVITE_SELECT} WHERE i.id=$1"))
        .bind(&iid)
        .fetch_one(&mut *tx)
        .await?;
    audit::org_event(&mut tx,&actor,&id,&headers,"org.invite.create",o["name"].as_str().unwrap_or(""),json!({"orgId":id,"inviteId":iid,"orgRole":role,"serverRoleId":inv["serverRoleId"],"serverRole":inv["serverRoleName"],"maxUses":inv["maxUses"],"expiresAt":expiry})).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"ok":true,"invite":invite_shape(&state,inv)})),
    ))
}
pub async fn revoke_invite(
    State(state): State<AppState>,
    Path((org, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = panel_user(&state, &headers, &Method::DELETE).await?;
    let mut tx = state.db.begin().await?;
    let o = locked_org(&mut tx, &actor, &org).await?;
    if sqlx::query("UPDATE org_invites SET revoked_at=now() WHERE org_id=$1 AND id=$2")
        .bind(&org)
        .bind(&id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::missing());
    }
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "org.invite.revoke",
        o["name"].as_str().unwrap_or(""),
        json!({"orgId":org,"inviteId":id}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn invite_view(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<Value>> {
    if token.is_empty() || token.len() > 100 {
        return Err(ApiError::missing());
    }
    let row:Option<(Value,String,String,Option<chrono::DateTime<chrono::Utc>>)>=sqlx::query_as(&format!("SELECT q.inv,o.id,o.name,o.suspended_at FROM ({INVITE_SELECT} WHERE i.token=$1) AS q(inv) JOIN org_invites x ON x.token=$1 JOIN organizations o ON o.id=x.org_id")).bind(token).fetch_optional(&state.db).await?;
    let Some((inv, id, n, suspended)) = row else {
        return Err(ApiError::missing());
    };
    Ok(Json(
        json!({"ok":true,"invite":invite_shape(&state,inv),"org":{"id":id,"name":n,"suspended":suspended.is_some()}}),
    ))
}
pub async fn join(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = auth::authenticate_account(&state, &headers, &Method::POST).await?;
    let mut tx = state.db.begin().await?;
    users::account_lock(&mut tx).await?;
    let enabled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM \"user\" WHERE id=$1 AND (NOT coalesce(banned,false) OR coalesce(ban_expires<=now(),false)))").bind(&actor.id).fetch_one(&mut *tx).await?;
    if !enabled {
        return Err(ApiError::unauthorized());
    }
    let org: Option<String> = sqlx::query_scalar("SELECT org_id FROM org_invites WHERE token=$1")
        .bind(&token)
        .fetch_optional(&mut *tx)
        .await?;
    let org = org.ok_or_else(ApiError::missing)?;
    let (name, suspended): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT name,suspended_at FROM organizations WHERE id=$1 FOR UPDATE")
            .bind(&org)
            .fetch_one(&mut *tx)
            .await?;
    let existing: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM org_members WHERE org_id=$1 AND user_id=$2)",
    )
    .bind(&org)
    .bind(&actor.id)
    .fetch_one(&mut *tx)
    .await?;
    if existing {
        tx.commit().await?;
        return Ok(Json(json!({"ok":true,"orgId":org,"alreadyMember":true})));
    }
    if suspended.is_some() {
        return Err(ApiError::new(
            StatusCode::GONE,
            "invite",
            "This organisation is suspended.",
        ));
    }
    let inv:Value=sqlx::query_scalar("UPDATE org_invites SET uses=uses+1 WHERE token=$1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at>now()) AND (max_uses IS NULL OR uses<max_uses) RETURNING to_jsonb(org_invites)").bind(token).fetch_optional(&mut *tx).await?.ok_or_else(||ApiError::new(StatusCode::GONE,"invite","This invite link can no longer be used."))?;
    sqlx::query("INSERT INTO org_members(org_id,user_id,role,invite_id) VALUES($1,$2,$3,$4)")
        .bind(&org)
        .bind(&actor.id)
        .bind(inv["org_role"].as_str())
        .bind(inv["id"].as_str())
        .execute(&mut *tx)
        .await?;
    let granted=sqlx::query("INSERT INTO server_grants(server_id,user_id,role_id,granted_by) SELECT s.id,$2,r.id,$4 FROM servers s JOIN org_roles r ON r.org_id=s.org_id WHERE s.org_id=$1 AND r.id=$3 ON CONFLICT DO NOTHING").bind(&org).bind(&actor.id).bind(inv["server_role_id"].as_str()).bind(inv["created_by"].as_str()).execute(&mut *tx).await?.rows_affected();
    audit::org_event(&mut tx,&actor,&org,&headers,"org.join",&name,json!({"orgId":org,"inviteId":inv["id"],"orgRole":inv["org_role"],"serverRoleId":inv["server_role_id"],"servers":granted})).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"orgId":org,"alreadyMember":false})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_names_and_templates() {
        assert_eq!(slug(" 测试 Équipe! "), "e-quipe");
        assert_eq!(slug("123 Great!Clan"), "123-great-clan");
        assert_eq!(slug("中文"), "org");
        assert_eq!(
            discord_invite("discord.com/invite/AB-c").unwrap(),
            "https://discord.gg/AB-c"
        );
        assert!(discord_invite("https://evil.test/invite/abc").is_err());
        assert!(ban_template(&json!("{UNKNOWN}")).is_err());
        assert_eq!(
            ban_template(&json!("  {reason} \n {uid} ")).unwrap(),
            "{reason} {uid}"
        );
    }
}
