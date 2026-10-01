use crate::{
    config::AppState,
    crypto::{decode_base64, hash_token, parse_bearer},
    error::{ApiError, Result},
};
use axum::http::{HeaderMap, Method, StatusCode};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use sqlx::Row;

pub const CAPS: &[&str] = &[
    "server.view",
    "chat.send",
    "players.moderate",
    "match.control",
    "rotation.edit",
    "players.notes",
    "rotation.save",
    "players.notes.manage",
    "integrity.view",
    "bans.manage",
    "slots.manage",
    "lists.ban",
    "lists.reserve",
    "config.apply",
    "automation.manage",
    "audit.read",
    "rcon.raw",
];

#[derive(Clone, Debug)]
pub struct Actor {
    pub id: String,
    pub name: String,
    pub owner: bool,
    pub key: Option<KeyScope>,
}
#[derive(Clone, Debug)]
pub struct KeyScope {
    pub org_id: String,
    pub caps: Vec<String>,
    pub server_ids: Option<Vec<String>>,
}
#[derive(Clone, Debug)]
pub struct ServerScope {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub caps: Vec<String>,
    pub manager: bool,
}
fn field<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

pub fn session_cookie(headers: &HeaderMap, secret: &str) -> Option<String> {
    let cookies = field(headers, "cookie")?;
    for raw in cookies.split(';') {
        let Some((name, value)) = raw.trim().split_once('=') else {
            continue;
        };
        if !["warcon.session_token", "__Secure-warcon.session_token"].contains(&name) {
            continue;
        }
        let decoded = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .ok()?;
        let (token, signature) = decoded.rsplit_once('.')?;
        if token.is_empty() {
            return None;
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(token.as_bytes());
        mac.verify_slice(&decode_base64(signature).ok()?).ok()?;
        return Some(token.to_owned());
    }
    None
}
pub async fn authenticate(state: &AppState, headers: &HeaderMap, method: &Method) -> Result<Actor> {
    if let Some(header) = field(headers, "authorization") {
        let token = parse_bearer(header).ok_or_else(ApiError::unauthorized)?;
        let row=sqlx::query("SELECT k.id,k.label,k.org_id,k.capabilities,k.server_ids,k.expires_at,k.revoked_at,o.suspended_at FROM api_keys k JOIN organizations o ON o.id=k.org_id WHERE k.key_hash=$1")
            .bind(hash_token(token)).fetch_optional(&state.db).await?.ok_or_else(ApiError::unauthorized)?;
        let revoked: Option<DateTime<Utc>> = row.try_get("revoked_at")?;
        let expires: Option<DateTime<Utc>> = row.try_get("expires_at")?;
        let suspended: Option<DateTime<Utc>> = row.try_get("suspended_at")?;
        if revoked.is_some() {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "api_key_revoked",
                "This API key has been revoked.",
            ));
        }
        if expires.is_some_and(|t| t <= Utc::now()) {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "api_key_expired",
                "This API key has expired.",
            ));
        }
        if suspended.is_some() {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "suspended",
                "This organisation is suspended.",
            ));
        }
        let caps: Value = row.try_get("capabilities")?;
        let ids: Option<Value> = row.try_get("server_ids")?;
        let scope = KeyScope {
            org_id: row.try_get("org_id")?,
            caps: known_capabilities(&caps),
            server_ids: ids.map(|v| json_strings(&v)),
        };
        let id: String = row.try_get("id")?;
        // Conditional update works across API processes; failure must not reject an otherwise valid key.
        let pool = state.db.clone();
        let touch_id = id.clone();
        tokio::spawn(async move {
            let _=sqlx::query("UPDATE api_keys SET last_used_at=now() WHERE id=$1 AND (last_used_at IS NULL OR last_used_at<=now()-interval '60 seconds')").bind(touch_id).execute(&pool).await;
        });
        return Ok(Actor {
            id: format!("key:{id}"),
            name: format!("{} (API key)", row.try_get::<String, _>("label")?),
            owner: false,
            key: Some(scope),
        });
    }
    // A mutation authenticated by cookies must originate from the configured panel.
    if ![Method::GET, Method::HEAD, Method::OPTIONS].contains(method)
        && field(headers, "origin") != Some(state.config.origin.as_str())
    {
        return Err(ApiError::forbidden());
    }
    let token =
        session_cookie(headers, &state.config.auth_secret).ok_or_else(ApiError::unauthorized)?;
    let row=sqlx::query("SELECT u.id,coalesce(u.username,u.name) AS name,u.role,u.must_change_password,u.auth_complete,u.auth_grace_started_at,u.banned,u.ban_expires FROM session s JOIN \"user\" u ON u.id=s.user_id WHERE s.token=$1 AND s.expires_at>now()")
        .bind(token).fetch_optional(&state.db).await?.ok_or_else(ApiError::unauthorized)?;
    let banned: Option<bool> = row.try_get("banned")?;
    let ban_expires: Option<DateTime<Utc>> = row.try_get("ban_expires")?;
    if banned.unwrap_or(false) && ban_expires.is_none_or(|e| e > Utc::now()) {
        return Err(ApiError::unauthorized());
    }
    if row.try_get::<bool, _>("must_change_password")? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "password_change_required",
            "Change password first.",
        ));
    }
    if !row.try_get::<bool, _>("auth_complete")? && enrolment_due(&state.db, &row).await? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "enrolment_required",
            "Complete sign-in requirements first.",
        ));
    }
    Ok(Actor {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        owner: row.try_get::<Option<String>, _>("role")?.as_deref() == Some("owner"),
        key: None,
    })
}
async fn enrolment_due(db: &sqlx::PgPool, user: &sqlx::postgres::PgRow) -> Result<bool> {
    let settings = crate::settings::load(db).await?;
    let setting = |name: &str, default: i64, max: i64| {
        settings
            .get(name)
            .and_then(Value::as_f64)
            .map(|n| n as i64)
            .unwrap_or(default)
            .clamp(0, max)
    };
    let mode = setting("authEnforce", 0, 2);
    if mode == 0 {
        return Ok(false);
    }
    let owner = user.try_get::<Option<String>, _>("role")?.as_deref() == Some("owner");
    let id: String = user.try_get("id")?;
    if mode == 1 && !owner {
        let privileged:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM org_members WHERE user_id=$1 AND role='owner') OR EXISTS(SELECT 1 FROM server_grants g JOIN org_roles r ON r.id=g.role_id WHERE g.user_id=$1 AND r.capabilities ?| ARRAY['bans.manage','slots.manage','lists.ban','lists.reserve','config.apply','automation.manage','rcon.raw','rotation.save','players.notes.manage'])").bind(&id).fetch_one(db).await?;
        if !privileged {
            return Ok(false);
        }
    }
    let started: Option<DateTime<Utc>> = user.try_get("auth_grace_started_at")?;
    let days = if owner {
        setting("authGraceDays", 14, 365)
    } else {
        setting("authMemberGraceDays", 30, 365)
    };
    Ok(started.is_some_and(|time| time + chrono::Duration::days(days) <= Utc::now()))
}
pub fn json_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
pub fn known_capabilities(value: &Value) -> Vec<String> {
    CAPS.iter()
        .filter(|cap| {
            value
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(**cap)))
        })
        .map(|s| (*s).to_owned())
        .collect()
}
pub fn parse_capabilities(value: &Value) -> Result<Vec<String>> {
    let values = value
        .as_array()
        .ok_or_else(|| ApiError::bad("capabilities must be a list."))?;
    if values
        .iter()
        .any(|v| v.as_str().is_none_or(|s| !CAPS.contains(&s)))
    {
        return Err(ApiError::bad("Unknown capability."));
    }
    Ok(known_capabilities(value))
}
pub async fn server_scope(
    state: &AppState,
    actor: &Actor,
    id: &str,
    cap: &str,
) -> Result<ServerScope> {
    let row=sqlx::query("SELECT s.id,s.org_id,s.name,o.suspended_at FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1").bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    let org_id: String = row.try_get("org_id")?;
    if !actor.owner
        && row
            .try_get::<Option<DateTime<Utc>>, _>("suspended_at")?
            .is_some()
    {
        return Err(ApiError::forbidden());
    }
    let (caps, manager) = if let Some(key) = &actor.key {
        if !key.caps.iter().any(|c| c == "server.view")
            || key.org_id != org_id
            || key
                .server_ids
                .as_ref()
                .is_some_and(|ids| !ids.iter().any(|s| s == id))
        {
            return Err(ApiError::missing());
        }
        (key.caps.clone(), false)
    } else if actor.owner {
        (CAPS.iter().map(|c| (*c).to_string()).collect(), true)
    } else {
        let role: Option<String> =
            sqlx::query_scalar("SELECT role FROM org_members WHERE org_id=$1 AND user_id=$2")
                .bind(&org_id)
                .bind(&actor.id)
                .fetch_optional(&state.db)
                .await?;
        if role.as_deref() == Some("owner") {
            (CAPS.iter().map(|c| (*c).to_string()).collect(), true)
        } else {
            let caps:Option<Value>=sqlx::query_scalar("SELECT r.capabilities FROM server_grants g JOIN org_roles r ON r.id=g.role_id AND r.org_id=$3 JOIN org_members m ON m.org_id=r.org_id AND m.user_id=g.user_id WHERE g.server_id=$1 AND g.user_id=$2")
                .bind(id).bind(&actor.id).bind(&org_id).fetch_optional(&state.db).await?;
            (
                known_capabilities(&caps.ok_or_else(ApiError::missing)?),
                false,
            )
        }
    };
    if !caps.iter().any(|c| c == cap) {
        return Err(ApiError::forbidden());
    }
    Ok(ServerScope {
        id: id.into(),
        org_id,
        name: row.try_get("name")?,
        caps,
        manager,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    #[test]
    fn signed_cookie_and_forgery() {
        let secret = "secret";
        let token = "session-token";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(token.as_bytes());
        let value = format!(
            "warcon.session_token={token}.{}",
            STANDARD.encode(mac.finalize().into_bytes())
        );
        let mut headers = HeaderMap::new();
        headers.insert("cookie", value.parse().unwrap());
        assert_eq!(session_cookie(&headers, secret).as_deref(), Some(token));
        assert!(session_cookie(&headers, "wrong").is_none());
    }
}
