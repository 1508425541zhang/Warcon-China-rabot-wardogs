mod common;
use axum::{
    Json, Router,
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    response::IntoResponse,
};
use common::Db;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;
use warcon_backend::{
    api,
    config::AppState,
    crypto, dispatcher, gateway,
    leadership::Leadership,
    observer::{self, Memory},
    relay, settings,
};
#[derive(Clone)]
struct Fixture {
    scene: Arc<Mutex<Value>>,
    requests: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}
async fn game(State(f): State<Fixture>, request: Request) -> Response<Body> {
    assert_eq!(request.headers()["authorization"], "Bearer fixture-key");
    f.requests.fetch_add(1, Ordering::SeqCst);
    let scene = f.scene.lock().unwrap().clone();
    let path = request.uri().path();
    if path == "/v1/status" && scene["failure"].as_u64().is_some() {
        return Response::builder()
            .status(scene["failure"].as_u64().unwrap() as u16)
            .header("retry-after", "2")
            .body(Body::from(
                json!({"error":{"code":"fixture_error","message":"fixture error"}}).to_string(),
            ))
            .unwrap();
    }
    let out = match path {
        "/v1/status" => {
            json!({"serverName":"Fixture","map":scene["map"],"experiences":["KOTH"],"lighting":"Day","players":{"current":scene["players"].as_array().unwrap().len(),"max":100},"matchSeconds":scene["seconds"],"scoreTick":{"current":1,"min":0,"max":4},"scoreCap":1000,"factionScores":[{"name":"Red","score":scene["score"],"colorHex":"#ff0000"},{"name":"Blue","score":0,"colorHex":"#0000ff"}],"rawOnly":{"headshotSource":true}})
        }
        "/v1/players" => json!({"players":scene["players"],"original":"raw-roster"}),
        "/v1/capabilities" => {
            json!({"build":"fixture-build","routes":["GET /v1/server-id","PATCH /v1/players/:id"],"config":{"writable":true}})
        }
        "/v1/server-id" => json!({"serverId":"join-code"}),
        "/v1/health" => json!({"uptimeSeconds":3600}),
        "/v1/config" => {
            json!({"revision":"revision-a","text":"[/Script/WDGame.WDGameSession]\nMaxReservedSlots=4\n","sections":[],"writable":true})
        }
        "/v1/bans" => json!({"bans":[]}),
        "/v1/reserved-slots" => json!({"reservedSlots":[]}),
        "/v1/broadcast" => {
            let active = f.active.fetch_add(1, Ordering::SeqCst) + 1;
            f.peak.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            f.active.fetch_sub(1, Ordering::SeqCst);
            json!({"ok":true})
        }
        p if p.ends_with("/kick") => json!({"ok":true}),
        _ => panic!("Unexpected path {path}"),
    };
    Json(out).into_response()
}
fn player(kills: i64, deaths: i64, cash: i64) -> Value {
    json!({"steamId":"76561198000000001","name":"Player","faction":"Red","kills":kills,"deaths":deaths,"cash":cash,"pingMs":40,"equipment":{"weapon":"Id.Item.AK74M"},"headshots":2})
}
async fn fixture(db: &Db) -> (Fixture, tokio::task::JoinHandle<()>) {
    let f = Fixture {
        scene: Arc::new(Mutex::new(
            json!({"map":"A","score":10,"seconds":60,"players":[player(3,1,500)]}),
        )),
        requests: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        peak: Arc::new(AtomicUsize::new(0)),
    };
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let app = Router::new().fallback(game).with_state(f.clone());
    let task = tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
    sqlx::query("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org')")
        .execute(&db.state.db)
        .await
        .unwrap();
    let secret = crypto::encrypt_secret(&db.state.config.encryption_key, "fixture-key").unwrap();
    sqlx::query("INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc,allow_private) VALUES('server','org','Server','127.0.0.1',$1,'http',$2,true),('hidden','org','Hidden','127.0.0.1',$1,'http',$2,true)").bind(port as i32).bind(secret).execute(&db.state.db).await.unwrap();
    (f, task)
}
async fn leader(state: &AppState) -> Arc<Leadership> {
    let leader = Arc::new(Leadership::new(state.db.clone(), "fixture-worker".into()));
    assert!(leader.renew().await.unwrap());
    assert!(state.runtime.leader.set(leader.clone()).is_ok());
    leader
}
#[tokio::test]
#[ignore = "Requires local PostgreSQL and creates an isolated development database"]
async fn native_observations_archive_raw_sessions_matches_hold_and_fencing() {
    let db = Db::new().await;
    let (f, game) = fixture(&db).await;
    let leader = leader(&db.state).await;
    let settings = settings::load(&db.state.db).await.unwrap();
    let mut m = Memory::new(
        "server".into(),
        "org".into(),
        "Server".into(),
        observer::now(),
        30000,
    );
    observer::observe(&db.state, &mut m, true, true, &settings)
        .await
        .unwrap();
    assert_eq!(m.reserved, Some(4));
    assert_eq!(m.build, "fixture-build");
    assert_eq!(m.game_id, "join-code");
    assert!(m.started_at > 0);
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM training_observations WHERE payload ? 'original' OR payload ? 'rawOnly'").fetch_one(&db.state.db).await.unwrap();
    assert_eq!(count, 2);
    let first: i64 = sqlx::query_scalar("SELECT id FROM matches WHERE ended_at IS NULL")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM player_sessions WHERE left_at IS NULL")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    f.scene.lock().unwrap()["players"] = json!([player(5, 2, 900)]);
    f.scene.lock().unwrap()["seconds"] = json!(90);
    m.players_at = observer::now() - 1000;
    m.players_interval = 1000;
    m.heartbeat_at = 0;
    observer::observe(&db.state, &mut m, true, true, &settings)
        .await
        .unwrap();
    let counters: (i32, i32, i32) =
        sqlx::query_as("SELECT kills,deaths,cash FROM player_sessions WHERE left_at IS NULL")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(counters, (5, 2, 900));
    // Counters drop on the player endpoint before the status endpoint reports the new map.
    f.scene.lock().unwrap()["players"] = json!([player(1, 0, 50)]);
    m.players_at = observer::now() - 1000;
    observer::observe(&db.state, &mut m, false, true, &settings)
        .await
        .unwrap();
    f.scene.lock().unwrap()["map"] = json!("B");
    f.scene.lock().unwrap()["score"] = json!(0);
    f.scene.lock().unwrap()["seconds"] = json!(0);
    m.players_at = observer::now() - 1000;
    observer::observe(&db.state, &mut m, true, true, &settings)
        .await
        .unwrap();
    let line: (i32, i32, i32) =
        sqlx::query_as("SELECT kills,deaths,cash_delta FROM match_players WHERE match_id=$1")
            .bind(first)
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(line, (5, 2, 400));
    let winner: Option<String> = sqlx::query_scalar("SELECT winner FROM matches WHERE id=$1")
        .bind(first)
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(winner.as_deref(), Some("Red"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM matches WHERE ended_at IS NULL")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    // A short missing roster is not a leave; after the grace it closes at lastSeen.
    let seen = m
        .sessions
        .as_ref()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .last_seen;
    f.scene.lock().unwrap()["players"] = json!([]);
    observer::observe(&db.state, &mut m, false, true, &settings)
        .await
        .unwrap();
    assert_eq!(m.sessions.as_ref().unwrap().len(), 1);
    m.sessions
        .as_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()
        .last_seen = seen - 61000;
    observer::observe(&db.state, &mut m, false, true, &settings)
        .await
        .unwrap();
    assert!(m.sessions.as_ref().unwrap().is_empty());
    f.scene.lock().unwrap()["failure"] = json!(429);
    observer::observe(&db.state, &mut m, true, true, &settings)
        .await
        .unwrap();
    assert!(m.ok);
    assert_eq!(m.failures, 0);
    assert!(m.hold > observer::now());
    f.scene.lock().unwrap()["failure"] = json!(503);
    for _ in 0..3 {
        observer::observe(&db.state, &mut m, true, true, &settings)
            .await
            .unwrap();
    }
    assert_eq!(m.tier, "offline");
    assert!(!m.ok);
    let before = f.requests.load(Ordering::SeqCst);
    leader.release().await.unwrap();
    assert_eq!(
        observer::observe(&db.state, &mut m, true, true, &settings)
            .await
            .unwrap_err()
            .code,
        "worker_ownership_lost"
    );
    assert_eq!(f.requests.load(Ordering::SeqCst), before);
    game.abort();
    db.close().await;
}
#[tokio::test]
#[ignore = "Requires local PostgreSQL and creates an isolated development database"]
async fn worker_relay_serializes_commands_checks_queued_sessions_and_never_retries() {
    let db = Db::new().await;
    let (f, game) = fixture(&db).await;
    let leader = leader(&db.state).await;
    let cookie = db.user("owner", true).await;
    let mut worker = db.state.clone();
    worker.config.worker.relay_secret = Some("fixture-relay-secret-at-least-32-bytes".into());
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let app = relay::router(worker);
    let task = tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
    let mut api_state = db.state.clone();
    api_state.runtime = Default::default();
    api_state.config.worker.relay_url = Some(format!("http://127.0.0.1:{port}"));
    api_state.config.worker.relay_secret = Some("fixture-relay-secret-at-least-32-bytes".into());
    let client = reqwest::Client::new();
    let reply = client
        .post(format!("http://127.0.0.1:{port}/relay/run"))
        .json(&json!({"serverId":"server","action":"broadcast","params":{"message":"test"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(reply.status(), 401);
    assert_eq!(f.requests.load(Ordering::SeqCst), 0);
    let first_params = json!({"message":"one"});
    let second_params = json!({"message":"two"});
    let (a, b) = tokio::join!(
        api::rcon_actions::system(&api_state, "server", "broadcast", &first_params, 0),
        api::rcon_actions::system(&api_state, "server", "broadcast", &second_params, 0)
    );
    assert!(a.is_ok(), "{a:?}");
    assert!(b.is_ok(), "{b:?}");
    assert_eq!(f.peak.load(Ordering::SeqCst), 1);
    assert_eq!(f.requests.load(Ordering::SeqCst), 2);
    let lane = dispatcher::acquire("server", 0, Duration::from_secs(1))
        .await
        .unwrap();
    let api_app = api::router(api_state.clone());
    let pending = tokio::spawn(async move {
        api_app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/servers/server/rcon/broadcast")
                    .header("content-type", "application/json")
                    .header("origin", "http://localhost:3000")
                    .header("cookie", cookie)
                    .body(Body::from(
                        json!({"message":"must not deliver"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    sqlx::query("DELETE FROM session WHERE user_id='owner'")
        .execute(&db.state.db)
        .await
        .unwrap();
    drop(lane);
    assert_eq!(pending.await.unwrap().status(), 401);
    assert_eq!(f.requests.load(Ordering::SeqCst), 2);
    leader.release().await.unwrap();
    let err = gateway::call(
        &api_state,
        "run",
        json!({"serverId":"server","action":"broadcast","params":{"message":"must not deliver"}}),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err,warcon_backend::game::Error::Api(e) if e.status==StatusCode::SERVICE_UNAVAILABLE)
    );
    assert_eq!(f.requests.load(Ordering::SeqCst), 2);
    task.abort();
    game.abort();
    db.close().await;
}
#[tokio::test]
#[ignore = "Requires local PostgreSQL and creates an isolated development database"]
async fn realtime_scopes_named_events_revocation_and_disconnect_cleanup() {
    let db = Db::new().await;
    let (_f, game) = fixture(&db).await;
    let owner = db.user("owner", true).await;
    let cookie = db.user("viewer", false).await;
    sqlx::raw_sql("INSERT INTO org_roles(id,org_id,name,capabilities) VALUES('view','org','Viewer','[\"server.view\"]'); INSERT INTO org_members(org_id,user_id,role) VALUES('org','viewer','member'); INSERT INTO server_grants(server_id,user_id,role_id) VALUES('server','viewer','view')").execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO server_live(server_id,ok,status,players,status_at,players_at,observed_at) VALUES('server',true,$1,'[]',now(),now(),now())").bind(json!({"serverName":"Server","map":"Map","playerCount":0})).execute(&db.state.db).await.unwrap();
    let (status, body) = db
        .call("GET", "/api/live?ids=server,hidden", json!({}), &cookie)
        .await;
    assert_eq!(status, 200);
    assert!(body["live"]["server"].is_object());
    assert!(body["live"]["hidden"].is_null());
    let response = db
        .app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/live/events?ids=server,hidden")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-accel-buffering"], "no");
    let mut stream = response.into_body().into_data_stream();
    let initial = stream.next().await.unwrap().unwrap();
    assert!(
        std::str::from_utf8(&initial)
            .unwrap()
            .contains("event: live")
    );
    db.state
        .runtime
        .emit(json!({"type":"outbox","serverId":"server","id":1,"state":"delivered"}));
    db.state
        .runtime
        .emit(json!({"type":"kills","serverId":"hidden","kills":[]}));
    db.state
        .runtime
        .emit(json!({"type":"kills","serverId":"server","kills":[{"eventId":"allowed"}]}));
    let event = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = std::str::from_utf8(&event).unwrap();
    assert!(text.contains("event: kills"));
    assert!(text.contains("allowed"));
    assert!(!text.contains("outbox"));
    sqlx::query("DELETE FROM server_grants WHERE user_id='viewer'")
        .execute(&db.state.db)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(7), stream.next())
            .await
            .unwrap()
            .is_none()
    );
    drop(stream);
    let response = db
        .app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/live/events?ids=server")
                .header("cookie", owner)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut stream = response.into_body().into_data_stream();
    stream.next().await.unwrap().unwrap();
    assert_eq!(db.state.runtime.events.receiver_count(), 1);
    drop(stream);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(db.state.runtime.events.receiver_count(), 0);
    game.abort();
    db.close().await;
}
