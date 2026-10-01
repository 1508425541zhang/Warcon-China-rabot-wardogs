mod common;
use chrono::Utc;
use common::Db;
use serde_json::json;
use warcon_backend::{
    feed_jobs::{self, Consumer},
    leadership::Leadership,
    legacy_consumer,
    observation_state::{self, Player, PresenceDiff},
    observer::Memory,
    trigger_engine,
};
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn records_permissions_native_observation_and_independent_feed() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    let outsider = db.user("outsider", false).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc)VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let configs = [
        ("welcome", json!({"message":"欢迎 {name}"})),
        ("broadcast", json!({"messages":["A","B"],"everyMinutes":1})),
        ("ping_kick", json!({"maxPingMs":200,"durationSeconds":60})),
        ("name_filter", json!({"builtinWords":true,"action":"alert"})),
        ("team_kill", json!({"warnAt":1,"kickAt":2})),
        ("kill_rate", json!({"maxKills":2})),
    ];
    let mut ids = vec![];
    for (kind, config) in configs {
        let (status, v) = db
            .call(
                "POST",
                "/api/servers/server/triggers",
                json!({"kind":kind,"config":config,"enabled":true}),
                &owner,
            )
            .await;
        assert_eq!(status.as_u16(), 201, "{v}");
        ids.push(v["trigger"]["id"].as_str().unwrap().to_owned());
    }
    let (status, _) = db
        .call("GET", "/api/servers/server/triggers", json!({}), &outsider)
        .await;
    assert_eq!(status.as_u16(), 404);
    let (status, v) = db
        .call(
            "POST",
            "/api/servers/server/triggers",
            json!({"kind":"seed_reward","config":{"minutes":10}}),
            &owner,
        )
        .await;
    assert_eq!(status.as_u16(), 201, "{v}");
    let (status, _) = db
        .call(
            "POST",
            "/api/servers/server/triggers",
            json!({"kind":"seed_reward","config":{"minutes":20}}),
            &owner,
        )
        .await;
    assert_eq!(status.as_u16(), 409);
    let leader = std::sync::Arc::new(Leadership::new(db.state.db.clone(), "test".into()));
    assert!(leader.renew().await.unwrap());
    assert!(db.state.runtime.leader.set(leader.clone()).is_ok());
    let ts = Utc::now().timestamp_millis();
    let player = Player {
        steam_id: "76561198000000001".into(),
        name: "naaazi".into(),
        faction: Some("A".into()),
        kills: 0,
        deaths: 0,
        cash: 0,
        ping: Some(300.),
    };
    let mut m = Memory::new("server".into(), "org".into(), "Server".into(), ts, 30000);
    m.status = json!({"map":"Europe","playerCount":1,"maxPlayers":50,"scores":[{"name":"A","score":1},{"name":"B","score":0}]});
    m.players = vec![player.clone()];
    m.players_interval = 30000;
    m.lists_loaded = true;
    m.sessions = Some(std::collections::HashMap::from([(
        player.steam_id.clone(),
        observation_state::joined(1, &player, ts, None, true),
    )]));
    let diff = PresenceDiff {
        joined: vec![player.clone()],
        ..Default::default()
    };
    trigger_engine::prepare(&db.state, &mut m, &diff, true, true, ts)
        .await
        .unwrap();
    // A rolled back observation cannot consume cadence or ping state.
    let mut next = m.clone();
    let mut tx = db.state.worker_transaction().await.unwrap();
    trigger_engine::evaluate(&mut tx, &mut next, &diff, true, true, None, 0, ts)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        0
    );
    let mut tx = db.state.worker_transaction().await.unwrap();
    trigger_engine::evaluate(&mut tx, &mut m, &diff, true, true, None, 0, ts)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM outbox ORDER BY id")
        .fetch_all(&db.state.db)
        .await
        .unwrap();
    assert_eq!(actions, vec!["whisper", "broadcast", "name_flag"]);
    let stayed = PresenceDiff {
        stayed: vec![player.clone()],
        ..Default::default()
    };
    let later = ts + 61000;
    trigger_engine::prepare(&db.state, &mut m, &stayed, true, true, later)
        .await
        .unwrap();
    let mut tx = db.state.worker_transaction().await.unwrap();
    trigger_engine::evaluate(&mut tx, &mut m, &stayed, true, true, None, 0, later)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox WHERE action='kick'")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    // Edited rules do not accept old cached intentions.
    let stale = m
        .automation
        .rows
        .iter()
        .find(|r| r["kind"] == "welcome")
        .unwrap()
        .clone();
    let (status, _) = db
        .call(
            "PATCH",
            &format!("/api/servers/server/triggers/{}", ids[0]),
            json!({"enabled":false}),
            &owner,
        )
        .await;
    assert_eq!(status.as_u16(), 200);
    let mut tx = db.state.worker_transaction().await.unwrap();
    let i = warcon_backend::trigger_store::Intent {
        action: "kick".into(),
        params: json!({}),
        target: player.steam_id.clone(),
        detail: json!({}),
        steam: Some(player.steam_id.clone()),
        key: "stale-config".into(),
        ok: "".into(),
    };
    assert_eq!(
        warcon_backend::trigger_store::enqueue(&mut tx, "server", &stale, &[i], None, Utc::now())
            .await
            .unwrap(),
        0
    );
    tx.commit().await.unwrap();
    let at = chrono::DateTime::from_timestamp_millis(ts).unwrap();
    for id in ["one", "two"] {
        sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,killer_name,killer_faction,victim_steam_id,victim_name,victim_faction,team_kill,cause,headshot,tags)VALUES($1,'server',$2,'boot','round',300,'Europe','76561198000000001','Player','A','76561198000000002','Victim','A',true,'Id.Item.AK74M',true,'[]')").bind(at).bind(id).execute(&db.state.db).await.unwrap();
    }
    for lane in ["legacy", "integrity"] {
        sqlx::query("INSERT INTO feed_processing_jobs(server_id,kill_ts,event_ids,consumer)VALUES('server',$1,'[\"one\",\"two\"]',$2)").bind(at).bind(lane).execute(&db.state.db).await.unwrap();
    }
    let job = feed_jobs::claim(&leader, Consumer::Legacy, None)
        .await
        .unwrap()
        .unwrap();
    let events = legacy_consumer::batch(&db.state, &job).await.unwrap();
    assert_eq!(events[0]["killer"]["faction"], "A");
    assert_eq!(
        sqlx::query_scalar::<_, bool>("SELECT bool_and(NOT faction_bracketed) FROM kills")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        true
    );
    let engine = legacy_consumer::Engine::default();
    engine
        .process(&db.state, "server", &events, true)
        .await
        .unwrap();
    engine
        .process(&db.state, "server", &events, true)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox WHERE action='kill_rate_flag'")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox WHERE trigger_kind='team_kill'")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM webhook_events WHERE kind='teamkills'")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        2
    );
    feed_jobs::finish(&leader, &job, None).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM feed_processing_jobs WHERE consumer='integrity'"
        )
        .fetch_one(&db.state.db)
        .await
        .unwrap(),
        "pending"
    );
    leader.release().await.unwrap();
    db.close().await;
}
