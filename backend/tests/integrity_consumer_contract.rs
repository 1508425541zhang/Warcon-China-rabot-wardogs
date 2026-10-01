mod common;
use chrono::{Duration, Utc};
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{
    feed_jobs::{self, Consumer},
    integrity_consumer,
    leadership::Leadership,
};
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn persisted_jobs_reconcile_exact_roster_brackets_and_keep_lanes_independent() {
    let db = Db::new().await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc)VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let at = chrono::DateTime::from_timestamp_millis(Utc::now().timestamp_millis()).unwrap()
        - Duration::seconds(2);
    let leader = std::sync::Arc::new(Leadership::new(db.state.db.clone(), "test".into()));
    assert!(leader.renew().await.unwrap());
    assert!(db.state.runtime.leader.set(leader.clone()).is_ok());
    let players = json!([{"steamId":"76561198000000001","faction":"A"},{"steamId":"76561198000000002","faction":"B"},{"steamId":"76561198000000003","faction":"A"}]);
    sqlx::query("INSERT INTO server_live(server_id,ok,players,players_at,status_at,status)VALUES('server',true,$1,$2,$2,'{\"map\":\"Europe\",\"scores\":[{\"name\":\"A\"},{\"name\":\"B\"}]}')").bind(players).bind(at+Duration::seconds(1)).execute(&db.state.db).await.unwrap();
    for (i, victim, observed) in [
        (0, "76561198000000002", Some(at - Duration::seconds(1))),
        (1, "76561198000000003", Some(at - Duration::seconds(1))),
        (2, "76561198000000002", None),
    ] {
        sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,killer_name,killer_faction,victim_steam_id,victim_name,victim_faction,cause,distance_m,headshot,tags,faction_observed_at)VALUES($1,'server',$2,'boot','round',300,'Europe','76561198000000001','Player','A',$3,'Victim','B','Id.Item.AK74M',120,true,'[]',$4)").bind(at).bind(format!("e{i}")).bind(victim).bind(observed).execute(&db.state.db).await.unwrap();
    }
    for lane in ["legacy", "integrity"] {
        sqlx::query("INSERT INTO feed_processing_jobs(server_id,kill_ts,event_ids,consumer)VALUES('server',$1,'[\"e2\",\"e0\",\"e1\"]',$2)").bind(at).bind(lane).execute(&db.state.db).await.unwrap();
    }
    let job = feed_jobs::claim(&leader, Consumer::Integrity, None)
        .await
        .unwrap()
        .unwrap();
    let kills = integrity_consumer::batch(&db.state, &job).await.unwrap();
    assert_eq!(
        kills
            .iter()
            .map(|k| k["eventId"].clone())
            .collect::<Vec<_>>(),
        vec![json!("e2"), json!("e0"), json!("e1")]
    );
    assert_eq!(kills[1]["killer"]["faction"], "A");
    assert_eq!(kills[1]["victim"]["faction"], "B");
    assert!(kills[0]["killer"]["faction"].is_null());
    assert!(kills[2]["killer"]["faction"].is_null());
    feed_jobs::finish(&leader, &job, None).await.unwrap();
    assert!(integrity_consumer::batch(&db.state, &job).await.is_err());
    let lanes:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('consumer',consumer,'state',state)FROM feed_processing_jobs ORDER BY consumer").fetch_all(&db.state.db).await.unwrap();
    assert_eq!(lanes[0], json!({"consumer":"integrity","state":"done"}));
    assert_eq!(lanes[1], json!({"consumer":"legacy","state":"pending"}));
    leader.release().await.unwrap();
    db.close().await;
}
