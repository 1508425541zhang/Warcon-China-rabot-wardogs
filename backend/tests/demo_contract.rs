mod common;
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{game::Client, observer};

#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn demo_creation_polling_and_dual_consumer_feed_without_game_server() {
    let db = Db::new().await;
    let owner = db.user("demoowner", true).await;
    let (status, body) = db
        .call("POST", "/api/orgs", json!({"name":"Demo fixture"}), &owner)
        .await;
    assert_eq!(status, 201, "{body}");
    let org = body["id"].as_str().unwrap().to_owned();
    let (status, body) = db
        .call(
            "POST",
            "/api/servers",
            json!({"orgId":org,"name":"Native demo","host":"demo","port":7776,"password":"demo"}),
            &owner,
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let server = body["id"].as_str().unwrap().to_owned();
    let client = Client::for_server(&db.state, &server).await.unwrap();
    let players = client.json("GET", "/v1/players", None).await.unwrap();
    assert!(players["players"].as_array().unwrap().len() >= 9);
    let mut memory = observer::Memory::new(
        server.clone(),
        org,
        "Native demo".into(),
        observer::now(),
        1,
    );
    let settings = warcon_backend::settings::defaults();
    observer::observe(&db.state, &mut memory, true, true, &settings)
        .await
        .unwrap();
    assert!(memory.ok);
    assert!(memory.players.len() >= 9);
    // Queued demo kills are discarded while Feed is disabled, just like the source.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM kills WHERE server_id=$1")
            .bind(&server)
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE servers SET feed_token_hash='fixture' WHERE id=$1")
        .bind(&server)
        .execute(&db.state.db)
        .await
        .unwrap();
    // Bounded polling exercises stochastic real simulator events, not synthetic DB inserts.
    let mut total = 0;
    for _ in 0..100 {
        client.json("GET", "/v1/players", None).await.unwrap();
        warcon_backend::mockgame::ingest_queued(&db.state, &server)
            .await
            .unwrap();
        total = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM kills WHERE server_id=$1")
            .bind(&server)
            .fetch_one(&db.state.db)
            .await
            .unwrap();
        if total > 0 {
            break;
        }
    }
    assert!(total > 0, "demo did not produce a kill in 100 ticks");
    let consumers: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT consumer FROM feed_processing_jobs WHERE server_id=$1 ORDER BY consumer",
    )
    .bind(&server)
    .fetch_all(&db.state.db)
    .await
    .unwrap();
    assert_eq!(consumers, vec!["integrity", "legacy"]);
    let (_, listing) = db.call("GET", "/api/servers", Value::Null, &owner).await;
    assert_eq!(listing["servers"][0]["demo"], true);
    db.close().await;
}
