use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::StatusCode,
    routing::get,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use warcon_backend::{
    rcon::{self, GameRequest},
    steam::SteamClient,
};

async fn listener(router: Router) -> (u16, tokio::task::JoinHandle<()>) {
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let handle = tokio::spawn(async move { axum::serve(socket, router).await.unwrap() });
    (port, handle)
}
#[tokio::test]
async fn game_503_and_disconnect_do_not_retry_commands() {
    let count = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/v1/kick",
            axum::routing::post(|State(count): State<Arc<AtomicUsize>>| async move {
                count.fetch_add(1, Ordering::SeqCst);
                (StatusCode::SERVICE_UNAVAILABLE, "busy")
            }),
        )
        .with_state(count.clone());
    let (port, server) = listener(app).await;
    let target = rcon::resolve_target("127.0.0.1", port, "http", true)
        .await
        .unwrap();
    let request = GameRequest {
        method: "POST".into(),
        path: "/v1/kick".into(),
        headers: Default::default(),
        body: Some("{}".into()),
        timeout_ms: Some(1000),
        insecure_tls: false,
    };
    let response = rcon::game_request(&target, &request).await.unwrap();
    assert_eq!(response.status, 503);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let bad = GameRequest {
        path: "//example.com/steal".into(),
        ..request
    };
    assert!(rcon::game_request(&target, &bad).await.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
    // Close a socket after receiving a request, before answering: never replay to a second IP.
    use tokio::io::AsyncReadExt;
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = socket.accept().await.unwrap();
        let mut buffer = [0; 8192];
        let n = stream.read(&mut buffer).await.unwrap();
        assert!(n > 0);
        drop(stream);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), socket.accept())
                .await
                .is_err()
        );
    });
    let mut target = rcon::resolve_target("127.0.0.1", port, "http", true)
        .await
        .unwrap();
    target.addresses = Some(vec![
        "127.0.0.1".parse().unwrap(),
        "127.0.0.1".parse().unwrap(),
    ]);
    let request = GameRequest {
        method: "POST".into(),
        path: "/v1/kick".into(),
        headers: Default::default(),
        body: Some("{}".into()),
        timeout_ms: Some(1000),
        insecure_tls: false,
    };
    assert!(rcon::game_request(&target, &request).await.is_err());
    task.await.unwrap();
}
#[tokio::test]
async fn steam_summaries_bans_and_rate_limits_are_independent() {
    async fn reply(OriginalUri(uri): OriginalUri) -> Json<Value> {
        let q = uri.query().unwrap_or("");
        assert!(q.contains("key=fixture-secret"));
        assert!(q.contains("steamids=76561198388453389"));
        if uri.path().contains("Summaries") {
            Json(
                json!({"response":{"players":[{"steamid":"76561198388453389","personaname":"测试玩家","communityvisibilitystate":3,"timecreated":1500000000,"avatar":"avatar"}]}}),
            )
        } else {
            Json(
                json!({"players":[{"SteamId":"76561198388453389","NumberOfVACBans":2,"NumberOfGameBans":1,"DaysSinceLastBan":8,"CommunityBanned":false,"EconomyBan":"none"}]}),
            )
        }
    }
    let (port, server) = listener(
        Router::new()
            .route("/ISteamUser/GetPlayerSummaries/v2/", get(reply))
            .route("/ISteamUser/GetPlayerBans/v1/", get(reply)),
    )
    .await;
    let client = SteamClient::loopback_fixture(
        "fixture-secret".into(),
        format!("http://127.0.0.1:{port}/").parse().unwrap(),
    )
    .unwrap();
    let profile = client.profile("76561198388453389").await.unwrap();
    assert_eq!(profile.persona, "测试玩家");
    assert_eq!(profile.vac_bans, 2);
    assert_eq!(profile.game_bans, 1);
    assert_eq!(profile.days_since_last_ban, Some(8));
    assert!(profile.public);
    assert!(profile.error.is_empty());
    server.abort();
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .fallback(|State(calls): State<Arc<AtomicUsize>>| async move {
            calls.fetch_add(1, Ordering::SeqCst);
            StatusCode::TOO_MANY_REQUESTS
        })
        .with_state(calls.clone());
    let (port, server) = listener(app).await;
    let client = SteamClient::loopback_fixture(
        "fixture-secret".into(),
        format!("http://127.0.0.1:{port}/").parse().unwrap(),
    )
    .unwrap();
    let error = client.profile("76561198388453389").await.unwrap_err();
    assert!(!error.to_string().contains("fixture-secret"));
    let before = calls.load(Ordering::SeqCst);
    assert!(client.profile("76561198388453389").await.is_err());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        before,
        "429 must prevent further external calls during backoff"
    );
    server.abort();
}
