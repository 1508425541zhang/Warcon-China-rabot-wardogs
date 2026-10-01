use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use openssl::{
    bn::{BigNum, BigNumContext},
    ec::{EcGroup, EcKey, EcPoint},
    hash::MessageDigest,
    nid::Nid,
    pkey::{PKey, Private},
    sign::Signer,
};
use serde_cbor_2::Value as C;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::collections::BTreeMap;
use tower::ServiceExt;
use warcon_backend::{
    api,
    config::{AppState, Config},
    migrations, passkeys,
};

async fn call(
    app: axum::Router,
    path: &str,
    body: Value,
    cookie: &str,
) -> (StatusCode, Value, Vec<String>) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("origin", "http://localhost:3000")
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    let status = response.status();
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .collect();
    (
        status,
        serde_json::from_slice(&to_bytes(response.into_body(), 1048576).await.unwrap()).unwrap(),
        cookies,
    )
}
fn private_key() -> PKey<Private> {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    let mut point = EcPoint::new(&group).unwrap();
    let mut context = BigNumContext::new().unwrap();
    let x =
        BigNum::from_hex_str("6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296")
            .unwrap();
    let y =
        BigNum::from_hex_str("4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5")
            .unwrap();
    point
        .set_affine_coordinates_gfp(&group, &x, &y, &mut context)
        .unwrap();
    PKey::from_ec_key(
        EcKey::from_private_components(&group, &BigNum::from_u32(1).unwrap(), &point).unwrap(),
    )
    .unwrap()
}
fn cose() -> Vec<u8> {
    let x =
        hex::decode("6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296").unwrap();
    let y =
        hex::decode("4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5").unwrap();
    serde_cbor_2::to_vec(&C::Map(BTreeMap::from([
        (C::Integer(1), C::Integer(2)),
        (C::Integer(3), C::Integer(-7)),
        (C::Integer(-1), C::Integer(1)),
        (C::Integer(-2), C::Bytes(x)),
        (C::Integer(-3), C::Bytes(y)),
    ])))
    .unwrap()
}
fn registration(options: &Value, cred_id: &[u8]) -> Value {
    let client=json!({"type":"webauthn.create","challenge":options["challenge"],"origin":"http://localhost:3000","crossOrigin":false}).to_string();
    let mut data = Sha256::digest(b"localhost").to_vec();
    data.push(0x45);
    data.extend([0; 4]);
    data.extend([0; 16]);
    data.extend((cred_id.len() as u16).to_be_bytes());
    data.extend(cred_id);
    data.extend(cose());
    let attest = serde_cbor_2::to_vec(&C::Map(BTreeMap::from([
        (C::Text("fmt".into()), C::Text("none".into())),
        (C::Text("attStmt".into()), C::Map(BTreeMap::new())),
        (C::Text("authData".into()), C::Bytes(data)),
    ])))
    .unwrap();
    json!({"response":{"id":B64.encode(cred_id),"rawId":B64.encode(cred_id),"type":"public-key","response":{"clientDataJSON":B64.encode(client),"attestationObject":B64.encode(attest),"transports":["internal"]},"clientExtensionResults":{}}})
}
fn assertion(
    options: &Value,
    cred_id: &[u8],
    handle: &str,
    counter: u32,
    origin: &str,
    uv: bool,
) -> Value {
    let client=json!({"type":"webauthn.get","challenge":options["challenge"],"origin":origin,"crossOrigin":false}).to_string();
    let mut data = Sha256::digest(b"localhost").to_vec();
    data.push(if uv { 5 } else { 1 });
    data.extend(counter.to_be_bytes());
    let signed = [
        data.as_slice(),
        Sha256::digest(client.as_bytes()).as_slice(),
    ]
    .concat();
    let key = private_key();
    let mut signer = Signer::new(MessageDigest::sha256(), &key).unwrap();
    signer.update(&signed).unwrap();
    let signature = signer.sign_to_vec().unwrap();
    json!({"response":{"id":B64.encode(cred_id),"rawId":B64.encode(cred_id),"type":"public-key","response":{"clientDataJSON":B64.encode(client),"authenticatorData":B64.encode(data),"signature":B64.encode(signature),"userHandle":handle},"clientExtensionResults":{}}})
}
fn challenge(cookies: &[String]) -> String {
    cookies
        .iter()
        .find(|s| s.starts_with("warcon.passkey_challenge="))
        .unwrap()
        .clone()
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn native_passkeys_register_verify_replay_and_upgrade_random_legacy_handles() {
    let options: PgConnectOptions = std::env::var("TEST_DATABASE_URL").unwrap().parse().unwrap();
    let admin = PgPool::connect_with(options.clone()).await.unwrap();
    let name = format!("warcon_rust_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let db = PgPool::connect_with(options.database(&name)).await.unwrap();
    migrations::migrate(
        &db,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../drizzle"),
    )
    .await
    .unwrap();
    let state = AppState {
        runtime: Default::default(),
        db: db.clone(),
        config: Config::for_test(),
    };
    let app = api::router(state.clone());
    let cred_id = [7u8; 32];
    let (status, options, cookies) = call(
        app.clone(),
        "/api/passkeys/register-options",
        json!({"username":"passkey-owner","displayName":"Passkey owner"}),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    let handle = options["user"]["id"].as_str().unwrap().to_owned();
    let response = registration(&options, &cred_id);
    let challenge_cookie = challenge(&cookies);
    let (status, result, cookies) = call(
        app.clone(),
        "/api/passkeys",
        response.clone(),
        &challenge_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let owner_session = cookies
        .iter()
        .find(|s| s.starts_with("warcon.session_token="))
        .unwrap()
        .clone();
    let id = result["userId"].as_str().unwrap().to_owned();
    assert_eq!(
        call(app.clone(), "/api/passkeys", response, &challenge_cookie)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let owner: bool = sqlx::query_scalar("SELECT role='owner' FROM \"user\" WHERE id=$1")
        .bind(&id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(owner);
    let (_, options, cookies) =
        call(app.clone(), "/api/passkeys/auth-options", json!({}), "").await;
    let signed = assertion(
        &options,
        &cred_id,
        &handle,
        1,
        "http://localhost:3000",
        true,
    );
    let (status, result, _) = call(
        app.clone(),
        "/api/passkeys/auth",
        signed.clone(),
        &challenge(&cookies),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        call(
            app.clone(),
            "/api/passkeys/auth",
            signed,
            &challenge(&cookies)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let public_key: String = sqlx::query_scalar("SELECT public_key FROM passkey WHERE user_id=$1")
        .bind(&id)
        .fetch_one(&db)
        .await
        .unwrap();
    let imported =
        passkeys::import_credential(&public_key, &B64.encode(cred_id), 1, false, false, None)
            .unwrap();
    assert_eq!(
        passkeys::export_public_key(imported.get_public_key()).unwrap(),
        public_key
    );
    // Counter rollback, origin mismatch and an absent UV flag must all fail even with a valid signature.
    for (counter, origin, uv) in [
        (1, "http://localhost:3000", true),
        (2, "https://evil.test", true),
        (2, "http://localhost:3000", false),
    ] {
        let (_, options, cookies) =
            call(app.clone(), "/api/passkeys/auth-options", json!({}), "").await;
        let response = assertion(&options, &cred_id, &handle, counter, origin, uv);
        assert_eq!(
            call(
                app.clone(),
                "/api/passkeys/auth",
                response,
                &challenge(&cookies)
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    // Better Auth made a random 32-byte handle and never saved it in the old schema.
    sqlx::query("UPDATE passkey SET rust_user_handle=NULL WHERE user_id=$1")
        .bind(&id)
        .execute(&db)
        .await
        .unwrap();
    let legacy_handle = B64.encode(b"legacy-random-handle-1234567890123");
    let (_, options, cookies) =
        call(app.clone(), "/api/passkeys/auth-options", json!({}), "").await;
    let response = assertion(
        &options,
        &cred_id,
        &legacy_handle,
        2,
        "http://localhost:3000",
        true,
    );
    let (status, result, _) = call(
        app.clone(),
        "/api/passkeys/auth",
        response,
        &challenge(&cookies),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let bound: String = sqlx::query_scalar("SELECT rust_user_handle FROM passkey WHERE user_id=$1")
        .bind(&id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(bound, legacy_handle);
    let (_, options, cookies) =
        call(app.clone(), "/api/passkeys/auth-options", json!({}), "").await;
    let response = assertion(
        &options,
        &cred_id,
        &handle,
        3,
        "http://localhost:3000",
        true,
    );
    assert_eq!(
        call(
            app.clone(),
            "/api/passkeys/auth",
            response,
            &challenge(&cookies)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    // A registration initiated by one account cannot be completed under another session.
    let (_, options, cookies) = call(
        app.clone(),
        "/api/passkeys/register-options",
        json!({"name":"second"}),
        &owner_session,
    )
    .await;
    let response = registration(&options, &[8u8; 32]);
    assert_eq!(
        call(app.clone(), "/api/passkeys", response, &challenge(&cookies))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    drop(app);
    drop(state);
    db.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
