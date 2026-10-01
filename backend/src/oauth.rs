//! Steam OpenID and Discord OAuth callbacks. No provider email is used for account linking.
use crate::{
    auth,
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, Peer},
    identity, identity_crypto as crypto, identity_signup,
};
use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
};
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;
use subtle::ConstantTimeEq;

const STEAM: &str = "https://steamcommunity.com/openid/login";
const NS: &str = "http://specs.openid.net/auth/2.0";
fn provider_valid(provider: &str) -> Result<()> {
    if !["steam", "discord"].contains(&provider) {
        return Err(ApiError::missing());
    }
    Ok(())
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "provider_unavailable",
                "Sign-in provider is unavailable.",
            )
        })
}
async fn bounded(mut response: reqwest::Response) -> Result<Vec<u8>> {
    if !response.status().is_success() {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "provider_rejected",
            "Sign-in provider rejected the request.",
        ));
    }
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::bad("Provider response is incomplete."))?
    {
        if data.len() + chunk.len() > 65536 {
            return Err(ApiError::bad("Provider response is too large."));
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
fn provider_error() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "provider_rejected",
        "Sign-in could not be verified.",
    )
}
pub fn steam_fields(
    query: &str,
    expected_return: &str,
) -> Result<(String, HashMap<String, String>)> {
    let mut fields = HashMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key.starts_with("openid.")
            && fields
                .insert(key.into_owned(), value.into_owned())
                .is_some()
        {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "steam_duplicate",
                "Steam sign-in answer repeats a field.",
            ));
        }
    }
    let get = |name: &str| fields.get(name).map(String::as_str).unwrap_or("");
    if get("openid.mode") != "id_res"
        || get("openid.ns") != NS
        || get("openid.return_to") != expected_return
    {
        return Err(provider_error());
    }
    let claimed = get("openid.claimed_id");
    let id = claimed
        .strip_prefix("https://steamcommunity.com/openid/id/")
        .or_else(|| claimed.strip_prefix("http://steamcommunity.com/openid/id/"))
        .ok_or_else(provider_error)?;
    if id.len() != 17
        || !id.bytes().all(|b| b.is_ascii_digit())
        || get("openid.identity") != claimed
    {
        return Err(provider_error());
    }
    let signed: Vec<_> = get("openid.signed").split(',').collect();
    if [
        "claimed_id",
        "identity",
        "return_to",
        "response_nonce",
        "assoc_handle",
    ]
    .iter()
    .any(|f| !signed.contains(f))
    {
        return Err(provider_error());
    }
    if get("openid.response_nonce").is_empty() || get("openid.assoc_handle").is_empty() {
        return Err(provider_error());
    }
    let id = id.to_owned();
    fields.insert("openid.mode".into(), "check_authentication".into());
    Ok((id, fields))
}
fn redirect(state: &AppState, path: &str, cookies: Vec<String>) -> Result<Response> {
    let mut response = identity::reply(json!({"ok":true}), cookies)?;
    *response.status_mut() = StatusCode::SEE_OTHER;
    let location = if path.starts_with("https://steamcommunity.com/openid/login?")
        || path.starts_with("https://discord.com/oauth2/authorize?")
    {
        path.to_owned()
    } else {
        format!(
            "{}{}",
            state.config.origin,
            crypto::safe_path(path, "/sign-in")
        )
    };
    response.headers_mut().insert(
        "location",
        HeaderValue::from_str(&location).map_err(|_| provider_error())?,
    );
    Ok(response)
}
pub async fn begin(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    identity::origin(&state, &headers)?;
    provider_valid(&provider)?;
    crate::ratelimit::allow(
        format!(
            "oauth:{}:{}",
            provider,
            identity::peer_key(&state, peer, &headers)
        ),
        30,
        true,
    )?;
    if provider == "discord"
        && (state.config.identity.discord_id.is_none()
            || state.config.identity.discord_secret.is_none())
    {
        return Err(ApiError::missing());
    }
    let link = body["mode"].as_str() == Some("link");
    let actor = if link {
        Some(auth::authenticate_account(&state, &headers, &Method::POST).await?)
    } else {
        None
    };
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM \"user\")")
        .fetch_one(&state.db)
        .await?;
    if !exists {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "not_set_up",
            "Open /setup first.",
        ));
    }
    let invite = body["invite"].as_str().unwrap_or("");
    if !invite.is_empty() {
        let mut tx = state.db.begin().await?;
        identity_signup::invite_allowed(&mut tx, invite).await?;
        tx.rollback().await?;
    }
    let signup = !link && (state.config.identity.allow_signup || !invite.is_empty());
    let nonce = crypto::random_token(32);
    let callback = if provider == "steam" {
        format!("{}/auth/steam/callback?state={nonce}", state.config.origin)
    } else {
        format!("{}/api/auth/callback/discord", state.config.origin)
    };
    let next = crypto::safe_path(
        body["next"].as_str().unwrap_or(""),
        if link { "/account" } else { "/" },
    );
    let back = crypto::safe_path(
        body["back"].as_str().unwrap_or(""),
        if link { "/account" } else { "/sign-in" },
    );
    let stored = json!({"linkUser":actor.as_ref().map(|a|a.id.as_str()),"link":link,"signup":signup,"invite":invite,"next":next,"back":back,"callback":callback});
    sqlx::query("INSERT INTO verification(id,identifier,value,expires_at) VALUES($1,$2,$3,now()+interval '10 minutes')").bind(uuid::Uuid::new_v4().to_string()).bind(format!("rust-oauth-{provider}-{nonce}")).bind(stored.to_string()).execute(&state.db).await?;
    let cookie = identity::cookie(
        &state,
        &provider,
        &crypto::signed(&state.config.auth_secret, &nonce),
        600,
    );
    let mut url = url::Url::parse(if provider == "steam" {
        STEAM
    } else {
        "https://discord.com/oauth2/authorize"
    })
    .unwrap();
    if provider == "steam" {
        let select = "http://specs.openid.net/auth/2.0/identifier_select";
        url.query_pairs_mut().extend_pairs([
            ("openid.ns", NS),
            ("openid.mode", "checkid_setup"),
            ("openid.identity", select),
            ("openid.claimed_id", select),
            ("openid.return_to", &callback),
            ("openid.realm", &state.config.origin),
        ]);
    } else {
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            (
                "client_id",
                state.config.identity.discord_id.as_deref().unwrap(),
            ),
            ("redirect_uri", &callback),
            ("scope", "identify"),
            ("state", &nonce),
        ]);
    }
    // Page actions perform the navigation; using JSON avoids a fetch automatically following it.
    identity::reply(json!({"ok":true,"url":url.as_str()}), vec![cookie])
}
async fn consume(
    state: &AppState,
    headers: &HeaderMap,
    provider: &str,
    query: &str,
) -> Result<Value> {
    let pairs: Vec<_> = url::form_urlencoded::parse(query.as_bytes())
        .filter(|(k, _)| k == "state")
        .collect();
    if pairs.len() != 1 {
        return Err(provider_error());
    }
    let nonce = crypto::read_signed(headers, &state.config.auth_secret, provider)
        .ok_or_else(provider_error)?;
    if !bool::from(nonce.as_bytes().ct_eq(pairs[0].1.as_bytes())) {
        return Err(provider_error());
    }
    let value: Option<String> = sqlx::query_scalar(
        "DELETE FROM verification WHERE identifier=$1 AND expires_at>now() RETURNING value",
    )
    .bind(format!("rust-oauth-{provider}-{nonce}"))
    .fetch_optional(&state.db)
    .await?;
    serde_json::from_str(&value.ok_or_else(provider_error)?).map_err(|_| provider_error())
}
pub async fn steam_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response> {
    callback(state, headers, "steam", query.unwrap_or_default()).await
}
pub async fn discord_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response> {
    callback(state, headers, "discord", query.unwrap_or_default()).await
}
async fn callback(
    state: AppState,
    headers: HeaderMap,
    provider: &str,
    query: String,
) -> Result<Response> {
    let clear = identity::cookie(&state, provider, "", 0);
    let stored = match consume(&state, &headers, provider, &query).await {
        Ok(v) => v,
        Err(_) => {
            return redirect(
                &state,
                &format!("/sign-in?error={provider}_state"),
                vec![clear],
            );
        }
    };
    let back = stored["back"].as_str().unwrap_or("/sign-in");
    let result = finish(&state, &headers, provider, &query, &stored).await;
    match result {
        Ok(cookie) => redirect(
            &state,
            stored["next"].as_str().unwrap_or("/"),
            if let Some(cookie) = cookie {
                vec![clear, cookie]
            } else {
                vec![clear]
            },
        ),
        Err(error) => {
            let mut tx = state.db.begin().await?;
            identity::audit(
                &mut tx,
                None,
                &headers,
                if stored["link"].as_bool() == Some(true) {
                    "account.link"
                } else {
                    "login"
                },
                provider,
                "denied",
            )
            .await?;
            tx.commit().await?;
            redirect(
                &state,
                &format!(
                    "{back}{}error={provider}_{}",
                    if back.contains('?') { "&" } else { "?" },
                    error.code
                ),
                vec![clear],
            )
        }
    }
}
async fn free_username(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    wanted: &str,
) -> Result<String> {
    let base = wanted
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c))
        .collect::<String>();
    let base = base.trim_start_matches(['.', '_', '-']);
    let base = if base.len() < 2 {
        "user"
    } else {
        &base[..base.len().min(32)]
    };
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT lower(username) FROM \"user\" WHERE username IS NOT NULL")
            .fetch_all(&mut **tx)
            .await?;
    for i in 1..=1000 {
        let suffix = if i == 1 { String::new() } else { i.to_string() };
        let candidate = format!("{}{suffix}", &base[..base.len().min(32 - suffix.len())]);
        if !rows.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Ok(format!(
        "{}{}",
        &base[..base.len().min(15)],
        &crypto::random_ascii(12).to_ascii_lowercase()
    ))
}
async fn finish(
    state: &AppState,
    headers: &HeaderMap,
    provider: &str,
    query: &str,
    stored: &Value,
) -> Result<Option<String>> {
    let callback = stored["callback"].as_str().ok_or_else(provider_error)?;
    let (account_id, name, image) = if provider == "steam" {
        let (id, fields) = steam_fields(query, callback)?;
        let response = client()?
            .post(STEAM)
            .form(&fields)
            .send()
            .await
            .map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "unreachable",
                    "Steam could not be reached.",
                )
            })?;
        let bytes = bounded(response).await?;
        let text = std::str::from_utf8(&bytes).map_err(|_| provider_error())?;
        if !text.lines().any(|line| {
            line.split_once(':')
                .is_some_and(|(k, v)| k.trim() == "is_valid" && v.trim() == "true")
        }) {
            return Err(provider_error());
        }
        let row = sqlx::query("SELECT persona,avatar FROM steam_profiles WHERE steam_id=$1")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?;
        let name = row
            .as_ref()
            .and_then(|r| r.try_get::<Option<String>, _>("persona").ok().flatten())
            .unwrap_or_else(|| format!("steam{}", &id[11..]));
        let image = row.and_then(|r| r.try_get::<Option<String>, _>("avatar").ok().flatten());
        (id, name, image)
    } else {
        let codes: Vec<_> = url::form_urlencoded::parse(query.as_bytes())
            .filter(|(k, _)| k == "code")
            .collect();
        if codes.len() != 1 || codes[0].1.is_empty() || codes[0].1.len() > 4096 {
            return Err(provider_error());
        }
        let fields = [
            ("grant_type", "authorization_code"),
            ("code", codes[0].1.as_ref()),
            ("redirect_uri", callback),
            (
                "client_id",
                state
                    .config
                    .identity
                    .discord_id
                    .as_deref()
                    .ok_or_else(provider_error)?,
            ),
            (
                "client_secret",
                state
                    .config
                    .identity
                    .discord_secret
                    .as_deref()
                    .ok_or_else(provider_error)?,
            ),
        ];
        let response = client()?
            .post("https://discord.com/api/oauth2/token")
            .form(&fields)
            .send()
            .await
            .map_err(|_| provider_error())?;
        let tokens: Value =
            serde_json::from_slice(&bounded(response).await?).map_err(|_| provider_error())?;
        let token = tokens["access_token"].as_str().ok_or_else(provider_error)?;
        if token.len() > 4096
            || tokens["token_type"]
                .as_str()
                .is_none_or(|s| !s.eq_ignore_ascii_case("bearer"))
        {
            return Err(provider_error());
        }
        let response = client()?
            .get("https://discord.com/api/v10/users/@me")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| provider_error())?;
        let profile: Value =
            serde_json::from_slice(&bounded(response).await?).map_err(|_| provider_error())?;
        let id = profile["id"].as_str().ok_or_else(provider_error)?;
        if !(1..=32).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(provider_error());
        }
        let name = profile["global_name"]
            .as_str()
            .or_else(|| profile["username"].as_str())
            .unwrap_or("discord")
            .to_owned();
        let image = profile["avatar"]
            .as_str()
            .filter(|s| s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .map(|s| format!("https://cdn.discordapp.com/avatars/{id}/{s}.png?size=128"));
        (id.to_owned(), name, image)
    };
    let link = stored["link"].as_bool() == Some(true);
    let actor = if link {
        Some(auth::authenticate_account(state, headers, &Method::GET).await?)
    } else {
        None
    };
    if actor
        .as_ref()
        .is_some_and(|a| Some(a.id.as_str()) != stored["linkUser"].as_str())
    {
        return Err(provider_error());
    }
    let mut tx = state.db.begin().await?;
    // Provider identity and user creation are serialized in the same order as setup/deletion.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('identity:accounts'))")
        .execute(&mut *tx)
        .await?;
    let linked: Option<String> =
        sqlx::query_scalar("SELECT user_id FROM account WHERE provider_id=$1 AND account_id=$2")
            .bind(provider)
            .bind(&account_id)
            .fetch_optional(&mut *tx)
            .await?;
    let id = if let Some(actor) = &actor {
        if linked.as_ref().is_some_and(|id| id != &actor.id) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "taken",
                "Provider account is already linked.",
            ));
        }
        // A concurrent unlink cannot overwrite another login path.
        sqlx::query("SELECT id FROM \"user\" WHERE id=$1 FOR UPDATE")
            .bind(&actor.id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(provider_error)?;
        actor.id.clone()
    } else if let Some(id) = &linked {
        id.clone()
    } else {
        if stored["signup"].as_bool() != Some(true) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "unknown",
                "Provider account is not linked. Sign up through an invite.",
            ));
        }
        identity_signup::final_guard(
            state,
            &mut tx,
            false,
            stored["invite"].as_str().unwrap_or(""),
        )
        .await?;
        let username = free_username(&mut tx, &name).await?;
        let id = identity::create_user(&mut tx, &username, &name, "member").await?;
        sqlx::query("UPDATE \"user\" SET image=$2 WHERE id=$1")
            .bind(&id)
            .bind(image)
            .execute(&mut *tx)
            .await?;
        id
    };
    if linked.is_none() {
        sqlx::query(
            "INSERT INTO account(id,account_id,provider_id,user_id,issuer) VALUES($1,$2,$3,$4,$3)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&account_id)
        .bind(provider)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    }
    if provider == "steam" {
        sqlx::query("UPDATE \"user\" SET steam_id=$2 WHERE id=$1 AND NOT EXISTS(SELECT 1 FROM \"user\" WHERE steam_id=$2 AND id<>$1)").bind(&id).bind(account_id).execute(&mut *tx).await?;
    }
    identity::refresh(&mut tx, &id).await?;
    let cookie = if link {
        None
    } else {
        Some(identity::new_session(state, &mut tx, &id, headers).await?)
    };
    identity::audit(
        &mut tx,
        actor.as_ref(),
        headers,
        if link { "account.link" } else { "login" },
        provider,
        "ok",
    )
    .await?;
    tx.commit().await?;
    Ok(cookie)
}
