use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Response, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
use warcon_backend::{
    actions, api,
    config::{AppState, Config},
    crypto,
    game::Client,
    identity_crypto, migrations,
};
#[derive(Clone)]
struct Fixture(Arc<Mutex<VecDeque<Value>>>);
async fn game(State(state): State<Fixture>, request: Request) -> Response<Body> {
    let expected = state
        .0
        .lock()
        .unwrap()
        .pop_front()
        .expect("Unexpected game request");
    assert_eq!(
        request.method().as_str(),
        expected["method"].as_str().unwrap()
    );
    assert_eq!(
        request.uri().to_string(),
        expected["path"].as_str().unwrap()
    );
    assert_eq!(
        request.headers()["authorization"],
        "Bearer fixture-rcon-key"
    );
    for (key, value) in expected["headers"].as_object().unwrap() {
        assert_eq!(
            request.headers().get(key).map(|v| v.to_str().unwrap()),
            value.as_str(),
            "header {key}"
        );
    }
    let bytes = to_bytes(request.into_body(), 2097152).await.unwrap();
    let actual = std::str::from_utf8(&bytes).unwrap();
    if expected["body"].is_null() {
        assert!(actual.is_empty())
    } else {
        let body = expected["body"].as_str().unwrap();
        if expected["headers"]
            .as_object()
            .unwrap()
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v == "application/json")
        {
            assert_eq!(
                serde_json::from_str::<Value>(actual).unwrap(),
                serde_json::from_str::<Value>(body).unwrap()
            )
        } else {
            assert_eq!(actual, body)
        }
    }
    let response = &expected["response"];
    let mut builder = Response::builder().status(response["status"].as_u64().unwrap() as u16);
    for (key, value) in response["headers"].as_object().unwrap() {
        builder = builder.header(key, value.as_str().unwrap());
    }
    builder
        .body(Body::from(response["text"].as_str().unwrap().to_owned()))
        .unwrap()
}
fn same(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64()),
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{actual} != {expected}");
            for (key, b) in b {
                same(
                    a.get(key)
                        .unwrap_or_else(|| panic!("Missing {key}: {actual}")),
                    b,
                )
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                same(a, b)
            }
        }
        _ => assert_eq!(actual, expected),
    }
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    cookie: &str,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(method)
                .uri(path)
                .header("origin", "http://localhost:3000")
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1048576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn every_original_registry_action_request_shape_and_permissions() {
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
    let fixture = Fixture(Arc::new(Mutex::new(VecDeque::new())));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn({
        let fixture = fixture.clone();
        async move {
            axum::serve(listener, Router::new().fallback(game).with_state(fixture))
                .await
                .unwrap()
        }
    });
    let state = AppState {
        db: db.clone(),
        config: Config::for_test(),
    };
    let blob = crypto::encrypt_secret(&state.config.encryption_key, "fixture-rcon-key").unwrap();
    sqlx::query("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org')")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO servers(id,name,host,port,scheme,password_enc,org_id,allow_private) VALUES('server','Fixture','127.0.0.1',$1,'http',$2,'org',true)").bind(port as i32).bind(blob).execute(&db).await.unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!("../fixtures/actions.json")).unwrap();
    assert_eq!(cases.len(), actions::NAMES.len());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        assert!(actions::definition(name).is_some());
        *fixture.0.lock().unwrap() = case["calls"].as_array().unwrap().iter().cloned().collect();
        let client = Client::for_server(&state, "server").await.unwrap();
        let actual = actions::run(&client, name, &case["params"])
            .await
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(
            fixture.0.lock().unwrap().is_empty(),
            "{name} did not make all expected calls"
        );
        same(&actual, &case["result"]);
    }
    // The raw escape hatch cannot expose credential documents, log peers, escaped paths or external hosts.
    for path in [
        "/v1/config",
        "/v1/CONFIG/validate",
        "/v1/config;",
        "/v1/%63onfig",
        "/v1/audit",
        "/v1/audit/rows",
        "//evil.test/v1/health",
        "/v1/a#token",
        "/v1/%252e/config",
    ] {
        assert!(
            actions::raw_path(&json!({"method":"GET","path":path})).is_err(),
            "{path}"
        );
    }
    for id in ["owner", "viewer"] {
        sqlx::query("INSERT INTO \"user\"(id,name,email,username,role,auth_complete) VALUES($1,$1,$2,$1,$3,true)").bind(id).bind(format!("{id}@test.invalid")).bind(if id=="owner"{"owner"}else{"member"}).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO session(id,user_id,token,expires_at) VALUES($1,$1,$2,now()+interval '7 days')").bind(id).bind(format!("{id}-token")).execute(&db).await.unwrap();
    }
    sqlx::query("INSERT INTO org_members(org_id,user_id,role) VALUES('org','viewer','member')")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO org_roles(id,org_id,name,capabilities) VALUES('viewer-role','org','Viewer','[\"server.view\",\"audit.read\"]')").execute(&db).await.unwrap();
    sqlx::query("INSERT INTO server_grants(server_id,user_id,role_id) VALUES('server','viewer','viewer-role')").execute(&db).await.unwrap();
    let viewer = format!(
        "warcon.session_token={}",
        identity_crypto::signed(&state.config.auth_secret, "viewer-token")
    );
    let owner = format!(
        "warcon.session_token={}",
        identity_crypto::signed(&state.config.auth_secret, "owner-token")
    );
    let app = api::router(state.clone());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/servers/server/rcon/configApply",
            json!({"text":"Password=do-not-log"}),
            &viewer
        )
        .await
        .0,
        403
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/servers/server/rcon/kick",
            json!({}),
            &owner
        )
        .await
        .0,
        405
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/servers/server/rcon/unknown",
            json!({}),
            &owner
        )
        .await
        .0,
        404
    );
    let cases: Vec<Value> = serde_json::from_str(include_str!("../fixtures/actions.json")).unwrap();
    let case = cases.iter().find(|c| c["name"] == "capabilities").unwrap();
    *fixture.0.lock().unwrap() = case["calls"].as_array().unwrap().iter().cloned().collect();
    let (status, response) = call(
        &app,
        "GET",
        "/api/servers/server/rcon/capabilities",
        json!({}),
        &viewer,
    )
    .await;
    assert_eq!(status, 200, "{response}");
    assert!(response["result"].get("raw").is_none());
    let case = cases.iter().find(|c| c["name"] == "serverLog").unwrap();
    *fixture.0.lock().unwrap() = case["calls"].as_array().unwrap().iter().cloned().collect();
    let (status, response) = call(
        &app,
        "GET",
        "/api/servers/server/rcon/serverLog?limit=40",
        json!({}),
        &viewer,
    )
    .await;
    assert_eq!(status, 200, "{response}");
    assert_eq!(response["result"]["entries"][0]["peer"], "");
    let lane = warcon_backend::dispatcher::acquire("server", 0, std::time::Duration::from_secs(1))
        .await
        .unwrap();
    let pending = tokio::spawn({
        let app = app.clone();
        let viewer = viewer.clone();
        async move {
            call(
                &app,
                "GET",
                "/api/servers/server/rcon/health",
                json!({}),
                &viewer,
            )
            .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    sqlx::query("DELETE FROM session WHERE user_id='viewer'")
        .execute(&db)
        .await
        .unwrap();
    drop(lane);
    assert_eq!(pending.await.unwrap().0, 401);
    assert!(fixture.0.lock().unwrap().is_empty());
    let secret:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_log WHERE detail::text LIKE '%do-not-log%' OR detail::text LIKE '%fixture-rcon-key%')").fetch_one(&db).await.unwrap();
    assert!(!secret);
    let permits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM faction_move_permits WHERE server_id='server'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(permits, 1);
    server.abort();
    db.close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
