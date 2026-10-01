mod common;
use chrono::{Duration, Utc};
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{
    api::kills::view, integrity_delivery, integrity_pipeline::Pipeline, integrity_score,
    outbox_worker,
};
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn expert_case_replay_idempotence_action_guards_and_unknown_delivery() {
    let db = Db::new().await;
    let at = chrono::DateTime::from_timestamp_millis(Utc::now().timestamp_millis()).unwrap()
        - Duration::seconds(2);
    let steam = "76561198000000001";
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org'); INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc) VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let match_id: i64 = sqlx::query_scalar(
        "INSERT INTO matches(server_id,started_at,map) VALUES('server',$1,'Europe') RETURNING id",
    )
    .bind(at - Duration::seconds(300))
    .fetch_one(&db.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO integrity_rules(org_id,config,assessment_mode,auto_kick_enabled) VALUES('org',$1,'statistical',true)").bind(integrity_score::defaults()).execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO integrity_model_state(org_id,active_baseline_generation,baseline_status,last_refresh_at) VALUES('org','g','READY',now())").execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO integrity_baselines(id,org_id,metric,level,weapon_category,source,sample_count,unique_players,unique_player_days,effective_sample_size,model_version,feature_version,weapon_map_version,generation,median,mad,p90,p95,p99,p995,p999,p9995,histogram,cdf,calculated_at) VALUES('baseline','org','kpm180',3,'INFANTRY','local',200,20,20,100,'ensemble-server-round-v3','rolling-infantry-v2',1,'g',0,1,0,0,0,0,0,0,'[]','[[0,200]]',now())").execute(&db.state.db).await.unwrap();
    let roster: Vec<_> = (1..=20)
        .map(|i| json!({"steamId":format!("765611980000000{i:02}"),"name":"Player","faction":"A"}))
        .collect();
    sqlx::query("INSERT INTO server_live(server_id,ok,players,players_at,status_at,feed_at,status) VALUES('server',true,$1,now(),now(),now(),'{}')").bind(json!(roster)).execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO player_sessions(server_id,steam_id,name,faction,joined_at,last_seen) VALUES('server',$1,'Player','A',$2,$3)").bind(steam).bind(at-Duration::seconds(300)).bind(at).execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO samples(server_id,ts,ok,player_count) VALUES('server',$1,true,20)")
        .bind(at)
        .execute(&db.state.db)
        .await
        .unwrap();
    let pipeline = Pipeline::default();
    let mut first = Vec::new();
    for i in 0..9 {
        first.push(kill(&db, at, match_id, i).await);
    }
    let outcome = pipeline
        .process(&db.state, "server", &first, true)
        .await
        .unwrap();
    assert!(!outcome.alerts.is_empty());
    assert!(outcome.actions.is_empty());
    let cases: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(c) FROM integrity_cases c WHERE org_id='org'")
            .fetch_all(&db.state.db)
            .await
            .unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0]["statistical"]["level"], "CASE");
    assert_eq!(cases[0]["statistical"]["committee"]["cheatVotes"], 2);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM integrity_scores")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    let replay = pipeline
        .process(&db.state, "server", &first, true)
        .await
        .unwrap();
    assert!(replay.alerts.is_empty());
    assert!(replay.actions.is_empty());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM integrity_scores")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        count
    );
    // A fresh process restores both rolling context and saved evidence identity.
    let restarted = Pipeline::default();
    assert!(
        restarted
            .process(&db.state, "server", &first, true)
            .await
            .unwrap()
            .alerts
            .is_empty()
    );
    let mut second = Vec::new();
    for i in 9..13 {
        second.push(kill(&db, at + Duration::milliseconds(1), match_id, i).await);
    }
    let next = pipeline
        .process(&db.state, "server", &second, true)
        .await
        .unwrap();
    assert_eq!(next.actions, vec!["KICK"]);
    let rows = outbox_worker::claim(&db.state, 60000).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        integrity_delivery::skip(&db.state, &rows[0])
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("UPDATE integrity_rules SET auto_kick_enabled=false WHERE org_id='org'")
        .execute(&db.state.db)
        .await
        .unwrap();
    assert!(
        integrity_delivery::skip(&db.state, &rows[0])
            .await
            .unwrap()
            .is_some()
    );
    // Simulate an interrupted sender. It becomes unknown and can never be reclaimed.
    sqlx::query("UPDATE outbox SET lease_until=now()-interval '1 second' WHERE id=$1")
        .bind(rows[0]["id"].as_i64())
        .execute(&db.state.db)
        .await
        .unwrap();
    assert!(
        outbox_worker::claim(&db.state, 60000)
            .await
            .unwrap()
            .is_empty()
    );
    let action: Value = sqlx::query_scalar("SELECT to_jsonb(a) FROM integrity_actions a LIMIT 1")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(action["delivery_state"], "unknown");
    assert!(action["effective_at"].is_null());
    db.close().await;
}
async fn kill(db: &Db, at: chrono::DateTime<Utc>, match_id: i64, i: i32) -> Value {
    let row:Value=sqlx::query_scalar("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,match_row,event_time,map,killer_steam_id,killer_name,killer_faction,victim_steam_id,victim_name,victim_faction,cause,distance_m,headshot,tags,faction_bracketed,faction_observed_at) VALUES($1,'server',$2,'boot','round',$3,$4,'Europe','76561198000000001','Player','A',$5,'Victim','B','Id.Item.AK74M',120,true,'[]',true,$1) RETURNING to_jsonb(kills)").bind(at).bind(format!("e{i}")).bind(match_id).bind(300.+i as f32).bind(format!("765611980000000{:02}",i+2)).fetch_one(&db.state.db).await.unwrap();
    view(row)
}
