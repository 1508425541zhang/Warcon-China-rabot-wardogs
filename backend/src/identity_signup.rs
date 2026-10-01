use crate::{
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, Peer},
    identity, identity_crypto as crypto,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};

pub async fn invite_allowed(tx: &mut Transaction<'_, Postgres>, token: &str) -> Result<()> {
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM org_invites i JOIN organizations o ON o.id=i.org_id WHERE i.token=$1 AND i.revoked_at IS NULL AND (i.expires_at IS NULL OR i.expires_at>now()) AND (i.max_uses IS NULL OR i.uses<i.max_uses) AND o.suspended_at IS NULL)").bind(token).fetch_one(&mut **tx).await?;
    if !valid {
        return Err(ApiError::new(
            StatusCode::GONE,
            "invite_expired",
            "This invite link can no longer be used.",
        ));
    }
    Ok(())
}
pub async fn turnstile(state: &AppState, token: &str) -> Result<()> {
    if state.config.identity.turnstile_key.is_none() {
        return Ok(());
    }
    let secret = state
        .config
        .identity
        .turnstile_secret
        .as_deref()
        .ok_or_else(ApiError::forbidden)?;
    if token.is_empty() || token.len() > 4000 {
        return Err(ApiError::bad("Please complete the verification challenge."));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ApiError::forbidden())?;
    let response = client
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .form(&[("secret", secret), ("response", token)])
        .send()
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "verification_unavailable",
                "Could not verify the challenge right now.",
            )
        })?;
    let data: Value = response
        .error_for_status()
        .map_err(|_| ApiError::forbidden())?
        .json()
        .await
        .map_err(|_| ApiError::forbidden())?;
    if data["success"].as_bool() != Some(true) {
        return Err(ApiError::forbidden());
    }
    Ok(())
}
/// Shared by password and Passkey sign-up. The final transaction rechecks these guards.
pub async fn authorize_signup(
    state: &AppState,
    body: &Value,
    peer: Peer,
    headers: &HeaderMap,
    allow_setup: bool,
) -> Result<bool> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM \"user\")")
        .fetch_one(&state.db)
        .await?;
    if !exists {
        if !allow_setup {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "not_set_up",
                "Open /setup first.",
            ));
        }
        if let Some(token) = &state.config.identity.setup_token {
            use subtle::ConstantTimeEq;
            if !bool::from(
                token
                    .as_bytes()
                    .ct_eq(body["token"].as_str().unwrap_or("").as_bytes()),
            ) {
                return Err(ApiError::forbidden());
            }
        }
        return Ok(true);
    }
    let invite = body["invite"].as_str().unwrap_or("");
    if invite.is_empty() && !state.config.identity.allow_signup {
        return Err(ApiError::missing());
    }
    if !invite.is_empty() {
        let mut tx = state.db.begin().await?;
        invite_allowed(&mut tx, invite).await?;
        tx.rollback().await?;
    }
    let keys = vec![format!(
        "signup:{}",
        identity::peer_key(state, peer, headers)
    )];
    let lock = identity::lock_seconds(&state.db, &keys).await?;
    if lock > 0 {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "signup_locked",
            "Too many sign-up attempts. Try again later.",
        ));
    }
    identity::note_failures(&state.db, &keys).await?;
    turnstile(
        state,
        body["turnstile"]
            .as_str()
            .or_else(|| body["cf-turnstile-response"].as_str())
            .unwrap_or(""),
    )
    .await?;
    Ok(false)
}
pub async fn final_guard(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    setup: bool,
    invite: &str,
) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('identity:accounts'))")
        .execute(&mut **tx)
        .await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM \"user\")")
        .fetch_one(&mut **tx)
        .await?;
    if exists == setup {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "registration_state_changed",
            "Registration is no longer available. Reload the page.",
        ));
    }
    if !setup {
        if !invite.is_empty() {
            invite_allowed(tx, invite).await?;
        } else if !state.config.identity.allow_signup {
            return Err(ApiError::missing());
        }
    }
    Ok(())
}
pub async fn register(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    identity::origin(&state, &headers)?;
    authorize_signup(&state, &body, peer, &headers, false).await?;
    let username = crypto::username_valid(body["username"].as_str().unwrap_or(""))?;
    let password = body["password"].as_str().unwrap_or("");
    let hash = crypto::hash_password(password.into()).await?;
    let mut tx = state.db.begin().await?;
    final_guard(
        &state,
        &mut tx,
        false,
        body["invite"].as_str().unwrap_or(""),
    )
    .await?;
    let id = identity::create_user(
        &mut tx,
        &username,
        body["displayName"].as_str().unwrap_or(""),
        "member",
    )
    .await?;
    sqlx::query("INSERT INTO account(id,account_id,provider_id,user_id,password) VALUES($1,$2,'credential',$2,$3)").bind(uuid::Uuid::new_v4().to_string()).bind(&id).bind(hash).execute(&mut *tx).await?;
    let cookie = identity::new_session(&state, &mut tx, &id, &headers).await?;
    identity::audit(&mut tx, None, &headers, "signup", &username, "ok").await?;
    tx.commit().await?;
    identity::reply(
        json!({"ok":true,"redirect":crypto::safe_path(body["next"].as_str().unwrap_or(""),"/")}),
        vec![cookie],
    )
}
