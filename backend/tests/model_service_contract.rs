use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgConnectOptions};
use tower::ServiceExt;
use warcon_backend::{
    migrations,
    model::Predictor,
    model_service::{self, ModelState},
};
const TOKEN: &str = "synthetic-model-test-key-at-least-32-characters";
// A zero-weight fixture verifies HTTP/database orchestration without requiring
// a private trained checkpoint in CI. Trained checkpoint parity is tested by model_fixture.
fn test_artifacts(root: &std::path::Path) {
    std::fs::create_dir(root).unwrap();
    let mut tensors = serde_json::Map::new();
    let mut count = 0usize;
    let mut add = |name: String, shape: Vec<usize>| {
        let length = shape.iter().product::<usize>();
        tensors.insert(name, json!({"shape":shape,"offset":count,"length":length}));
        count += length;
    };
    add(
        "embedding.value_embedding.tokenConv.weight".into(),
        vec![128, 4, 3],
    );
    add("embedding.position_embedding.pe".into(), vec![1, 60, 128]);
    for l in 0..2 {
        let prefix = format!("encoder.attn_layers.{l}");
        for n in [
            "query_projection",
            "key_projection",
            "value_projection",
            "out_projection",
        ] {
            add(format!("{prefix}.attention.{n}.weight"), vec![128, 128]);
            add(format!("{prefix}.attention.{n}.bias"), vec![128]);
        }
        for n in ["norm1", "norm2"] {
            add(format!("{prefix}.{n}.weight"), vec![128]);
            add(format!("{prefix}.{n}.bias"), vec![128]);
        }
        add(format!("{prefix}.conv1.weight"), vec![256, 128, 1]);
        add(format!("{prefix}.conv1.bias"), vec![256]);
        add(format!("{prefix}.conv2.weight"), vec![128, 256, 1]);
        add(format!("{prefix}.conv2.bias"), vec![128]);
    }
    add("encoder.norm.weight".into(), vec![128]);
    add("encoder.norm.bias".into(), vec![128]);
    add("projection.weight".into(), vec![4, 128]);
    add("projection.bias".into(), vec![4]);
    let index = serde_json::to_vec(&json!({"tensors":tensors})).unwrap();
    let weights = vec![0u8; count * 4];
    let contract=serde_json::to_vec(&json!({"features":["small_arm_kills_60s","headshot_rate_60s"],"feature_groups":["combat","combat"],"window_steps":60,"center":[0,0],"scale":[1,1],"base":{"features":["small_arm_kills_60s","headshot_rate_60s"],"weapon_vocabulary":["Id.Item.AK74M"],"distance_baseline":{}},"distance_baseline":{},"weapon_class_by_id":{"Id.Item.AK74M":"automatic"}})).unwrap();
    let calibration = b"{}";
    let hash = |v: &[u8]| hex::encode(Sha256::digest(v));
    let manifest = json!({"model_id":"fixture","checkpoint_sha256":"test","feature_schema":"fixture-v1","epoch":0,"channels":4,"features":2,"window_steps":60,"bucket_seconds":30,"weights_sha256":hash(&weights),"weights_index_sha256":hash(&index),"scaler_sha256":hash(&contract),"calibration_sha256":hash(calibration),"extra_identity":"preserved"});
    for (n, data) in [
        ("weights.f32", weights),
        ("weights.json", index),
        ("feature_scaler.json", contract),
        ("calibration.json", calibration.to_vec()),
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
    ] {
        std::fs::write(root.join(n), data).unwrap();
    }
}
async fn req(
    app: axum::Router,
    method: &str,
    path: &str,
    token: &str,
    bytes: Vec<u8>,
) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(bytes))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    (
        status,
        serde_json::from_slice(
            &to_bytes(response.into_body(), 10 * 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap(),
    )
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn registered_sources_read_only_and_authenticated_native_inference() {
    let options: PgConnectOptions = std::env::var("TEST_DATABASE_URL").unwrap().parse().unwrap();
    let admin = PgPool::connect_with(options.clone()).await.unwrap();
    let database = format!("warcon_rust_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let db = PgPool::connect_with(options.database(&database))
        .await
        .unwrap();
    migrations::migrate(
        &db,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../drizzle"),
    )
    .await
    .unwrap();
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('model-org','Model','model');INSERT INTO servers(id,org_id,name,host,port,password_enc) VALUES('model-server','model-org','Model','example.org',80,'fixture')").execute(&db).await.unwrap();
    let match_id:i64=sqlx::query_scalar("INSERT INTO matches(server_id,started_at,map) VALUES('model-server','2026-09-01T00:00:00Z','TestMap') RETURNING id").fetch_one(&db).await.unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO integrity_model_runs(id,org_id,server_id,steam_id,match_id,slot,config_revision,threshold,created_at) VALUES($1,'model-org','model-server','76561198000000001',$2,1,'fixture',99.9,'2026-09-01T00:30:30Z')").bind(&id).bind(match_id).execute(&db).await.unwrap();
    for t in 0..=61 {
        let ts = chrono::DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
            + chrono::Duration::seconds(t * 30);
        sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,match_row,event_time,map,killer_steam_id,killer_name,victim_steam_id,victim_name,cause,headshot,tags) VALUES($1,'model-server',$2,'boot','round',$3,$4,'TestMap','76561198000000001','Player','76561198000000002','Victim','Id.Item.AK74M',$5,'[]')").bind(ts).bind(format!("e{t}")).bind(match_id).bind((t*30)as f64).bind(t%2==0).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO training_feed_batches(server_id,received_at,instance_id,payload) VALUES('model-server',$1,'boot',$2)").bind(ts).bind(json!({"events":[{"eventId":format!("e{t}"),"contextTags":if t%2==0{vec!["Event.Headshot"]}else{vec![]}}]})).execute(&db).await.unwrap();
    }
    let players = json!([{"steamId":"76561198000000001","name":"Redacted","kills":61},{"steamId":"76561198000000002","name":"Another","kills":1}]);
    sqlx::query("INSERT INTO training_observations(server_id,poll_started_at,received_at,endpoint,payload) VALUES('model-server','2026-09-01T00:30:00Z','2026-09-01T00:30:00Z','/v1/players',$1)").bind(&players).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO player_progress_samples(server_id,match_id,bucket,observed_at,players) VALUES('model-server',$1,1,'2026-09-01T00:30:00Z',$2)").bind(match_id).bind(&players).execute(&db).await.unwrap();
    let body = json!({"schema":"fixture-v1","requestId":id,"sources":{"matches":[{"id":match_id}],"player":"ignored arbitrary value"}});
    let source = model_service::read_source(&db, &body).await.unwrap();
    assert_eq!(source["player"], "76561198000000001");
    assert_eq!(source["observations"][0]["payload"]["roster_size"], 2);
    assert_eq!(
        source["observations"][0]["payload"]["players"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        source["observations"][0]["payload"]["players"][0]
            .get("name")
            .is_none()
    );
    assert!(source["progress"][0]["players"][0].get("name").is_none());
    let mut wrong = body.clone();
    wrong["sources"]["matches"][0]["id"] = json!(match_id + 1);
    assert!(model_service::read_source(&db, &wrong).await.is_err());
    let mut as_string = body.clone();
    as_string["sources"]["matches"][0]["id"] = json!(match_id.to_string());
    assert!(model_service::read_source(&db, &as_string).await.is_ok());
    let root = std::env::temp_dir().join(format!("warcon-model-fixture-{}", uuid::Uuid::new_v4()));
    test_artifacts(&root);
    let predictor = Predictor::load(&root).unwrap();
    let state = ModelState::new(predictor, db.clone(), TOKEN).unwrap();
    let app = model_service::router(state.clone());
    assert_eq!(
        req(app.clone(), "GET", "/v1/health", "wrong", vec![])
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, health) = req(app.clone(), "GET", "/v1/health", TOKEN, vec![]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["extra_identity"], "preserved");
    assert_eq!(
        req(app.clone(), "GET", "/v1/assess", TOKEN, vec![]).await.0,
        StatusCode::NOT_FOUND
    );
    let permit = state.busy.clone().try_acquire_owned().unwrap();
    assert_eq!(
        req(
            app.clone(),
            "POST",
            "/v1/assess",
            TOKEN,
            serde_json::to_vec(&body).unwrap()
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(permit);
    assert_eq!(
        req(app.clone(), "POST", "/v1/assess", TOKEN, vec![])
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        req(
            app.clone(),
            "POST",
            "/v1/assess",
            TOKEN,
            vec![b' '; 8 * 1024 * 1024 + 1]
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        req(app.clone(), "POST", "/v1/assess", TOKEN, vec![255])
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        req(
            app.clone(),
            "POST",
            "/v1/assess",
            TOKEN,
            serde_json::to_vec(&wrong).unwrap()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, result) = req(
        app.clone(),
        "POST",
        "/v1/assess",
        TOKEN,
        serde_json::to_vec(&body).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "READY");
    assert_eq!(result["observedBuckets"], 60);
    assert!(result["score"].as_f64().unwrap().is_finite());
    assert_eq!(result["pointScores"].as_array().unwrap().len(), 60);
    let run_state: String =
        sqlx::query_scalar("SELECT state FROM integrity_model_runs WHERE id=$1")
            .bind(&id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        run_state, "pending",
        "inference must never complete or punish a job itself"
    );
    drop(app);
    drop(state);
    db.close().await;
    sqlx::query(&format!("DROP DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}
