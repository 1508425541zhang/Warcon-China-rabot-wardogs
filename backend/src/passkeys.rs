//! Passkey ceremonies keep challenge state exclusively in PostgreSQL.
use crate::{
    auth,
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, Peer},
    identity, identity_crypto as crypto, identity_signup,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::BTreeMap;
use webauthn_rs::prelude::*;
use webauthn_rs_core::proto::{
    AttestationFormat, COSEKey, COSEKeyType, Credential, ParsedAttestation, RegisteredExtensions,
    UserVerificationPolicy,
};

fn rejected() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "passkey_rejected",
        "That passkey was not accepted.",
    )
}
fn webauthn(state: &AppState) -> Result<Webauthn> {
    let origin = url::Url::parse(&state.config.origin)
        .map_err(|_| ApiError::bad("Invalid panel origin."))?;
    let id = origin
        .host_str()
        .ok_or_else(|| ApiError::bad("Invalid panel origin."))?;
    WebauthnBuilder::new(id, &origin)
        .and_then(|b| b.rp_name(&state.config.identity.app_name).build())
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "passkeys_unavailable",
                "Passkeys require a valid HTTPS origin or localhost.",
            )
        })
}
fn user_handle(id: &str) -> uuid::Uuid {
    // Existing Better Auth user IDs are opaque text. Registration needs a stable 16-byte handle.
    uuid::Uuid::from_bytes(Sha256::digest(id.as_bytes())[..16].try_into().unwrap())
}
pub fn import_credential(
    public_key: &str,
    credential_id: &str,
    counter: i32,
    multi_device: bool,
    backed_up: bool,
    transports: Option<&str>,
) -> Result<Passkey> {
    let bytes = crate::crypto::decode_base64(public_key).map_err(|_| rejected())?;
    let cbor: serde_cbor_2::Value = serde_cbor_2::from_slice(&bytes).map_err(|_| rejected())?;
    let key = COSEKey::try_from(&cbor).map_err(|_| rejected())?;
    let cred_id = crate::crypto::decode_base64(credential_id).map_err(|_| rejected())?;
    if !(1..=1024).contains(&cred_id.len()) || counter < 0 {
        return Err(rejected());
    }
    let transports = transports.map(|v| {
        v.split(',')
            .filter_map(|t| serde_json::from_value(json!(t.trim())).ok())
            .collect()
    });
    Ok(Credential {
        cred_id: cred_id.into(),
        cred: key,
        counter: counter as u32,
        transports,
        user_verified: true,
        backup_eligible: multi_device,
        backup_state: backed_up,
        registration_policy: UserVerificationPolicy::Required,
        extensions: RegisteredExtensions::none(),
        attestation: ParsedAttestation::default(),
        attestation_format: AttestationFormat::None,
    }
    .into())
}
pub fn export_public_key(key: &COSEKey) -> Result<String> {
    use serde_cbor_2::Value as C;
    let mut map = BTreeMap::new();
    map.insert(C::Integer(3), C::Integer(key.type_.clone() as i128));
    match &key.key {
        COSEKeyType::EC_EC2(k) => {
            map.insert(C::Integer(1), C::Integer(2));
            map.insert(C::Integer(-1), C::Integer(k.curve.clone() as i128));
            map.insert(C::Integer(-2), C::Bytes(k.x.as_slice().to_vec()));
            map.insert(C::Integer(-3), C::Bytes(k.y.as_slice().to_vec()));
        }
        COSEKeyType::EC_OKP(k) => {
            map.insert(C::Integer(1), C::Integer(1));
            map.insert(C::Integer(-1), C::Integer(k.curve.clone() as i128));
            map.insert(C::Integer(-2), C::Bytes(k.x.as_slice().to_vec()));
        }
        COSEKeyType::RSA(k) => {
            map.insert(C::Integer(1), C::Integer(3));
            map.insert(C::Integer(-1), C::Bytes(k.n.as_slice().to_vec()));
            map.insert(C::Integer(-2), C::Bytes(k.e.to_vec()));
        }
    }
    Ok(URL_SAFE_NO_PAD.encode(serde_cbor_2::to_vec(&C::Map(map)).map_err(|_| rejected())?))
}
async fn save_challenge(state: &AppState, kind: &str, value: Value) -> Result<String> {
    let token = format!("rust-passkey-{kind}-{}", crypto::random_token(32));
    sqlx::query("INSERT INTO verification(id,identifier,value,expires_at) VALUES($1,$2,$3,now()+interval '5 minutes')").bind(uuid::Uuid::new_v4().to_string()).bind(&token).bind(value.to_string()).execute(&state.db).await?;
    Ok(identity::cookie(
        state,
        "passkey_challenge",
        &crypto::signed(&state.config.auth_secret, &token),
        300,
    ))
}
async fn consume_challenge(state: &AppState, headers: &HeaderMap, kind: &str) -> Result<Value> {
    let token = crypto::read_signed(headers, &state.config.auth_secret, "passkey_challenge")
        .ok_or_else(rejected)?;
    if !token.starts_with(&format!("rust-passkey-{kind}-")) {
        return Err(rejected());
    }
    // DELETE RETURNING is atomic, so only one callback can use this ceremony.
    let value: Option<String> = sqlx::query_scalar(
        "DELETE FROM verification WHERE identifier=$1 AND expires_at>now() RETURNING value",
    )
    .bind(token)
    .fetch_optional(&state.db)
    .await?;
    serde_json::from_str(&value.ok_or_else(rejected)?).map_err(|_| rejected())
}
pub async fn register_options(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    identity::origin(&state, &headers)?;
    let actor = match auth::authenticate_account(&state, &headers, &Method::POST).await {
        Ok(a) => Some(a),
        Err(e) if e.status == StatusCode::UNAUTHORIZED => None,
        Err(e) => return Err(e),
    };
    let (id, username, display_name, setup, invite) = if let Some(actor) = &actor {
        (
            actor.id.clone(),
            actor.name.clone(),
            actor.name.clone(),
            false,
            String::new(),
        )
    } else {
        let setup = identity_signup::authorize_signup(&state, &body, peer, &headers, true).await?;
        let username = crypto::username_valid(body["username"].as_str().unwrap_or(""))?;
        (
            uuid::Uuid::new_v4().to_string(),
            username.clone(),
            crate::feed::truncate(body["displayName"].as_str().unwrap_or(&username), 80),
            setup,
            body["invite"].as_str().unwrap_or("").to_owned(),
        )
    };
    let excluded: Vec<String> =
        sqlx::query_scalar("SELECT credential_id FROM passkey WHERE user_id=$1")
            .bind(&id)
            .fetch_all(&state.db)
            .await?;
    let excluded = excluded
        .iter()
        .map(|s| {
            crate::crypto::decode_base64(s)
                .map(Into::into)
                .map_err(|_| rejected())
        })
        .collect::<Result<Vec<_>>>()?;
    let (options, ceremony) = webauthn(&state)?
        .start_passkey_registration(user_handle(&id), &username, &display_name, Some(excluded))
        .map_err(|_| rejected())?;
    let value = json!({"ceremony":ceremony,"id":id,"username":username,"displayName":display_name,"signup":actor.is_none(),"setup":setup,"invite":invite,"name":crate::feed::truncate(body["name"].as_str().unwrap_or("Passkey"),60)});
    let cookie = save_challenge(&state, "register", value).await?;
    identity::reply(
        serde_json::to_value(options).map_err(|_| rejected())?["publicKey"].clone(),
        vec![cookie],
    )
}
pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    identity::origin(&state, &headers)?;
    let stored = consume_challenge(&state, &headers, "register").await?;
    let signup = stored["signup"].as_bool() == Some(true);
    let setup = stored["setup"].as_bool() == Some(true);
    let id = stored["id"].as_str().ok_or_else(rejected)?;
    let actor = if signup {
        None
    } else {
        Some(auth::authenticate_account(&state, &headers, &Method::POST).await?)
    };
    if actor.as_ref().is_some_and(|a| a.id != id) {
        return Err(rejected());
    }
    let response: RegisterPublicKeyCredential =
        serde_json::from_value(body["response"].clone()).map_err(|_| rejected())?;
    let ceremony: PasskeyRegistration =
        serde_json::from_value(stored["ceremony"].clone()).map_err(|_| rejected())?;
    let passkey = webauthn(&state)?
        .finish_passkey_registration(&response, &ceremony)
        .map_err(|_| rejected())?;
    let credential: Credential = passkey.into();
    let mut tx = state.db.begin().await?;
    if signup {
        identity_signup::final_guard(
            &state,
            &mut tx,
            setup,
            stored["invite"].as_str().unwrap_or(""),
        )
        .await?;
        // Keep the user ID bound to the verified WebAuthn handle.
        let username = stored["username"].as_str().ok_or_else(rejected)?;
        let result=sqlx::query("INSERT INTO \"user\"(id,name,email,email_verified,username,display_username,role) VALUES($1,$2,$3,true,$4,$4,$5)").bind(id).bind(stored["displayName"].as_str().unwrap_or(username)).bind(format!("{username}@warcon.invalid")).bind(username).bind(if setup{"owner"}else{"member"}).execute(&mut *tx).await;
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
    } else {
        sqlx::query("SELECT id FROM \"user\" WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(rejected)?;
    }
    let credential_id = URL_SAFE_NO_PAD.encode(credential.cred_id.as_slice());
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("passkey:{credential_id}"))
        .execute(&mut *tx)
        .await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM passkey WHERE credential_id=$1)")
            .bind(&credential_id)
            .fetch_one(&mut *tx)
            .await?;
    if exists {
        return Err(rejected());
    }
    let passkey_id = uuid::Uuid::new_v4().to_string();
    let transports = credential.transports.as_ref().map(|t| {
        t.iter()
            .filter_map(|v| serde_json::to_value(v).ok()?.as_str().map(str::to_owned))
            .collect::<Vec<_>>()
            .join(",")
    });
    let counter = i32::try_from(credential.counter).map_err(|_| rejected())?;
    sqlx::query("INSERT INTO passkey(id,name,public_key,user_id,credential_id,counter,device_type,backed_up,transports,created_at,rust_user_handle) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,now(),$10)").bind(&passkey_id).bind(stored["name"].as_str().unwrap_or("Passkey")).bind(export_public_key(&credential.cred)?).bind(id).bind(credential_id).bind(counter).bind(if credential.backup_eligible{"multiDevice"}else{"singleDevice"}).bind(credential.backup_state).bind(transports).bind(URL_SAFE_NO_PAD.encode(user_handle(id).as_bytes())).execute(&mut *tx).await?;
    identity::refresh(&mut tx, id).await?;
    identity::audit(
        &mut tx,
        actor.as_ref(),
        &headers,
        if signup {
            "signup.passkey"
        } else {
            "passkey.add"
        },
        "",
        "ok",
    )
    .await?;
    let mut cookies = vec![identity::cookie(&state, "passkey_challenge", "", 0)];
    if signup {
        cookies.push(identity::new_session(&state, &mut tx, id, &headers).await?);
    }
    tx.commit().await?;
    identity::reply(json!({"ok":true,"id":passkey_id,"userId":id}), cookies)
}
pub async fn auth_options(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response> {
    identity::origin(&state, &headers)?;
    crate::ratelimit::allow(
        format!(
            "passkey-auth:{}",
            identity::peer_key(&state, peer, &headers)
        ),
        30,
        true,
    )?;
    let (mut options, ceremony) = webauthn(&state)?
        .start_discoverable_authentication()
        .map_err(|_| rejected())?;
    // The existing login button uses an explicit ceremony, rather than conditional autofill.
    options.mediation = None;
    let cookie = save_challenge(&state, "auth", json!({"ceremony":ceremony})).await?;
    identity::reply(
        serde_json::to_value(options).map_err(|_| rejected())?["publicKey"].clone(),
        vec![cookie],
    )
}
pub async fn authenticate(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    identity::origin(&state, &headers)?;
    crate::ratelimit::allow(
        format!(
            "passkey-auth:{}",
            identity::peer_key(&state, peer, &headers)
        ),
        30,
        true,
    )?;
    let stored = consume_challenge(&state, &headers, "auth").await?;
    let response: PublicKeyCredential =
        serde_json::from_value(body["response"].clone()).map_err(|_| rejected())?;
    let credential_id = URL_SAFE_NO_PAD.encode(response.get_credential_id());
    let id: Option<String> =
        sqlx::query_scalar("SELECT user_id FROM passkey WHERE credential_id=$1 LIMIT 1")
            .bind(&credential_id)
            .fetch_optional(&state.db)
            .await?;
    let id = id.ok_or_else(rejected)?;
    let handle = response.get_user_unique_id().ok_or_else(rejected)?;
    if handle.is_empty() || handle.len() > 64 {
        return Err(rejected());
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM \"user\" WHERE id=$1 FOR UPDATE")
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(rejected)?;
    let rows =
        sqlx::query("SELECT * FROM passkey WHERE credential_id=$1 AND user_id=$2 FOR UPDATE")
            .bind(&credential_id)
            .bind(&id)
            .fetch_all(&mut *tx)
            .await?;
    if rows.len() != 1 {
        return Err(rejected());
    }
    let row = &rows[0];
    let handle = URL_SAFE_NO_PAD.encode(handle);
    if row
        .try_get::<Option<String>, _>("rust_user_handle")?
        .is_some_and(|expected| expected != handle)
    {
        return Err(rejected());
    }
    let mut passkey = import_credential(
        &row.try_get::<String, _>("public_key")?,
        &credential_id,
        row.try_get("counter")?,
        row.try_get::<String, _>("device_type")? == "multiDevice",
        row.try_get("backed_up")?,
        row.try_get::<Option<String>, _>("transports")?.as_deref(),
    )?;
    let ceremony: DiscoverableAuthentication =
        serde_json::from_value(stored["ceremony"].clone()).map_err(|_| rejected())?;
    let result = webauthn(&state)?
        .finish_discoverable_authentication(&response, ceremony, &[DiscoverableKey::from(&passkey)])
        .map_err(|_| rejected())?;
    let old_counter: i32 = row.try_get("counter")?;
    if (result.counter() > 0 || old_counter > 0) && result.counter() <= old_counter as u32 {
        return Err(rejected());
    }
    passkey.update_credential(&result);
    let credential: Credential = passkey.into();
    sqlx::query("UPDATE passkey SET counter=$2,backed_up=$3,device_type=$4,rust_user_handle=coalesce(rust_user_handle,$5) WHERE id=$1")
        .bind(row.try_get::<String, _>("id")?)
        .bind(i32::try_from(credential.counter).map_err(|_| rejected())?)
        .bind(credential.backup_state)
        .bind(if credential.backup_eligible {
            "multiDevice"
        } else {
            "singleDevice"
        })
        .bind(handle)
        .execute(&mut *tx)
        .await?;
    let cookie = identity::new_session(&state, &mut tx, &id, &headers).await?;
    identity::audit(&mut tx, None, &headers, "login.passkey", "", "ok").await?;
    tx.commit().await?;
    identity::reply(
        json!({"ok":true}),
        vec![cookie, identity::cookie(&state, "passkey_challenge", "", 0)],
    )
}
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let actor = auth::authenticate_account(&state, &headers, &Method::GET).await?;
    let passkeys:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'createdAt',created_at,'deviceType',device_type,'backedUp',backed_up) FROM passkey WHERE user_id=$1 ORDER BY created_at").bind(actor.id).fetch_all(&state.db).await?;
    Ok(Json(json!({"passkeys":passkeys})))
}
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = auth::authenticate_account(&state, &headers, &Method::DELETE).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM \"user\" WHERE id=$1 FOR UPDATE")
        .bind(&actor.id)
        .fetch_one(&mut *tx)
        .await?;
    let remaining:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM passkey WHERE user_id=$1 AND id<>$2)+(SELECT count(*) FROM account WHERE user_id=$1)").bind(&actor.id).bind(&id).fetch_one(&mut *tx).await?;
    if remaining == 0 {
        return Err(ApiError::bad("Keep another way to sign in."));
    }
    if sqlx::query("DELETE FROM passkey WHERE id=$1 AND user_id=$2")
        .bind(id)
        .bind(&actor.id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::missing());
    }
    identity::refresh(&mut tx, &actor.id).await?;
    identity::audit(&mut tx, Some(&actor), &headers, "passkey.delete", "", "ok").await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
