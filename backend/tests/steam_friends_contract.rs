mod common;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
async fn steam(
    State(c): State<Arc<AtomicUsize>>,
    Query(q): Query<HashMap<String, String>>,
    r: axum::extract::Request,
) -> Response {
    c.fetch_add(1, Ordering::SeqCst);
    if r.uri().path().contains("GetPlayerBans") {
        return Json(json!({"players":[{"SteamId":"76561198000000002","NumberOfVACBans":1},{"SteamId":"76561198000000003","NumberOfGameBans":1},{"SteamId":"76561198000000004","NumberOfVACBans":0}]})).into_response();
    }
    if q.get("steamid").is_some_and(|s| s.ends_with('9')) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({"friendslist":{"friends":[{"steamid":"76561198000000002"},{"steamid":"76561198000000002"},{"steamid":"invalid"},{"steamid":"76561198000000003"},{"steamid":"76561198000000004"}]}})).into_response()
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn native_friends_dedup_private_staleness_and_budget() {
    let db = common::Db::new().await;
    let count = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn({
        let c = count.clone();
        async move {
            axum::serve(listener, Router::new().fallback(steam).with_state(c))
                .await
                .unwrap()
        }
    });
    let client = warcon_backend::steam::SteamClient::loopback_fixture(
        "fixture".into(),
        format!("http://127.0.0.1:{port}/").parse().unwrap(),
    )
    .unwrap();
    let id = "76561198000000001";
    sqlx::query("INSERT INTO steam_profiles(steam_id)VALUES($1),('76561198000000009')")
        .bind(id)
        .execute(&db.state.db)
        .await
        .unwrap();
    warcon_backend::steam::refresh_friends(&db.state, &client, id)
        .await
        .unwrap();
    let v: Value = sqlx::query_scalar("SELECT to_jsonb(p)FROM steam_profiles p WHERE steam_id=$1")
        .bind(id)
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(v["friends_state"], "public");
    assert_eq!(v["friends_total"], 3);
    assert_eq!(v["friends_checked"], 3);
    assert_eq!(v["banned_friends"], 2);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    warcon_backend::steam::refresh_friends(&db.state, &client, id)
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    warcon_backend::steam::refresh_friends(&db.state, &client, "76561198000000009")
        .await
        .unwrap();
    let s: String = sqlx::query_scalar(
        "SELECT friends_state FROM steam_profiles WHERE steam_id='76561198000000009'",
    )
    .fetch_one(&db.state.db)
    .await
    .unwrap();
    assert_eq!(s, "private");
    sqlx::query(
        "UPDATE steam_profiles SET friends_checked_at=now()-interval '8 days'WHERE steam_id=$1",
    )
    .bind(id)
    .execute(&db.state.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE site_settings SET value='19998'::jsonb WHERE key LIKE 'rust:steamFriendBudget:%'",
    )
    .execute(&db.state.db)
    .await
    .unwrap();
    warcon_backend::steam::refresh_friends(&db.state, &client, id)
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 3);
    task.abort();
    db.close().await;
}
