mod common;
use chrono::{Duration, Utc};
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{integrity_baseline_db, integrity_career};
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn replay_provenance_clean_episode_exclusion_and_atomic_refresh() {
    let db = Db::new().await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org'); INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc) VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let at = Utc::now() - Duration::hours(2);
    for i in 0..180 {
        let received = at + Duration::seconds(i);
        let event = format!("e{i}");
        sqlx::query(
            "INSERT INTO samples(server_id,ts,ok,player_count) VALUES('server',$1,true,25)",
        )
        .bind(received)
        .execute(&db.state.db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,match_row,event_time,map,killer_steam_id,killer_name,killer_faction,victim_steam_id,victim_name,victim_faction,cause,distance_m,headshot,tags,faction_bracketed,faction_observed_at) VALUES($1,'server',$2,'boot','round',9,$3,'Europe',$6,'Name','A',$4,'Victim','B','Id.Item.AK74M',123,$5,'[]',true,$1)").bind(received).bind(&event).bind(i as f32).bind(format!("765611980000000{:02}",i%10+2)).bind(i%3==0).bind(if i%2==0{"76561198000000001"}else{"76561198000000020"}).execute(&db.state.db).await.unwrap();
        sqlx::query("INSERT INTO feed_processing_jobs(server_id,kill_ts,event_ids,consumer,state,attempts,created_at,done_at) VALUES('server',$1,$2,'integrity','done',1,$1,$1)").bind(received).bind(json!([event])).execute(&db.state.db).await.unwrap();
    }
    let count = integrity_baseline_db::refresh(&db.state, "org")
        .await
        .unwrap();
    assert!(count > 0);
    let state: Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id='org'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(state["baseline_status"], "READY");
    let generation = state["active_baseline_generation"].clone();
    let mut conn = db.state.db.acquire().await.unwrap();
    let career = integrity_career::load(&mut conn, "org", "76561198000000001", Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert!(career["sampleCount"].as_u64().unwrap() >= 1);
    drop(conn);
    // Exclude one exact episode's event, not the player's unrelated clean history.
    sqlx::query("INSERT INTO integrity_cases(id,org_id,server_id,steam_id,created_at,confidence,trigger,rule_version,risk_score,risk_breakdown,snapshot) VALUES('case','org','server','76561198000000001',now(),'B','test',1,40,'[]','{}')").execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO integrity_case_events(case_id,instance_id,event_id,event) VALUES('case','boot','e1',$1)").bind(json!({"killerSteamId":"76561198000000001","ts":at+Duration::seconds(1)})).execute(&db.state.db).await.unwrap();
    assert!(
        integrity_baseline_db::refresh(&db.state, "org")
            .await
            .unwrap()
            > 0
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM integrity_baselines WHERE org_id='org'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert!(count > 0);
    let updated: Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id='org'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_ne!(generation, updated["active_baseline_generation"]);
    sqlx::query("DELETE FROM feed_processing_jobs WHERE consumer='integrity'")
        .execute(&db.state.db)
        .await
        .unwrap();
    assert!(
        integrity_baseline_db::refresh(&db.state, "org")
            .await
            .is_err()
    );
    let final_state: Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id='org'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(final_state["baseline_status"], "READY");
    assert_eq!(
        final_state["active_baseline_generation"],
        updated["active_baseline_generation"]
    );
    db.close().await;
}
