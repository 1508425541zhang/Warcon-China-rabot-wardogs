mod common;
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::Response,
};
use common::Db;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use warcon_backend::{
    crypto, leadership::Leadership, short_model::Model, short_observer as observer,
};
const PID: &str = "76561198000000001";
fn model() -> Model {
    Model::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../services/short-risk/artifacts-rust/isolation.json"),
    )
    .unwrap()
}
async fn game(State(calls): State<Arc<AtomicUsize>>, r: Request) -> Response<Body> {
    assert_eq!(r.headers()["authorization"], "Bearer fixture-key");
    assert!(r.uri().path().ends_with("/kick"));
    let n = calls.fetch_add(1, Ordering::SeqCst);
    Response::builder()
        .status(if n == 1 { 502 } else { 200 })
        .header("content-type", "application/json")
        .body(Body::from(if n == 1 {
            r#"{"error":{"code":"unknown_delivery","message":"fixture"}}"#
        } else {
            r#"{"ok":true}"#
        }))
        .unwrap()
}
fn snapshot(model: &Model, mid: i64, pid: &str) -> Value {
    let now = chrono::Utc::now();
    let at = |offset: i64| {
        (now + chrono::Duration::seconds(offset))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    json!({"model_sha256":model.source_model_sha256,"algorithm":"IsolationForest","evaluated_at":at(0),"players":[{"server_id":"server","player_id":pid,"name":"Player","anomaly_score":0.8,"percentile":99.99,"scope":["server","boot",mid.to_string(),"map"],"recent_windows":(-4..=0).map(|i|json!({"at":at(i*10),"score":0.8})).collect::<Vec<_>>()}]})
}
async fn record(db: &Db, pid: &str) -> Value {
    sqlx::query_scalar(
        "SELECT value FROM site_settings WHERE key LIKE 'shortRisk:server:'||$1||':%' LIMIT 1",
    )
    .bind(pid)
    .fetch_one(&db.state.db)
    .await
    .unwrap()
}
#[tokio::test]
#[ignore = "Requires local PostgreSQL; only isolated fixture data and local HTTP game commands"]
async fn short_actions_warn_protect_claim_audit_and_never_replay() {
    let db = Db::new().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let app = Router::new().fallback(game).with_state(calls.clone());
    let task = tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
    sqlx::query("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org')")
        .execute(&db.state.db)
        .await
        .unwrap();
    let secret = crypto::encrypt_secret(&db.state.config.encryption_key, "fixture-key").unwrap();
    sqlx::query("INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc,allow_private) VALUES('server','org','Server','127.0.0.1',$1,'http',$2,true)").bind(port as i32).bind(secret).execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO integrity_rules(org_id,config,assessment_mode,auto_action_max_per_hour,auto_action_max_percent_online) VALUES('org','{\"minimumOnlineForAutoAction\":1}','short_only',2,100)").execute(&db.state.db).await.unwrap();
    let mid:i64=sqlx::query_scalar("INSERT INTO matches(server_id,map,started_at) VALUES('server','map',now()-interval '10 minutes') RETURNING id").fetch_one(&db.state.db).await.unwrap();
    let players = json!(
        (1..=4)
            .map(|i| json!({"steamId":format!("7656119800000000{i}"),"name":"Player"}))
            .collect::<Vec<_>>()
    );
    sqlx::query("INSERT INTO server_live(server_id,ok,status_at,players_at,feed_at,players) VALUES('server',true,now(),now(),now(),$1)").bind(players).execute(&db.state.db).await.unwrap();
    let leader = Arc::new(Leadership::new(db.state.db.clone(), "fixture".into()));
    assert!(leader.renew().await.unwrap());
    assert!(db.state.runtime.leader.set(leader.clone()).is_ok());
    let model = model();
    let s = snapshot(&model, mid, PID);
    // Missing feed blocks a real action while still displaying a warning.
    sqlx::query("UPDATE server_live SET feed_at=NULL WHERE server_id='server'")
        .execute(&db.state.db)
        .await
        .unwrap();
    observer::actions(&db.state, &model, &s).await.unwrap();
    assert_eq!(record(&db, PID).await["state"], "warning");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    sqlx::query("UPDATE server_live SET feed_at=now() WHERE server_id='server'")
        .execute(&db.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO site_settings(key,value) VALUES('qqVip',$1)").bind(json!({"entries":[{"serverId":"server","steamId":PID,"enabled":true,"whitelist":true}]})).execute(&db.state.db).await.unwrap();
    observer::actions(&db.state, &model, &s).await.unwrap();
    assert_eq!(record(&db, PID).await["reason"], "VIP 白名单");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    sqlx::query("DELETE FROM site_settings WHERE key='qqVip'")
        .execute(&db.state.db)
        .await
        .unwrap();
    observer::actions(&db.state, &model, &s).await.unwrap();
    assert_eq!(record(&db, PID).await["state"], "delivered");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    observer::actions(&db.state, &model, &s).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let second = "76561198000000002";
    let s2 = snapshot(&model, mid, second);
    observer::actions(&db.state, &model, &s2).await.unwrap();
    assert_eq!(record(&db, second).await["state"], "unknown");
    observer::actions(&db.state, &model, &s2).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let third = "76561198000000003";
    observer::actions(&db.state, &model, &snapshot(&model, mid, third))
        .await
        .unwrap();
    assert_eq!(record(&db, third).await["reason"], "达到自动处罚频率上限");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_log WHERE action='shortRisk.kick'"
        )
        .fetch_one(&db.state.db)
        .await
        .unwrap(),
        2
    );
    // Inference writes its own snapshot only; it never claims/acks Feed consumer jobs.
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM feed_processing_jobs WHERE state='done'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    let mut history = observer::History::new();
    observer::tick(&db.state, Arc::new(model), &mut history, false)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM feed_processing_jobs WHERE state='done'"
        )
        .fetch_one(&db.state.db)
        .await
        .unwrap(),
        before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM integrity_model_runs")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        0
    );
    leader.release().await.unwrap();
    task.abort();
    db.close().await;
}
