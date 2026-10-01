mod common;
use axum::http::StatusCode;
use common::Db;
use serde_json::{Value, json};
use warcon_backend::integrity_reports;
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn reporters_require_verified_identity_and_freeze_without_punishment() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc)VALUES('server','org','Server','example.invalid',1234,'http','fixture');INSERT INTO player_sessions(server_id,steam_id,name,joined_at,last_seen)VALUES('server','76561198000000002','Target',now()-interval '10 minutes',now());").execute(&db.state.db).await.unwrap();
    let input = json!({"serverId":"server","target":"76561198000000002","reason":"请核验连续爆头"});
    assert_eq!(
        db.call("POST", "/api/reports", input.clone(), &owner)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("INSERT INTO account(id,account_id,provider_id,user_id)VALUES('steam','76561198000000001','steam','owner')").execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,killer_name,victim_steam_id,victim_name,cause,distance_m,headshot,tags)VALUES(now(),'server','event','boot','round',300,'Europe','76561198000000002','Target','76561198000000001','Reporter','Id.Item.AK74M',120,true,'[\"Penetration\"]')").execute(&db.state.db).await.unwrap();
    let (status, response) = db.call("POST", "/api/reports", input.clone(), &owner).await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
    let id = response["report"]["id"].as_i64().unwrap();
    let event: Value =
        sqlx::query_scalar("SELECT event FROM integrity_report_events WHERE report_id=$1")
            .bind(id)
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(event["cause"], "Id.Item.AK74M");
    assert_eq!(event["headshot"], true);
    assert_eq!(event["distanceM"], 120);
    assert_eq!(event["tags"], json!(["Penetration"]));
    assert!(event["ts"].is_string());
    assert_eq!(
        db.call("POST", "/api/reports", input, &owner).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM integrity_actions")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        0
    );
    let roster = vec![
        json!({"steamId":"1","name":"Same"}),
        json!({"steamId":"2","name":"Same"}),
    ];
    assert_eq!(
        integrity_reports::resolve("Same", &roster)
            .unwrap_err()
            .code,
        "multiple_targets"
    );
    db.close().await;
}
