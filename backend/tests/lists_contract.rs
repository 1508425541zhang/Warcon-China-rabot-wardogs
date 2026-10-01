mod common;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use common::Db;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use warcon_backend::{ban_enforcement, crypto, dispatcher, game, list_sync};
#[derive(Clone, Default)]
struct Mock {
    slots: Arc<Mutex<Vec<String>>>,
    calls: Arc<Mutex<Vec<(String, String)>>>,
    fail: Arc<Mutex<bool>>,
}
async fn game(State(m): State<Mock>, r: Request) -> Response {
    let path = r.uri().path().to_owned();
    let method = r.method().to_string();
    assert_eq!(r.headers()["authorization"], "Bearer fixture-key");
    m.calls.lock().unwrap().push((method.clone(), path.clone()));
    let body = to_bytes(r.into_body(), 100000).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let out = match (method.as_str(), path.as_str()) {
        ("GET", "/v1/bans") => json!({"bans":[]}),
        ("GET", "/v1/reserved-slots") => json!({"reservedSlots":m.slots.lock().unwrap().clone()}),
        ("GET", "/v1/capabilities") => json!({"routes":["POST /v1/reserved-slots"]}),
        ("POST", "/v1/reserved-slots") => {
            if *m.fail.lock().unwrap() {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":{"code":"unreachable","message":"Mock unavailable"}})),
                )
                    .into_response();
            }
            m.slots
                .lock()
                .unwrap()
                .push(body["steamId"].as_str().unwrap().into());
            json!({"ok":true})
        }
        ("POST", "/v1/players/76561198000000001/kick") => json!({"ok":true}),
        ("DELETE", p) if p.starts_with("/v1/reserved-slots/") => {
            m.slots
                .lock()
                .unwrap()
                .retain(|s| s != p.rsplit('/').next().unwrap());
            json!({"ok":true})
        }
        _ => panic!("Unexpected {method} {path}"),
    };
    Json(out).into_response()
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn list_permissions_history_expiry_adoption_sync_and_ban_delivery() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    let editor = db.user("editor", false).await;
    let viewer = db.user("viewer", false).await;
    let outsider = db.user("outsider", false).await;
    sqlx::query("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org'),('other','Other','other')").execute(&db.state.db).await.unwrap();
    let mock = Mock::default();
    mock.slots.lock().unwrap().push("76561198000000009".into());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let job = tokio::spawn({
        let m = mock.clone();
        async move {
            axum::serve(listener, Router::new().fallback(game).with_state(m))
                .await
                .unwrap()
        }
    });
    let secret = crypto::encrypt_secret(&db.state.config.encryption_key, "fixture-key").unwrap();
    sqlx::query("INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc,allow_private) VALUES('server','org','Server','127.0.0.1',$1,'http',$2,true)").bind(port as i32).bind(secret).execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO org_members(org_id,user_id,role) VALUES('org','editor','member'),('org','viewer','member')").execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO org_roles(id,org_id,name,capabilities) VALUES('edit','org','Editor','[\"server.view\",\"lists.reserve\",\"slots.manage\"]'),('view','org','Viewer','[\"server.view\"]')").execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO server_grants(server_id,user_id,role_id) VALUES('server','editor','edit'),('server','viewer','view')").execute(&db.state.db).await.unwrap();
    assert_eq!(
        db.call("GET", "/api/orgs/org/lists", json!({}), &outsider)
            .await
            .0,
        404
    );
    assert_eq!(
        db.call("GET", "/api/orgs/org/lists", json!({}), &viewer)
            .await
            .0,
        404
    );
    let (s, v) = db
        .call("GET", "/api/orgs/org/lists", json!({}), &editor)
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["kinds"], json!(["reserve"]));
    assert!(v["banMessage"].is_null());
    assert_eq!(v["lists"].as_array().unwrap().len(), 1);
    assert_eq!(
        db.call(
            "POST",
            "/api/orgs/org/lists/ban/entries",
            json!({"steamId":"76561198000000001"}),
            &editor
        )
        .await
        .0,
        403
    );
    let (s, v) = db
        .call(
            "POST",
            "/api/orgs/org/lists/reserve/entries",
            json!({"steamId":"76561198000000001","reason":"staff note"}),
            &editor,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    assert_eq!(v["entry"]["servers"][0]["state"], "applied");
    assert!(
        mock.slots
            .lock()
            .unwrap()
            .contains(&"76561198000000001".into())
    );
    assert!(
        mock.slots
            .lock()
            .unwrap()
            .contains(&"76561198000000009".into())
    );
    assert_eq!(
        db.call(
            "POST",
            "/api/orgs/org/lists/reserve/entries",
            json!({"steamId":"76561198000000001"}),
            &editor
        )
        .await
        .0,
        409
    );
    let (s, v) = db
        .call("GET", "/api/servers/server/lists/state", json!({}), &viewer)
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["reserved"]["76561198000000001"]["note"], "");
    assert_eq!(v["reserved"]["76561198000000009"]["managed"], false);
    let n = mock.calls.lock().unwrap().len();
    assert_eq!(
        db.call(
            "PATCH",
            "/api/orgs/org/lists/reserve/entries/76561198000000001",
            json!({"reason":"updated"}),
            &editor
        )
        .await
        .0,
        200
    );
    assert_eq!(mock.calls.lock().unwrap().len(), n);
    assert_eq!(
        db.call(
            "DELETE",
            "/api/orgs/org/lists/reserve/entries/76561198000000001",
            json!({}),
            &editor
        )
        .await
        .0,
        200
    );
    assert_eq!(
        *mock.slots.lock().unwrap(),
        vec!["76561198000000009".to_string()]
    );
    let (_, v) = db
        .call(
            "GET",
            "/api/orgs/org/lists/reserve/entries?includeRemoved=1",
            json!({}),
            &editor,
        )
        .await;
    assert_eq!(v["entries"][0]["removal"], "manual");
    assert!(!v["entries"][0]["removedAt"].is_null());
    let (_, c) = db
        .call("GET", "/api/orgs/org/lists/import", json!({}), &owner)
        .await;
    assert_eq!(c["candidates"][0]["steamId"], "76561198000000009");
    assert_eq!(
        db.call("GET", "/api/orgs/org/lists/import", json!({}), &editor)
            .await
            .0,
        403
    );
    let(s,v)=db.call("POST","/api/orgs/org/lists/import",json!({"entries":[{"kind":"reserve","steamId":"76561198000000009"},{"kind":"reserve","steamId":"76561198000000009"}]}),&owner).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["imported"], 1);
    assert_eq!(v["skipped"], 1);
    assert_eq!(
        db.call(
            "DELETE",
            "/api/orgs/org/lists/reserve/entries/76561198000000009",
            json!({}),
            &owner
        )
        .await
        .0,
        200
    );
    assert!(mock.slots.lock().unwrap().is_empty());
    *mock.fail.lock().unwrap() = true;
    let (s, v) = db
        .call(
            "POST",
            "/api/servers/server/lists/reserve/entries",
            json!({"steamId":"76561198000000002"}),
            &editor,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    assert!(!v["sync"]["error"].as_str().unwrap().is_empty());
    assert_eq!(
        mock.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, p)| m == "POST" && p == "/v1/reserved-slots")
            .count(),
        2
    );
    *mock.fail.lock().unwrap() = false;
    assert_eq!(
        db.call("POST", "/api/servers/server/lists/sync", json!({}), &editor)
            .await
            .0,
        200
    );
    assert!(
        mock.slots
            .lock()
            .unwrap()
            .contains(&"76561198000000002".into())
    );
    sqlx::query("UPDATE list_entries SET expires_at=now()-interval '1 second' WHERE steam_id='76561198000000002' AND removed_at IS NULL").execute(&db.state.db).await.unwrap();
    let expired = list_sync::expire_entries(&db.state).await.unwrap();
    assert_eq!(expired["lifted"], 1);
    assert_eq!(
        list_sync::expire_entries(&db.state).await.unwrap()["lifted"],
        0
    );
    db.call("POST", "/api/servers/server/lists/sync", json!({}), &editor)
        .await;
    assert!(mock.slots.lock().unwrap().is_empty());
    let (s, v) = db
        .call(
            "POST",
            "/api/orgs/org/lists/ban/entries",
            json!({"steamId":"76561198000000001","reason":"Review confirmed"}),
            &owner,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    let count = mock.calls.lock().unwrap().len();
    let _lane = dispatcher::acquire("server", 0, std::time::Duration::from_secs(5))
        .await
        .unwrap();
    let client = game::Client::for_server(&db.state, "server").await.unwrap();
    let mut retry = Default::default();
    assert_eq!(
        ban_enforcement::enforce(
            &db.state,
            &client,
            "server",
            &["76561198000000001".into()],
            &mut retry
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        ban_enforcement::enforce(
            &db.state,
            &client,
            "server",
            &["76561198000000001".into()],
            &mut retry
        )
        .await
        .unwrap(),
        0
    );
    assert_eq!(mock.calls.lock().unwrap().len(), count + 1);
    drop(_lane);
    sqlx::query("UPDATE organizations SET suspended_at=now() WHERE id='org'")
        .execute(&db.state.db)
        .await
        .unwrap();
    assert_eq!(
        db.call("GET", "/api/orgs/org/lists", json!({}), &editor)
            .await
            .0,
        403
    );
    let prior = mock.calls.lock().unwrap().len();
    assert!(
        !list_sync::reconcile(&db.state, "server", 0).await.unwrap()["ok"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(mock.calls.lock().unwrap().len(), prior);
    job.abort();
    db.close().await;
}
