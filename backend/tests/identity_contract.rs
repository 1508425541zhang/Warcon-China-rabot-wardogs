use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgConnectOptions};
use tower::ServiceExt;
use warcon_backend::{
    api,
    config::{AppState, Config},
    identity_crypto as crypto, migrations,
};

async fn call(
    app: axum::Router,
    path: &str,
    body: Value,
    cookie: &str,
    origin: bool,
) -> (StatusCode, Value, Vec<String>) {
    let mut req = Request::builder()
        .method(
            if path.ends_with("/account") || path.ends_with("/session") {
                "GET"
            } else {
                "POST"
            },
        )
        .uri(path)
        .header("content-type", "application/json");
    if origin {
        req = req.header("origin", "http://localhost:3000");
    }
    if !cookie.is_empty() {
        req = req.header("cookie", cookie);
    }
    let response = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .collect();
    let bytes = to_bytes(response.into_body(), 1048576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap(), cookies)
}
fn session_cookie(cookies: &[String]) -> String {
    cookies
        .iter()
        .find(|s| s.starts_with("warcon.session_token=") && !s.ends_with('='))
        .unwrap()
        .clone()
}
fn challenge_cookie(cookies: &[String]) -> String {
    cookies
        .iter()
        .find(|s| s.starts_with("warcon.two_factor=") && !s.ends_with('='))
        .unwrap()
        .clone()
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn complete_password_otp_recovery_and_account_transactions() {
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
    let mut config = Config::for_test();
    config.identity.setup_token = Some("fixture-setup-token".into());
    let state = AppState {
        runtime: Default::default(),
        db: db.clone(),
        config,
    };
    let app = api::router(state.clone());
    let setup = json!({"username":"Owner","password":"PasswordForTest123!","again":"PasswordForTest123!","token":"fixture-setup-token"});
    assert_eq!(
        call(app.clone(), "/api/identity/setup", setup.clone(), "", false)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            app.clone(),
            "/api/identity/register",
            setup.clone(),
            "",
            true
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, _, cookies) =
        call(app.clone(), "/api/identity/setup", setup.clone(), "", true).await;
    assert_eq!(status, StatusCode::OK);
    let mut owner = session_cookie(&cookies);
    assert_eq!(
        call(app.clone(), "/api/identity/setup", setup.clone(), "", true)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let id: String = sqlx::query_scalar("SELECT id FROM \"user\" WHERE username='owner'")
        .fetch_one(&db)
        .await
        .unwrap();
    let (_, payload, _) = call(
        app.clone(),
        "/api/identity/session",
        Value::Null,
        &owner,
        true,
    )
    .await;
    assert_eq!(payload["user"]["role"], "owner");
    assert!(payload["user"].get("recovery_key_hash").is_none());
    assert!(payload["session"].get("token").is_none());
    sqlx::query("UPDATE \"user\" SET must_change_password=true WHERE id=$1")
        .bind(&id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        call(
            app.clone(),
            "/api/identity/account",
            Value::Null,
            &owner,
            true
        )
        .await
        .0,
        StatusCode::OK,
        "forced users must retain account access"
    );
    assert_eq!(call(app.clone(),"/api/identity/account/password",json!({"current":"incorrect","next":"UpdatedPassword123!","again":"UpdatedPassword123!"}),&owner,true).await.0,StatusCode::FORBIDDEN);
    assert_eq!(call(app.clone(),"/api/identity/account/password",json!({"current":"PasswordForTest123!","next":"UpdatedPassword123!","again":"UpdatedPassword123!"}),&owner,true).await.0,StatusCode::OK);
    let (_, _, cookies) = call(
        app.clone(),
        "/api/identity/login",
        json!({"username":"OWNER","password":"UpdatedPassword123!","next":"//evil.test"}),
        "",
        true,
    )
    .await;
    let previous = session_cookie(&cookies);
    let (_, totp, _) = call(
        app.clone(),
        "/api/identity/account/totpStart",
        json!({"password":"UpdatedPassword123!"}),
        &owner,
        true,
    )
    .await;
    assert!(
        totp["totp"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/")
    );
    let encrypted: String = sqlx::query_scalar("SELECT secret FROM two_factor WHERE user_id=$1")
        .bind(&id)
        .fetch_one(&db)
        .await
        .unwrap();
    let secret = crypto::factor_decrypt(&state.config.auth_secret, &encrypted).unwrap();
    let code = crypto::totp(&secret, chrono::Utc::now().timestamp());
    let (status, confirmed, _) = call(
        app.clone(),
        "/api/identity/account/totpConfirm",
        json!({"code":code}),
        &owner,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let codes = confirmed["backupCodes"].as_array().unwrap();
    assert_eq!(codes.len(), 10);
    assert_eq!(
        call(
            app.clone(),
            "/api/identity/verify",
            json!({"code":code}),
            "",
            true
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, login, cookies) = call(
        app.clone(),
        "/api/identity/login",
        json!({"username":"owner","password":"UpdatedPassword123!"}),
        "",
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login["twoFactorRedirect"], true);
    let challenge = challenge_cookie(&cookies);
    let backup = codes[0].as_str().unwrap();
    let future_a = call(
        app.clone(),
        "/api/identity/verify",
        json!({"code":backup,"backup":true,"trustDevice":true}),
        &challenge,
        true,
    );
    let future_b = call(
        app.clone(),
        "/api/identity/verify",
        json!({"code":backup,"backup":true}),
        &challenge,
        true,
    );
    let (a, b) = tokio::join!(future_a, future_b);
    let accepted = if a.0 == StatusCode::OK {
        assert_eq!(b.0, StatusCode::UNAUTHORIZED);
        a
    } else {
        assert_eq!(a.0, StatusCode::UNAUTHORIZED);
        assert_eq!(b.0, StatusCode::OK);
        b
    };
    owner = session_cookie(&accepted.2);
    let remaining: String =
        sqlx::query_scalar("SELECT backup_codes FROM two_factor WHERE user_id=$1")
            .bind(&id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(
            &crypto::factor_decrypt(&state.config.auth_secret, &remaining).unwrap()
        )
        .unwrap()
        .len(),
        9
    );
    let (status, key, _) = call(
        app.clone(),
        "/api/identity/account/recoveryKey",
        json!({}),
        &owner,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let key = key["recoveryKey"].as_str().unwrap();
    let r1 = call(
        app.clone(),
        "/api/identity/recover",
        json!({"username":"owner","key":key}),
        "",
        true,
    );
    let r2 = call(
        app.clone(),
        "/api/identity/recover",
        json!({"username":"owner","key":key}),
        "",
        true,
    );
    let (a, b) = tokio::join!(r1, r2);
    assert!(
        (a.0 == StatusCode::OK && b.0 == StatusCode::UNAUTHORIZED)
            || (b.0 == StatusCode::OK && a.0 == StatusCode::UNAUTHORIZED)
    );
    // Changing the password revokes every other session and trusted-device record.
    assert_eq!(call(app.clone(),"/api/identity/account/password",json!({"current":"UpdatedPassword123!","next":"AnotherPassword123!","again":"AnotherPassword123!"}),&owner,true).await.0,StatusCode::OK);
    assert_eq!(
        call(
            app.clone(),
            "/api/identity/session",
            Value::Null,
            &previous,
            true
        )
        .await
        .1["user"],
        Value::Null
    );
    assert_eq!(
        call(
            app.clone(),
            "/api/identity/account/deleteAccount",
            json!({"password":"AnotherPassword123!"}),
            &owner,
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "the last owner cannot delete themselves"
    );
    // Legacy Better Auth verification records remain consumable after upgrade.
    let old_challenge = "2fa-legacy-fixture";
    sqlx::query("INSERT INTO verification(id,identifier,value,expires_at) VALUES('legacy-challenge',$1,$2,now()+interval '10 minutes')").bind(old_challenge).bind(&id).execute(&db).await.unwrap();
    let legacy_cookie = format!(
        "warcon.two_factor={}",
        percent_encoding::utf8_percent_encode(
            &crypto::signed(&state.config.auth_secret, old_challenge),
            percent_encoding::NON_ALPHANUMERIC
        )
    );
    assert_eq!(
        call(
            app.clone(),
            "/api/identity/verify",
            json!({"code":crypto::totp(&secret,chrono::Utc::now().timestamp())}),
            &legacy_cookie,
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    // Per-account lockout persists across API processes.
    sqlx::query("DELETE FROM login_attempts")
        .execute(&db)
        .await
        .unwrap();
    for _ in 0..8 {
        assert_eq!(
            call(
                app.clone(),
                "/api/identity/login",
                json!({"username":"owner","password":"IncorrectPassword123!"}),
                "",
                true
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(
            app.clone(),
            "/api/identity/login",
            json!({"username":"owner","password":"AnotherPassword123!"}),
            "",
            true
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query("UPDATE \"user\" SET banned=true WHERE id=$1")
        .bind(&id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        call(
            app.clone(),
            "/api/identity/account",
            Value::Null,
            &owner,
            true
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(app.clone(), "/api/identity/logout", json!({}), &owner, true)
            .await
            .0,
        StatusCode::OK
    );
    let leaked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_log WHERE target LIKE '%Password%' OR detail::text LIKE '%Password%')").fetch_one(&db).await.unwrap();
    assert!(!leaked);
    drop(app);
    drop(state);
    db.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
