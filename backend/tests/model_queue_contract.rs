mod common;
use axum::{Json, Router, routing::post};
use chrono::{Duration, Utc};
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{crypto, integrity_score, model_http, model_queue};
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn long_queue_scope_deduplication_fingerprint_and_atomic_kick_intent() {
    let db = Db::new().await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org'); INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc) VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    sqlx::query(
        "INSERT INTO integrity_rules(org_id,config,assessment_mode) VALUES('org',$1,'long_only')",
    )
    .bind(integrity_score::defaults())
    .execute(&db.state.db)
    .await
    .unwrap();
    let steam = "76561198000000001";
    let now = Utc::now();
    let mid: i64 = sqlx::query_scalar(
        "INSERT INTO matches(server_id,started_at,map) VALUES('server',$1,'Europe') RETURNING id",
    )
    .bind(now - Duration::minutes(32))
    .fetch_one(&db.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO server_live(server_id,ok,players,players_at,status_at) VALUES('server',true,$1,now(),now())").bind(json!([{"steamId":steam,"name":"Name","faction":"A"}])).execute(&db.state.db).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/v1/assess",post(move|headers:axum::http::HeaderMap,Json(request):Json<Value>|async move{
        assert_eq!(headers["authorization"],"Bearer fixture-model-key-at-least-thirty-two");
        assert_eq!(request["sources"]["matches"][0]["id"],mid);assert!(request["sources"].get("player_progress_samples").is_some());assert!(request["sources"].get("integrity_windows").is_some());assert!(request["sources"].get("integrity_player_metric_history").is_some());
        let m=model_http::manifest();let c=model_http::calibration();let score=(c["p98"].as_f64().unwrap()+c["p99"].as_f64().unwrap())/2.;
        Json(json!({"modelId":m["model_id"],"checkpointSha256":m["checkpoint_sha256"],"schema":m["feature_schema"],"requestId":request["requestId"],"calibrationSha256":m["calibration_sha256"],"windowSeconds":1800,"bucketSeconds":30,"status":"READY","score":score,"pointScores":vec![score;60]}))
    }))).await.unwrap();
    });
    let token = crypto::encrypt_secret(
        &db.state.config.encryption_key,
        "fixture-model-key-at-least-thirty-two",
    )
    .unwrap();
    let c = json!({"revision":"r1","developerEnabled":true,"autoPunishEnabled":true,"url":format!("http://127.0.0.1:{port}"),"tokenEnc":token,"maxActionsPerHour":10,"cooldownSeconds":600,"intervalSeconds":1800});
    sqlx::query("INSERT INTO site_settings(key,value) VALUES('integrityModel:org',$1)")
        .bind(c)
        .execute(&db.state.db)
        .await
        .unwrap();
    model_queue::schedule(&db.state, "org", "server", steam, mid, now)
        .await
        .unwrap();
    model_queue::schedule(&db.state, "org", "server", steam, mid, now)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM integrity_model_runs")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    assert!(model_queue::process_next(&db.state).await.unwrap());
    assert!(!model_queue::process_next(&db.state).await.unwrap());
    let run: Value = sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_model_runs r")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(run["state"], "READY");
    assert_eq!(run["action"], "KICK");
    assert_eq!(run["action_state"], "pending");
    assert!(!run["punished_at"].is_null());
    let id = run["id"].as_str().unwrap();
    assert!(model_queue::enforce(&db.state, id).await.unwrap().is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    // A pending job under an old revision never reaches the remote model.
    sqlx::query("INSERT INTO integrity_model_runs(id,org_id,server_id,steam_id,match_id,slot,config_revision,threshold) VALUES($1,'org','server',$2,$3,99,'old',1)").bind(uuid::Uuid::new_v4().to_string()).bind(steam).bind(mid).execute(&db.state.db).await.unwrap();
    assert!(!model_queue::process_next(&db.state).await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM integrity_model_runs WHERE config_revision='old'"
        )
        .fetch_one(&db.state.db)
        .await
        .unwrap(),
        "superseded"
    );
    server.abort();
    db.close().await;
}
