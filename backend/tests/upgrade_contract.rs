mod common;
use serde_json::Value;
use warcon_backend::migrations;
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn old_single_consumer_jobs_upgrade_without_replaying_actions() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../drizzle");
    let journal: Value =
        serde_json::from_slice(&std::fs::read(folder.join("meta/_journal.json")).unwrap()).unwrap();
    let cutoff = journal["entries"]
        .as_array()
        .unwrap()
        .iter()
        .position(|e| e["tag"] == "0047_calm_vulcan")
        .unwrap();
    let db = common::Db::new_at(cutoff).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Upgrade','upgrade');INSERT INTO servers(id,org_id,name,host,port,password_enc)VALUES('server','org','Server','example.invalid',1,'fixture');INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,victim_steam_id,victim_name,tags)VALUES(date_trunc('second',now()),'server','kept','boot','round',100,'Europe','76561198000000001','76561198000000002','Victim','[]');INSERT INTO feed_processing_jobs(server_id,kill_ts,event_ids,created_at,state,attempts)SELECT 'server',ts,'[\"kept\"]',now(),'done',3 FROM kills WHERE event_id='kept';INSERT INTO feed_processing_jobs(server_id,kill_ts,event_ids,created_at,state,attempts)VALUES('server',now(),'[\"missing\"]',now(),'done',1),('server',now(),'[\"too-old\"]',now()-interval '31 days','done',2)").execute(&db.state.db).await.unwrap();
    let applied = migrations::migrate(&db.state.db, &folder).await.unwrap();
    assert_eq!(
        applied,
        journal["entries"].as_array().unwrap().len() - cutoff
    );
    let rows: Vec<(String, String, i32, Value, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT consumer,state,attempts,event_ids,created_at FROM feed_processing_jobs ORDER BY id",
    )
    .fetch_all(&db.state.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 4);
    assert!(rows[..3].iter().all(|r| r.0 == "legacy" && r.1 == "done"));
    assert_eq!(rows[0].2, 3);
    let integrity = &rows[3];
    assert_eq!(
        (&integrity.0, &integrity.1, integrity.2),
        (&"integrity".to_owned(), &"pending".to_owned(), 0)
    );
    assert_eq!(integrity.3, serde_json::json!(["kept"]));
    assert!(integrity.4 < chrono::Utc::now() - chrono::Duration::minutes(5));
    assert_eq!(migrations::migrate(&db.state.db, &folder).await.unwrap(), 0);
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM feed_processing_jobs WHERE consumer='integrity'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(n, 1);
    let actions: i64 = sqlx::query_scalar("SELECT count(*) FROM integrity_actions")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(actions, 0);
    db.close().await;
}
