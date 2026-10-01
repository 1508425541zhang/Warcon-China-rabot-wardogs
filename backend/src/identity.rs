//! Native identity business. Authentication secrets never appear in JSON responses or logs.
use crate::http::{ApiJson, Peer};
use crate::{
    auth::{self, Actor},
    config::AppState,
    error::{ApiError, Result},
    identity_crypto as crypto,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::{PgPool, Postgres, Row, Transaction};
use subtle::ConstantTimeEq;

pub fn origin(state: &AppState, headers: &HeaderMap) -> Result<()> {
    if headers.get("origin").and_then(|h| h.to_str().ok()) != Some(state.config.origin.as_str())
        || headers.contains_key("authorization")
    {
        return Err(ApiError::forbidden());
    }
    Ok(())
}
fn ua(headers: &HeaderMap) -> String {
    crate::feed::truncate(
        headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .unwrap_or(""),
        300,
    )
}
pub fn peer_key(state: &AppState, peer: Peer, headers: &HeaderMap) -> String {
    let Some(address) = crate::http::client_address(state, peer, headers) else {
        return "unknown".into();
    };
    let mut mac =
        Hmac::<Sha256>::new_from_slice(state.config.auth_secret.as_bytes()).expect("HMAC");
    mac.update(address.to_string().as_bytes());
    hex::encode(mac.finalize().into_bytes())[..32].into()
}
pub fn cookie(state: &AppState, suffix: &str, value: &str, age: i64) -> String {
    let secure = state.config.origin.starts_with("https://");
    let name = format!("{}warcon.{suffix}", if secure { "__Secure-" } else { "" });
    let encoded = percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC);
    format!(
        "{name}={encoded}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{}",
        if secure { "; Secure" } else { "" }
    )
}
pub fn reply(value: Value, cookies: Vec<String>) -> Result<Response> {
    let mut response = Json(value).into_response();
    for value in cookies {
        response.headers_mut().append(
            "set-cookie",
            HeaderValue::from_str(&value).map_err(|_| ApiError::bad("Invalid cookie."))?,
        );
    }
    Ok(response)
}
fn input<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn disabled(row: &sqlx::postgres::PgRow) -> Result<bool> {
    Ok(row.try_get::<Option<bool>, _>("banned")?.unwrap_or(false)
        && row
            .try_get::<Option<DateTime<Utc>>, _>("ban_expires")?
            .is_none_or(|t| t > Utc::now()))
}
pub async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    user: Option<&Actor>,
    headers: &HeaderMap,
    action: &str,
    target: &str,
    outcome: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,category,action,target,outcome,user_agent) VALUES($1,$2,'auth',$3,$4,$5,$6)")
        .bind(user.map(|u|u.id.as_str()).unwrap_or("")).bind(user.map(|u|u.name.as_str()).unwrap_or(""))
        .bind(action).bind(target).bind(outcome).bind(ua(headers)).execute(&mut **tx).await?;
    Ok(())
}
pub async fn methods(db: &PgPool, id: &str) -> Result<Value> {
    let providers: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT provider_id FROM account WHERE user_id=$1 ORDER BY provider_id",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    let row=sqlx::query("SELECT coalesce(two_factor_enabled,false) AS factor,recovery_key_hash IS NOT NULL AS recovery,(SELECT count(*) FROM passkey WHERE user_id=u.id)::int AS passkeys FROM \"user\" u WHERE id=$1").bind(id).fetch_one(db).await?;
    Ok(
        json!({"password":providers.iter().any(|p|p=="credential"),"twoFactor":row.try_get::<bool,_>("factor")?,"providers":providers,"passkeys":row.try_get::<i32,_>("passkeys")?,"recoveryKey":row.try_get::<bool,_>("recovery")?}),
    )
}
pub fn enrolment(methods: &Value, owner: bool) -> Value {
    let providers = methods["providers"].as_array();
    let sso = ["discord", "steam"]
        .iter()
        .filter(|p| providers.is_some_and(|a| a.iter().any(|v| v.as_str() == Some(**p))))
        .count();
    let password = methods["password"].as_bool() == Some(true);
    let factor = methods["twoFactor"].as_bool() == Some(true);
    let recovery = methods["recoveryKey"].as_bool() == Some(true);
    let ways = methods["passkeys"].as_u64().unwrap_or(0)
        + sso as u64
        + u64::from(password && factor)
        + u64::from(recovery);
    let mut problems = Vec::new();
    if password && !factor {
        problems.push("Your password has no second factor. Turn on an authenticator app, or add a passkey and remove the password.");
    }
    if ways < 2 {
        problems.push(if ways==0 {"You have no safe way in yet. Add a passkey or link Discord or Steam, then add a second one."}else{"You have only one way in. Add a second: another passkey, a linked Discord or Steam account, or a recovery key."});
    }
    if owner && sso == 0 && !recovery {
        problems.push("Owners cannot be reset by anyone else, so link Discord or Steam or save a recovery key.");
    }
    json!({"waysIn":ways,"complete":problems.is_empty(),"problems":problems})
}
pub async fn refresh(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<()> {
    sqlx::query("UPDATE \"user\" u SET auth_complete=(NOT EXISTS(SELECT 1 FROM account WHERE user_id=u.id AND provider_id='credential') OR coalesce(u.two_factor_enabled,false)) AND ((SELECT count(*) FROM passkey WHERE user_id=u.id)+(SELECT count(DISTINCT provider_id) FROM account WHERE user_id=u.id AND provider_id IN ('steam','discord'))+CASE WHEN coalesce(u.two_factor_enabled,false) AND EXISTS(SELECT 1 FROM account WHERE user_id=u.id AND provider_id='credential') THEN 1 ELSE 0 END+CASE WHEN recovery_key_hash IS NOT NULL THEN 1 ELSE 0 END>=2) AND (coalesce(u.role,'member')<>'owner' OR recovery_key_hash IS NOT NULL OR EXISTS(SELECT 1 FROM account WHERE user_id=u.id AND provider_id IN ('steam','discord'))) WHERE id=$1")
        .bind(id).execute(&mut **tx).await?;
    Ok(())
}
pub async fn new_session(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    headers: &HeaderMap,
) -> Result<String> {
    // The row lock makes account disable/delete and session creation serialize.
    let row = sqlx::query("SELECT banned,ban_expires,coalesce(username,name) AS name,role FROM \"user\" WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    if disabled(&row)? {
        return Err(ApiError::unauthorized());
    }
    let token = crypto::random_token(32);
    sqlx::query("INSERT INTO session(id,token,expires_at,user_agent,user_id) VALUES($1,$2,now()+interval '7 days',$3,$4)")
        .bind(uuid::Uuid::new_v4().to_string()).bind(&token).bind(ua(headers)).bind(id).execute(&mut **tx).await?;
    sqlx::query("UPDATE \"user\" SET auth_grace_started_at=coalesce(auth_grace_started_at,now()) WHERE id=$1").bind(id).execute(&mut **tx).await?;
    refresh(tx, id).await?;
    let actor = Actor {
        id: id.into(),
        name: row.try_get("name")?,
        owner: row.try_get::<Option<String>, _>("role")?.as_deref() == Some("owner"),
        key: None,
    };
    audit(tx, Some(&actor), headers, "login", "", "ok").await?;
    Ok(cookie(
        state,
        "session_token",
        &crypto::signed(&state.config.auth_secret, &token),
        7 * 86400,
    ))
}
pub async fn lock_seconds(db: &PgPool, keys: &[String]) -> Result<i64> {
    let secs:Option<i64>=sqlx::query_scalar("SELECT greatest(0,ceil(extract(epoch FROM max(locked_until)-now())))::bigint FROM login_attempts WHERE key=ANY($1)").bind(keys).fetch_one(db).await?;
    Ok(secs.unwrap_or(0))
}
pub async fn note_failures(db: &PgPool, keys: &[String]) -> Result<()> {
    let mut tx = db.begin().await?;
    for key in keys {
        let limit = if key.starts_with("ip:") { 40i32 } else { 8 };
        sqlx::query("INSERT INTO login_attempts(key,count,first_at) VALUES($1,1,now()) ON CONFLICT(key) DO UPDATE SET count=CASE WHEN login_attempts.first_at>now()-interval '30 minutes' THEN login_attempts.count+1 ELSE 1 END,first_at=CASE WHEN login_attempts.first_at>now()-interval '30 minutes' THEN login_attempts.first_at ELSE now() END,locked_until=CASE WHEN login_attempts.first_at>now()-interval '30 minutes' AND login_attempts.count+1>=$2 THEN now()+interval '15 minutes' ELSE NULL END")
            .bind(key).bind(limit).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
fn locked(seconds: i64) -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "login_locked",
        format!("Try again in {seconds} seconds."),
    )
}
pub async fn configuration(State(state): State<AppState>) -> Result<Json<Value>> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM \"user\")")
        .fetch_one(&state.db)
        .await?;
    Ok(Json(
        json!({"initialized":exists,"tokenRequired":state.config.identity.setup_token.is_some(),"orgSignup":state.config.identity.allow_signup,"discord":state.config.identity.discord_id.is_some()&&state.config.identity.discord_secret.is_some(),"turnstileSiteKey":state.config.identity.turnstile_key}),
    ))
}
pub async fn setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    origin(&state, &headers)?;
    if let Some(token) = &state.config.identity.setup_token {
        if !bool::from(token.as_bytes().ct_eq(input(&body, "token").as_bytes())) {
            return Err(ApiError::forbidden());
        }
    }
    let username = crypto::username_valid(input(&body, "username"))?;
    let password = input(&body, "password").to_owned();
    if body.get("again").is_some() && password != input(&body, "again") {
        return Err(ApiError::bad("Passwords do not match."));
    }
    let hash = crypto::hash_password(password).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('identity:accounts'))")
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM \"user\"")
        .fetch_one(&mut *tx)
        .await?;
    if count != 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "initialized",
            "The panel is already initialized.",
        ));
    }
    let id = create_user(&mut tx, &username, input(&body, "name"), "owner").await?;
    sqlx::query("INSERT INTO account(id,account_id,provider_id,user_id,password) VALUES($1,$2,'credential',$2,$3)").bind(uuid::Uuid::new_v4().to_string()).bind(&id).bind(hash).execute(&mut *tx).await?;
    let cookie = new_session(&state, &mut tx, &id, &headers).await?;
    audit(&mut tx, None, &headers, "setup.owner", &username, "ok").await?;
    tx.commit().await?;
    reply(json!({"ok":true,"redirect":"/"}), vec![cookie])
}
pub async fn create_user(
    tx: &mut Transaction<'_, Postgres>,
    username: &str,
    name: &str,
    role: &str,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let result=sqlx::query("INSERT INTO \"user\"(id,name,email,email_verified,username,display_username,role) VALUES($1,$2,$3,true,$4,$4,$5)")
        .bind(&id).bind(if name.trim().is_empty(){username.to_owned()}else{crate::feed::truncate(name.trim(),100)})
        .bind(format!("{username}@warcon.invalid")).bind(username).bind(role).execute(&mut **tx).await;
    if result.as_ref().is_err_and(|e| {
        e.as_database_error()
            .is_some_and(|e| e.is_unique_violation())
    }) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "username_taken",
            "Username is already in use.",
        ));
    }
    result?;
    Ok(id)
}
pub async fn login(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    origin(&state, &headers)?;
    let username = crate::feed::truncate(input(&body, "username").trim(), 32).to_lowercase();
    let keys = vec![
        format!("u:{username}"),
        format!("ip:{}", peer_key(&state, peer, &headers)),
    ];
    let seconds = lock_seconds(&state.db, &keys).await?;
    if seconds > 0 {
        return Err(locked(seconds));
    }
    let row=sqlx::query("SELECT u.id,u.banned,u.ban_expires,coalesce(u.two_factor_enabled,false) AS factor,a.password FROM \"user\" u LEFT JOIN account a ON a.user_id=u.id AND a.provider_id='credential' WHERE lower(u.username)=$1").bind(&username).fetch_optional(&state.db).await?;
    let hash = row
        .as_ref()
        .and_then(|r| r.try_get::<Option<String>, _>("password").ok().flatten());
    let verified = crypto::verify_password(input(&body, "password").into(), hash.clone()).await?;
    let valid = verified
        && row
            .as_ref()
            .is_some_and(|r| disabled(r).ok() == Some(false));
    if !valid {
        note_failures(&state.db, &keys).await?;
        let mut tx = state.db.begin().await?;
        audit(
            &mut tx,
            None,
            &headers,
            "login.password",
            &username,
            "denied",
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Username or password is incorrect.",
        ));
    }
    let row = row.unwrap();
    let id: String = row.try_get("id")?;
    let mut tx = state.db.begin().await?;
    let current=sqlx::query("SELECT banned,ban_expires,coalesce(two_factor_enabled,false) AS factor FROM \"user\" WHERE id=$1 FOR UPDATE").bind(&id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::unauthorized)?;
    let current_hash: Option<String> = sqlx::query_scalar(
        "SELECT password FROM account WHERE user_id=$1 AND provider_id='credential'",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    if disabled(&current)? || current_hash != hash {
        return Err(ApiError::unauthorized());
    }
    sqlx::query("DELETE FROM login_attempts WHERE key=ANY($1)")
        .bind(&keys)
        .execute(&mut *tx)
        .await?;
    let remembered = if current.try_get::<bool, _>("factor")? {
        trusted(&state, &mut tx, &id, &headers).await?
    } else {
        false
    };
    if current.try_get::<bool, _>("factor")? && !remembered {
        let challenge = format!("2fa-{}", crypto::random_ascii(20));
        sqlx::query("INSERT INTO verification(id,identifier,value,expires_at) VALUES($1,$2,$3,now()+interval '10 minutes')")
            .bind(uuid::Uuid::new_v4().to_string()).bind(&challenge).bind(&id).execute(&mut *tx).await?;
        tx.commit().await?;
        return reply(
            json!({"ok":true,"twoFactorRedirect":true,"redirect":"/sign-in/verify"}),
            vec![
                cookie(
                    &state,
                    "two_factor",
                    &crypto::signed(&state.config.auth_secret, &challenge),
                    600,
                ),
                cookie(&state, "session_token", "", 0),
            ],
        );
    }
    let mut cookies = vec![cookie(&state, "two_factor", "", 0)];
    if remembered {
        cookies.push(trust_cookie(&state, &mut tx, &id).await?);
    }
    let session = new_session(&state, &mut tx, &id, &headers).await?;
    audit(&mut tx, None, &headers, "login.password", &username, "ok").await?;
    tx.commit().await?;
    reply(
        json!({"ok":true,"redirect":crypto::safe_path(input(&body,"next"),"/")}),
        {
            cookies.push(session);
            cookies
        },
    )
}
async fn trusted(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    headers: &HeaderMap,
) -> Result<bool> {
    let Some(value) = crypto::read_signed(headers, &state.config.auth_secret, "trust_device")
    else {
        return Ok(false);
    };
    let Some((signature, identifier)) = value.split_once('!') else {
        return Ok(false);
    };
    // Better Auth uses base64url for this inner signature, standard base64 for the signed cookie.
    let expected = crypto::signature(&state.config.auth_secret, &format!("{id}!{identifier}"))
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned();
    if !bool::from(expected.as_bytes().ct_eq(signature.as_bytes())) {
        return Ok(false);
    }
    let consumed = sqlx::query(
        "DELETE FROM verification WHERE identifier=$1 AND value=$2 AND expires_at>now()",
    )
    .bind(identifier)
    .bind(id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(consumed > 0)
}
async fn trust_cookie(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<String> {
    let identifier = format!("trust-device-{}", crypto::random_ascii(32));
    sqlx::query("INSERT INTO verification(id,identifier,value,expires_at) VALUES($1,$2,$3,now()+interval '30 days')").bind(uuid::Uuid::new_v4().to_string()).bind(&identifier).bind(id).execute(&mut **tx).await?;
    let signature = crypto::signature(&state.config.auth_secret, &format!("{id}!{identifier}"))
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned();
    Ok(cookie(
        state,
        "trust_device",
        &crypto::signed(
            &state.config.auth_secret,
            &format!("{signature}!{identifier}"),
        ),
        30 * 86400,
    ))
}
pub async fn verify(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    origin(&state, &headers)?;
    let keys = vec![format!("ip:{}", peer_key(&state, peer, &headers))];
    let seconds = lock_seconds(&state.db, &keys).await?;
    if seconds > 0 {
        return Err(locked(seconds));
    }
    let challenge = crypto::read_signed(&headers, &state.config.auth_secret, "two_factor")
        .ok_or_else(ApiError::unauthorized)?;
    let mut tx = state.db.begin().await?;
    let id: Option<String> = sqlx::query_scalar(
        "SELECT value FROM verification WHERE identifier=$1 AND expires_at>now() LIMIT 1",
    )
    .bind(&challenge)
    .fetch_optional(&mut *tx)
    .await?;
    let id = id.ok_or_else(ApiError::unauthorized)?;
    sqlx::query("SELECT id FROM \"user\" WHERE id=$1 FOR UPDATE")
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    let rows = sqlx::query(
        "SELECT id,value FROM verification WHERE identifier=$1 AND expires_at>now() FOR UPDATE",
    )
    .bind(&challenge)
    .fetch_all(&mut *tx)
    .await?;
    if rows.len() != 1 {
        return Err(ApiError::unauthorized());
    }
    if rows[0].try_get::<String, _>("value")? != id {
        return Err(ApiError::unauthorized());
    }
    let row=sqlx::query("SELECT tf.* FROM two_factor tf JOIN \"user\" u ON u.id=tf.user_id WHERE tf.user_id=$1 AND coalesce(tf.verified,true) AND coalesce(u.two_factor_enabled,false) FOR UPDATE OF tf").bind(&id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::unauthorized)?;
    let factor_id: String = row.try_get("id")?;
    if row
        .try_get::<Option<DateTime<Utc>>, _>("locked_until")?
        .is_some_and(|t| t > Utc::now())
    {
        return Err(locked(900));
    }
    let code = input(&body, "code")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    let backup = body["backup"].as_bool() == Some(true);
    let mut codes: Vec<String> = serde_json::from_str(&crypto::factor_decrypt(
        &state.config.auth_secret,
        &row.try_get::<String, _>("backup_codes")?,
    )?)
    .map_err(|_| ApiError::bad("Invalid factor record."))?;
    let valid = if backup {
        if let Some(index) = codes
            .iter()
            .position(|c| bool::from(c.as_bytes().ct_eq(code.as_bytes())))
        {
            codes.remove(index);
            true
        } else {
            false
        }
    } else {
        crypto::verify_totp(
            &crypto::factor_decrypt(
                &state.config.auth_secret,
                &row.try_get::<String, _>("secret")?,
            )?,
            &code,
            Utc::now().timestamp(),
        )
    };
    if !valid {
        sqlx::query("UPDATE two_factor SET failed_verification_count=CASE WHEN locked_until<=now() THEN 1 ELSE coalesce(failed_verification_count,0)+1 END,locked_until=CASE WHEN locked_until<=now() THEN NULL WHEN coalesce(failed_verification_count,0)+1>=10 THEN now()+interval '15 minutes' ELSE NULL END WHERE id=$1").bind(&factor_id).execute(&mut *tx).await?;
        audit(&mut tx, None, &headers, "login.2fa", "", "denied").await?;
        tx.commit().await?;
        note_failures(&state.db, &keys).await?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_factor",
            "That code was not accepted.",
        ));
    }
    sqlx::query("UPDATE two_factor SET failed_verification_count=0,locked_until=NULL,backup_codes=$2 WHERE id=$1").bind(&factor_id).bind(crypto::factor_encrypt(&state.config.auth_secret,&serde_json::to_string(&codes).unwrap())?).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM verification WHERE identifier=$1 OR identifier=$2")
        .bind(&challenge)
        .bind(format!("2fa-attempts-{challenge}"))
        .execute(&mut *tx)
        .await?;
    let mut cookies = vec![
        new_session(&state, &mut tx, &id, &headers).await?,
        cookie(&state, "two_factor", "", 0),
    ];
    sqlx::query("DELETE FROM login_attempts WHERE key=ANY($1)")
        .bind(&keys)
        .execute(&mut *tx)
        .await?;
    if body["trustDevice"].as_bool() == Some(true) {
        cookies.push(trust_cookie(&state, &mut tx, &id).await?);
    }
    audit(&mut tx, None, &headers, "login.2fa", "", "ok").await?;
    tx.commit().await?;
    reply(
        json!({"ok":true,"redirect":crypto::safe_path(input(&body,"next"),"/")}),
        cookies,
    )
}
pub async fn recover(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    origin(&state, &headers)?;
    let username = crate::feed::truncate(input(&body, "username").trim(), 32).to_lowercase();
    let keys = vec![
        format!("u:{username}"),
        format!("ip:{}", peer_key(&state, peer, &headers)),
    ];
    let secs = lock_seconds(&state.db, &keys).await?;
    if secs > 0 {
        return Err(locked(secs));
    }
    let mut tx = state.db.begin().await?;
    let row=sqlx::query("SELECT id,recovery_key_hash,banned,ban_expires FROM \"user\" WHERE lower(username)=$1 FOR UPDATE").bind(&username).fetch_optional(&mut *tx).await?;
    let actual = crypto::recovery_hash(input(&body, "key"));
    let expected = row
        .as_ref()
        .and_then(|r| {
            r.try_get::<Option<String>, _>("recovery_key_hash")
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| "0".repeat(64));
    if !bool::from(actual.as_bytes().ct_eq(expected.as_bytes()))
        || row.as_ref().is_none_or(|r| disabled(r).ok() != Some(false))
    {
        tx.rollback().await?;
        note_failures(&state.db, &keys).await?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Username or recovery key is incorrect.",
        ));
    }
    let id: String = row.unwrap().try_get("id")?;
    sqlx::query("UPDATE \"user\" SET recovery_key_hash=NULL,recovery_key_at=NULL WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM login_attempts WHERE key=ANY($1)")
        .bind(&keys)
        .execute(&mut *tx)
        .await?;
    let session = new_session(&state, &mut tx, &id, &headers).await?;
    audit(&mut tx, None, &headers, "login.recovery", &username, "ok").await?;
    tx.commit().await?;
    reply(
        json!({"ok":true,"redirect":"/account?recovered=1"}),
        vec![session],
    )
}
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    origin(&state, &headers)?;
    if let Some(token) = auth::session_cookie(&headers, &state.config.auth_secret) {
        sqlx::query("DELETE FROM session WHERE token=$1")
            .bind(token)
            .execute(&state.db)
            .await?;
    }
    reply(
        json!({"ok":true,"redirect":"/sign-in"}),
        vec![
            cookie(&state, "session_token", "", 0),
            cookie(&state, "two_factor", "", 0),
        ],
    )
}
pub async fn session(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    let actor = match auth::authenticate_account(&state, &headers, &Method::GET).await {
        Ok(a) => a,
        Err(e) if e.status == StatusCode::UNAUTHORIZED => {
            return reply(
                json!({"user":null,"session":null}),
                vec![cookie(&state, "session_token", "", 0)],
            );
        }
        Err(e) => return Err(e),
    };
    let token = auth::session_cookie(&headers, &state.config.auth_secret)
        .ok_or_else(ApiError::unauthorized)?;
    let renewed=sqlx::query("UPDATE session SET updated_at=now(),expires_at=now()+interval '7 days' WHERE token=$1 AND updated_at<now()-interval '1 day'").bind(&token).execute(&state.db).await?.rows_affected()>0;
    let row=sqlx::query("SELECT u.id,u.role,u.auth_complete,u.auth_grace_started_at,u.must_change_password,jsonb_build_object('id',u.id,'name',u.name,'username',coalesce(u.username,u.display_username,split_part(u.email,'@',1)),'displayUsername',u.display_username,'role',u.role,'image',u.image,'mustChangePassword',u.must_change_password,'defaultOrgId',u.default_org_id,'authComplete',u.auth_complete,'authGraceStartedAt',u.auth_grace_started_at) AS user,jsonb_build_object('id',s.id,'expiresAt',s.expires_at) AS session FROM \"user\" u JOIN session s ON s.user_id=u.id WHERE u.id=$1 AND s.token=$2 AND s.expires_at>now()").bind(&actor.id).bind(&token).fetch_optional(&state.db).await?.ok_or_else(ApiError::unauthorized)?;
    let enrolment_gate =
        !row.try_get::<bool, _>("auth_complete")? && auth::enrolment_due(&state.db, &row).await?;
    reply(
        json!({"user":row.try_get::<Value,_>("user")?,"session":row.try_get::<Value,_>("session")?,"gate":{"password":row.try_get::<bool,_>("must_change_password")?,"enrolment":enrolment_gate}}),
        if renewed {
            vec![cookie(
                &state,
                "session_token",
                &crypto::signed(&state.config.auth_secret, &token),
                7 * 86400,
            )]
        } else {
            vec![]
        },
    )
}
async fn require_password(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    password: &str,
) -> Result<bool> {
    let encoded: Option<String> = sqlx::query_scalar(
        "SELECT password FROM account WHERE user_id=$1 AND provider_id='credential' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .flatten();
    if encoded.is_none() {
        return Ok(false);
    }
    if !crypto::verify_password(password.into(), encoded).await? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "invalid_password",
            "Current password is wrong.",
        ));
    }
    Ok(true)
}
pub async fn account_action(
    State(state): State<AppState>,
    Path(action): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    let actor = auth::authenticate_account(&state, &headers, &Method::POST).await?;
    let mut tx = state.db.begin().await?;
    if action == "deleteAccount" {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('identity:accounts'))")
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("SELECT id FROM \"user\" WHERE id=$1 FOR UPDATE")
        .bind(&actor.id)
        .fetch_one(&mut *tx)
        .await?;
    let mut cookies = vec![];
    let value = match action.as_str() {
        "password" => {
            let next = input(&body, "next");
            crypto::password_valid(next)?;
            if next != input(&body, "again") {
                return Err(ApiError::bad("New passwords do not match."));
            }
            let exists = require_password(&mut tx, &actor.id, input(&body, "current")).await?;
            let hash = crypto::hash_password(next.into()).await?;
            if exists {
                sqlx::query("UPDATE account SET password=$2,updated_at=now() WHERE user_id=$1 AND provider_id='credential'").bind(&actor.id).bind(hash).execute(&mut *tx).await?;
            } else {
                sqlx::query("INSERT INTO account(id,account_id,provider_id,user_id,password) VALUES($1,$2,'credential',$2,$3)").bind(uuid::Uuid::new_v4().to_string()).bind(&actor.id).bind(hash).execute(&mut *tx).await?;
            }
            let token = auth::session_cookie(&headers, &state.config.auth_secret)
                .ok_or_else(ApiError::unauthorized)?;
            sqlx::query("DELETE FROM session WHERE user_id=$1 AND token<>$2")
                .bind(&actor.id)
                .bind(token)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM verification WHERE value=$1 AND (identifier LIKE 'trust-device-%' OR identifier LIKE '2fa-%')").bind(&actor.id).execute(&mut *tx).await?;
            sqlx::query(
                "UPDATE \"user\" SET must_change_password=false,updated_at=now() WHERE id=$1",
            )
            .bind(&actor.id)
            .execute(&mut *tx)
            .await?;
            json!({"changed":exists,"set":!exists})
        }
        "totpStart" => {
            require_password(&mut tx, &actor.id, input(&body, "password")).await?;
            let enabled: bool = sqlx::query_scalar(
                "SELECT coalesce(two_factor_enabled,false) FROM \"user\" WHERE id=$1",
            )
            .bind(&actor.id)
            .fetch_one(&mut *tx)
            .await?;
            if enabled {
                return Err(ApiError::bad("Authenticator is already enabled."));
            }
            let secret = crypto::random_ascii(32);
            let codes = crypto::backup_codes();
            sqlx::query("DELETE FROM two_factor WHERE user_id=$1")
                .bind(&actor.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO two_factor(id,user_id,secret,backup_codes,verified) VALUES($1,$2,$3,$4,false)").bind(uuid::Uuid::new_v4().to_string()).bind(&actor.id).bind(crypto::factor_encrypt(&state.config.auth_secret,&secret)?).bind(crypto::factor_encrypt(&state.config.auth_secret,&serde_json::to_string(&codes).unwrap())?).execute(&mut *tx).await?;
            let email: String = sqlx::query_scalar("SELECT email FROM \"user\" WHERE id=$1")
                .bind(&actor.id)
                .fetch_one(&mut *tx)
                .await?;
            let issuer = &state.config.identity.app_name;
            let mut uri = url::Url::parse("otpauth://totp/").unwrap();
            uri.set_path(&format!("{issuer}:{email}"));
            uri.query_pairs_mut()
                .append_pair("secret", &crypto::base32(secret.as_bytes()))
                .append_pair("issuer", issuer)
                .append_pair("digits", "6")
                .append_pair("period", "30");
            json!({"totp":{"secret":crypto::base32(secret.as_bytes()),"uri":uri.as_str()}})
        }
        "totpConfirm" => {
            let row=sqlx::query("SELECT id,secret,backup_codes,verified,locked_until,failed_verification_count FROM two_factor WHERE user_id=$1 FOR UPDATE").bind(&actor.id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
            if row
                .try_get::<Option<DateTime<Utc>>, _>("locked_until")?
                .is_some_and(|t| t > Utc::now())
            {
                return Err(locked(900));
            }
            let code = input(&body, "code")
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            if !crypto::verify_totp(
                &crypto::factor_decrypt(
                    &state.config.auth_secret,
                    &row.try_get::<String, _>("secret")?,
                )?,
                &code,
                Utc::now().timestamp(),
            ) {
                let expired = row
                    .try_get::<Option<DateTime<Utc>>, _>("locked_until")?
                    .is_some();
                let failures = if expired {
                    1
                } else {
                    row.try_get::<Option<i32>, _>("failed_verification_count")?
                        .unwrap_or(0)
                        + 1
                };
                sqlx::query("UPDATE two_factor SET failed_verification_count=$2,locked_until=CASE WHEN $2>=10 THEN now()+interval '15 minutes' ELSE NULL END WHERE user_id=$1").bind(&actor.id).bind(failures).execute(&mut *tx).await?;
                tx.commit().await?;
                return Err(ApiError::bad("That code was not accepted."));
            }
            if row.try_get::<Option<bool>, _>("verified")?.unwrap_or(true) {
                return Err(ApiError::bad("Authenticator is already enabled."));
            }
            sqlx::query("UPDATE two_factor SET verified=true,failed_verification_count=0,locked_until=NULL WHERE user_id=$1").bind(&actor.id).execute(&mut *tx).await?;
            sqlx::query("UPDATE \"user\" SET two_factor_enabled=true WHERE id=$1")
                .bind(&actor.id)
                .execute(&mut *tx)
                .await?;
            let codes: Value = serde_json::from_str(&crypto::factor_decrypt(
                &state.config.auth_secret,
                &row.try_get::<String, _>("backup_codes")?,
            )?)
            .map_err(|_| ApiError::bad("Invalid factor record."))?;
            json!({"enabled":true,"backupCodes":codes})
        }
        "totpDisable" => {
            require_password(&mut tx, &actor.id, input(&body, "password")).await?;
            sqlx::query("DELETE FROM two_factor WHERE user_id=$1")
                .bind(&actor.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE \"user\" SET two_factor_enabled=false WHERE id=$1")
                .bind(&actor.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM verification WHERE value=$1 AND (identifier LIKE 'trust-device-%' OR identifier LIKE '2fa-%')").bind(&actor.id).execute(&mut *tx).await?;
            json!({"disabled":true})
        }
        "backupCodes" => {
            require_password(&mut tx, &actor.id, input(&body, "password")).await?;
            let codes = crypto::backup_codes();
            let n=sqlx::query("UPDATE two_factor SET backup_codes=$2 WHERE user_id=$1 AND coalesce(verified,true)").bind(&actor.id).bind(crypto::factor_encrypt(&state.config.auth_secret,&serde_json::to_string(&codes).unwrap())?).execute(&mut *tx).await?.rows_affected();
            if n == 0 {
                return Err(ApiError::missing());
            }
            json!({"backupCodes":codes})
        }
        "recoveryKey" => {
            let key = crypto::recovery_key();
            sqlx::query(
                "UPDATE \"user\" SET recovery_key_hash=$2,recovery_key_at=now() WHERE id=$1",
            )
            .bind(&actor.id)
            .bind(crypto::recovery_hash(&key))
            .execute(&mut *tx)
            .await?;
            json!({"recoveryKey":key})
        }
        "recoveryKeyClear" => {
            sqlx::query(
                "UPDATE \"user\" SET recovery_key_hash=NULL,recovery_key_at=NULL WHERE id=$1",
            )
            .bind(&actor.id)
            .execute(&mut *tx)
            .await?;
            json!({"recoveryKeyCleared":true})
        }
        "revoke" => {
            let current = auth::session_cookie(&headers, &state.config.auth_secret)
                .ok_or_else(ApiError::unauthorized)?;
            let n = sqlx::query("DELETE FROM session WHERE user_id=$1 AND id=$2 AND token<>$3")
                .bind(&actor.id)
                .bind(input(&body, "id"))
                .bind(current)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if n == 0 {
                return Err(ApiError::bad("Bad session id."));
            }
            json!({"revoked":true})
        }
        "steam" => {
            let steam = input(&body, "steamId").trim();
            if !steam.is_empty()
                && (steam.len() != 17 || !steam.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(ApiError::bad("Invalid SteamID64."));
            }
            let result = sqlx::query("UPDATE \"user\" SET steam_id=$2 WHERE id=$1")
                .bind(&actor.id)
                .bind(if steam.is_empty() { None } else { Some(steam) })
                .execute(&mut *tx)
                .await;
            if result.as_ref().is_err_and(|e| {
                e.as_database_error()
                    .is_some_and(|e| e.is_unique_violation())
            }) {
                return Err(ApiError::bad("SteamID is already linked."));
            }
            result?;
            json!({"steam":true,"steamId":if steam.is_empty(){Value::Null}else{json!(steam)}})
        }
        "defaultOrg" => {
            let org = input(&body, "orgId").trim();
            let name: Option<String> = if org.is_empty() {
                None
            } else {
                sqlx::query_scalar("SELECT name FROM organizations WHERE id=$1 AND ($2 OR EXISTS(SELECT 1 FROM org_members WHERE org_id=$1 AND user_id=$3))").bind(org).bind(actor.owner).bind(&actor.id).fetch_optional(&mut *tx).await?
            };
            if !org.is_empty() && name.is_none() {
                return Err(ApiError::missing());
            }
            sqlx::query("UPDATE \"user\" SET default_org_id=$2 WHERE id=$1")
                .bind(&actor.id)
                .bind(if org.is_empty() { None } else { Some(org) })
                .execute(&mut *tx)
                .await?;
            cookies.push("warcon_scope=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0".into());
            json!({"defaultOrg":true,"orgName":name.unwrap_or_default()})
        }
        "removePassword" | "unlinkDiscord" | "unlinkSteam" => {
            let provider = match action.as_str() {
                "removePassword" => "credential",
                "unlinkDiscord" => "discord",
                _ => "steam",
            };
            if provider == "credential"
                && input(&body, "confirm").to_lowercase() != actor.name.to_lowercase()
            {
                return Err(ApiError::bad("Type your username to confirm."));
            }
            let ways:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM passkey WHERE user_id=$1)+(SELECT count(*) FROM account WHERE user_id=$1 AND provider_id<>$2)").bind(&actor.id).bind(provider).fetch_one(&mut *tx).await?;
            if ways == 0 {
                return Err(ApiError::bad("Keep another way to sign in."));
            }
            sqlx::query("DELETE FROM account WHERE user_id=$1 AND provider_id=$2")
                .bind(&actor.id)
                .bind(provider)
                .execute(&mut *tx)
                .await?;
            if provider == "credential" {
                sqlx::query("DELETE FROM two_factor WHERE user_id=$1")
                    .bind(&actor.id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("UPDATE \"user\" SET two_factor_enabled=false,must_change_password=false WHERE id=$1").bind(&actor.id).execute(&mut *tx).await?;
                sqlx::query("DELETE FROM verification WHERE value=$1 AND (identifier LIKE 'trust-device-%' OR identifier LIKE '2fa-%')").bind(&actor.id).execute(&mut *tx).await?;
            }
            json!({"unlinked":provider,"passwordRemoved":provider=="credential"})
        }
        "deleteAccount" => {
            let has_password =
                require_password(&mut tx, &actor.id, input(&body, "password")).await?;
            if !has_password {
                if input(&body, "confirm").to_lowercase() != actor.name.to_lowercase() {
                    return Err(ApiError::bad("Type your username to confirm."));
                }
                let token = auth::session_cookie(&headers, &state.config.auth_secret)
                    .ok_or_else(ApiError::unauthorized)?;
                let fresh:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM session WHERE token=$1 AND created_at>now()-interval '1 day')").bind(token).fetch_one(&mut *tx).await?;
                if !fresh {
                    return Err(ApiError::new(
                        StatusCode::FORBIDDEN,
                        "fresh_session_required",
                        "Sign out and sign in again before deleting your account.",
                    ));
                }
            }
            if actor.owner {
                let owners:Vec<String>=sqlx::query_scalar("SELECT id FROM \"user\" WHERE role='owner' AND NOT coalesce(banned,false) ORDER BY id FOR UPDATE").fetch_all(&mut *tx).await?;
                if owners.len() <= 1 {
                    return Err(ApiError::bad("The panel needs at least one owner."));
                }
            }
            let orgs:Vec<String>=sqlx::query_scalar("SELECT o.id FROM organizations o JOIN org_members m ON m.org_id=o.id WHERE m.user_id=$1 AND m.role='owner' ORDER BY o.id FOR UPDATE OF o").bind(&actor.id).fetch_all(&mut *tx).await?;
            for org in orgs {
                let others:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM org_members WHERE org_id=$1 AND role='owner' AND user_id<>$2)").bind(org).bind(&actor.id).fetch_one(&mut *tx).await?;
                if !others {
                    return Err(ApiError::bad(
                        "Transfer ownership or delete your organization before deleting your account.",
                    ));
                }
            }
            sqlx::query(
                "UPDATE api_keys SET revoked_at=coalesce(revoked_at,now()) WHERE created_by=$1",
            )
            .bind(&actor.id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE org_invites SET revoked_at=coalesce(revoked_at,now()) WHERE created_by=$1",
            )
            .bind(&actor.id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE audit_log SET actor_name='[deleted]',user_agent='' WHERE actor_id=$1",
            )
            .bind(&actor.id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE audit_log SET target='[deleted]' WHERE category IN ('auth','user','org') AND lower(target)=lower($1)").bind(&actor.name).execute(&mut *tx).await?;
            sqlx::query("UPDATE list_entries SET added_by_name='[deleted]' WHERE added_by=$1")
                .bind(&actor.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE list_entries SET removed_by_name='[deleted]' WHERE removed_by=$1")
                .bind(&actor.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM verification WHERE value=$1 AND (identifier LIKE 'trust-device-%' OR identifier LIKE '2fa-%')").bind(&actor.id).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM \"user\" WHERE id=$1")
                .bind(&actor.id)
                .execute(&mut *tx)
                .await?;
            let deleted = Actor {
                id: actor.id.clone(),
                name: "[deleted]".into(),
                owner: false,
                key: None,
            };
            audit(
                &mut tx,
                Some(&deleted),
                &headers,
                "account.delete",
                "",
                "ok",
            )
            .await?;
            tx.commit().await?;
            return reply(
                json!({"ok":true,"redirect":"/sign-in?deleted=1"}),
                vec![cookie(&state, "session_token", "", 0)],
            );
        }
        _ => return Err(ApiError::missing()),
    };
    refresh(&mut tx, &actor.id).await?;
    audit(
        &mut tx,
        Some(&actor),
        &headers,
        &format!("account.{action}"),
        "",
        "ok",
    )
    .await?;
    tx.commit().await?;
    reply(value, cookies)
}
pub async fn account(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let actor = auth::authenticate_account(&state, &headers, &Method::GET).await?;
    let token = auth::session_cookie(&headers, &state.config.auth_secret)
        .ok_or_else(ApiError::unauthorized)?;
    let row=sqlx::query("SELECT steam_id,default_org_id,recovery_key_at,auth_grace_started_at,must_change_password FROM \"user\" WHERE id=$1").bind(&actor.id).fetch_one(&state.db).await?;
    let methods = methods(&state.db, &actor.id).await?;
    let enrolment = enrolment(&methods, actor.owner);
    let passkeys:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'createdAt',created_at,'deviceType',device_type,'backedUp',backed_up) FROM passkey WHERE user_id=$1 ORDER BY created_at").bind(&actor.id).fetch_all(&state.db).await?;
    let sessions:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'createdAt',created_at,'expiresAt',expires_at,'userAgent',coalesce(user_agent,''),'current',token=$2) FROM session WHERE user_id=$1 AND expires_at>now() ORDER BY created_at DESC").bind(&actor.id).bind(token).fetch_all(&state.db).await?;
    let settings = crate::settings::load(&state.db).await?;
    let days = settings
        .get(if actor.owner {
            "authGraceDays"
        } else {
            "authMemberGraceDays"
        })
        .and_then(Value::as_f64)
        .map(|v| v as i64)
        .unwrap_or(if actor.owner { 14 } else { 30 });
    let started: Option<DateTime<Utc>> = row.try_get("auth_grace_started_at")?;
    let complete = enrolment["complete"].as_bool() == Some(true);
    let mode = settings
        .get("authEnforce")
        .and_then(Value::as_f64)
        .unwrap_or(0.) as i64;
    let privileged = if mode == 1 && !complete && !actor.owner {
        sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM org_members WHERE user_id=$1 AND role='owner') OR EXISTS(SELECT 1 FROM server_grants g JOIN org_roles r ON r.id=g.role_id WHERE g.user_id=$1 AND r.capabilities ?| ARRAY['bans.manage','slots.manage','lists.ban','lists.reserve','config.apply','automation.manage','rcon.raw','rotation.save','players.notes.manage'])").bind(&actor.id).fetch_one(&state.db).await?
    } else {
        true
    };
    let policy = if mode == 0 {
        json!({"enforced":false,"nudge":true})
    } else {
        json!({"enforced":privileged,"nudge":privileged})
    };
    let deadline = if complete {
        None
    } else {
        started.map(|s| s + Duration::days(days))
    };
    Ok(Json(
        json!({"sessions":sessions,"passkeys":passkeys,"methods":methods,"enrolment":enrolment,"hasPassword":methods["password"],"providers":methods["providers"],"discord":state.config.identity.discord_id.is_some()&&state.config.identity.discord_secret.is_some(),"steamId":row.try_get::<Option<String>,_>("steam_id")?.unwrap_or_default(),"defaultOrgId":row.try_get::<Option<String>,_>("default_org_id")?.unwrap_or_default(),"recoveryKeyAt":row.try_get::<Option<DateTime<Utc>>,_>("recovery_key_at")?,"status":{"due":deadline.is_some_and(|t|t<=Utc::now()),"deadline":deadline,"daysLeft":deadline.map(|t|((t-Utc::now()).num_seconds() as f64/86400.).ceil().max(0.))},"policy":policy}),
    ))
}
