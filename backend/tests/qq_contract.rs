mod common;
use axum::{Json, Router, http::StatusCode, routing::post};
use chrono::{Duration, Utc};
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{qq_community, qq_config, qq_economy, qq_identity, qq_protocol, qq_runtime};
const STEAM: &str = "76561198000000001";
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn identity_migration_credits_votes_reconciliation_and_signed_inbox() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    let member = db.user("member", false).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc) VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let policy = json!({"serverId":"server","groups":["groupOpenID0123456789"],"maps":["Europe","Africa"],"lowAt":20,"pointsPerMinute":2});
    let input = json!({"revision":"environment","enabled":true,"provider":"official","url":"https://api.bot.qq.com","selfId":"123456","secret":"fixture-qq-event-secret","policies":[policy]});
    assert_eq!(
        db.call("PUT", "/api/admin/qq", input.clone(), &member)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, response) = db.call("PUT", "/api/admin/qq", input.clone(), &owner).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(response["config"]["hasSecret"] == true);
    assert!(!response.to_string().contains("fixture-qq-event-secret"));
    assert_eq!(
        db.call("PUT", "/api/admin/qq", input, &owner).await.0,
        StatusCode::CONFLICT
    );
    let stored: String =
        sqlx::query_scalar("SELECT value::text FROM site_settings WHERE key='qqCommunity'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert!(!stored.contains("fixture-qq-event-secret"));
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action='qq.settings.update'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(audits, 1);
    let at = Utc::now();
    let ids = vec![STEAM.into(), STEAM.into()];
    for (step, count, trusted) in [
        (0, 10, true),
        (1, 10, true),
        (1, 10, true),
        (2, 21, true),
        (3, 10, false),
    ] {
        let mut tx = db.state.db.begin().await.unwrap();
        qq_economy::warmth(
            &db.state,
            &mut tx,
            "server",
            at + Duration::seconds(step * 30),
            &ids,
            30000,
            count,
            trusted,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(
        qq_economy::wallet(&db.state, "server", STEAM)
            .await
            .unwrap(),
        json!({"balance":2,"warmMinutes":1})
    );
    sqlx::query("INSERT INTO qq_links(server_id,member_id,user_id,steam_id) VALUES('server','ob11:876543',NULL,$1)").bind(STEAM).execute(&db.state.db).await.unwrap();
    let official = "official:123456:memberOpenID0123456789";
    let roster = json!({"players":[{"steamId":STEAM,"name":"张三"}]});
    assert!(
        qq_identity::bind_player(&db.state, "server", official, STEAM, "错名", &roster)
            .await
            .is_err()
    );
    assert!(
        qq_identity::bind_player(&db.state, "server", official, STEAM, "张三", &roster)
            .await
            .unwrap()["migrated"]
            == true
    );
    assert!(
        qq_identity::bind_player(&db.state, "server", official, STEAM, "张三", &roster)
            .await
            .unwrap()["already"]
            == true
    );
    assert_eq!(
        qq_economy::wallet(&db.state, "server", STEAM)
            .await
            .unwrap()["balance"],
        2
    );
    let view = db
        .call(
            "GET",
            "/api/servers/server/qq-bindings?q=765611&page=999",
            json!({}),
            &owner,
        )
        .await;
    assert_eq!(view.0, StatusCode::OK, "{}", view.1);
    assert_eq!(view.1["page"], 1);
    assert_eq!(view.1["links"][0]["previous_member_id"], "ob11:876543");
    let change = json!({"action":"remove","memberId":official,"expectedSteamId":STEAM,"expectedUserId":"wrong","reason":"人工核验"});
    assert_eq!(
        db.call("POST", "/api/servers/server/qq-bindings", change, &owner)
            .await
            .0,
        StatusCode::CONFLICT
    );
    qq_identity::unbind_member(&db.state, "server", official)
        .await
        .unwrap();
    assert_eq!(
        qq_economy::wallet(&db.state, "server", STEAM)
            .await
            .unwrap()["balance"],
        2
    );
    // Wallet debit rollback is real, including insufficient funds and duplicate ledger identity.
    let mut tx = db.state.db.begin().await.unwrap();
    qq_economy::debit(&mut tx, "server", STEAM, 1, "buy:test", "测试")
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        qq_economy::wallet(&db.state, "server", STEAM)
            .await
            .unwrap()["balance"],
        2
    );
    sqlx::query("UPDATE qq_wallets SET balance=100 WHERE server_id='server'")
        .execute(&db.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO qq_votes(id,server_id,maps,ends_at) VALUES('vote','server','[\"Europe\",\"Africa\"]',now()+interval '1 minute')").execute(&db.state.db).await.unwrap();
    let c = qq_config::load(&db.state).await.unwrap();
    let p = c.policy("server").unwrap();
    qq_community::cast_vote(&db.state, p, STEAM, "2")
        .await
        .unwrap();
    qq_community::cast_vote(&db.state, p, STEAM, "2")
        .await
        .unwrap();
    assert_eq!(
        qq_economy::wallet(&db.state, "server", STEAM)
            .await
            .unwrap()["balance"],
        90
    );
    sqlx::query("UPDATE qq_votes SET ends_at=now() WHERE id='vote'")
        .execute(&db.state.db)
        .await
        .unwrap();
    qq_community::close_votes(&db.state, &c).await.unwrap();
    let winner: String = sqlx::query_scalar("SELECT winner FROM qq_votes WHERE id='vote'")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(winner, "Africa");
    sqlx::query("UPDATE qq_orders SET state='unknown' WHERE id='map:vote'")
        .execute(&db.state.db)
        .await
        .unwrap();
    let mut tx = db.state.db.begin().await.unwrap();
    qq_economy::reconcile(&mut tx, "server", "map:vote", true, "owner")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        qq_economy::wallet(&db.state, "server", STEAM)
            .await
            .unwrap()["balance"],
        100
    );
    let mut tx = db.state.db.begin().await.unwrap();
    assert!(
        qq_economy::reconcile(&mut tx, "server", "map:vote", true, "owner")
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    let m = qq_protocol::Message {
        id: "official:123456:groupOpenID0123456789:message".into(),
        group_id: "groupOpenID0123456789".into(),
        member_id: official.into(),
        content: "/积分".into(),
    };
    qq_protocol::accept(&db.state, &c, &m).await.unwrap();
    qq_protocol::accept(&db.state, &c, &m).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM qq_inbox")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    sqlx::query("UPDATE qq_inbox SET created_at=now()-interval '5 minutes'")
        .execute(&db.state.db)
        .await
        .unwrap();
    qq_runtime::message(&db.state, &c).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT reply_state FROM qq_inbox")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(status, "expired");
    sqlx::raw_sql("UPDATE qq_inbox SET state='processing',reply_state='sending';INSERT INTO qq_orders(id,server_id,steam_id,kind,params,cost,state) VALUES('interrupted','server','76561198000000001','friendly','{}',20,'processing');INSERT INTO qq_deliveries(order_id,steam_id,state)VALUES('interrupted','76561198000000002','sending');").execute(&db.state.db).await.unwrap();
    qq_runtime::recover(&db.state).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM qq_orders WHERE id='interrupted'")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        "unknown"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM qq_deliveries WHERE order_id='interrupted'"
        )
        .fetch_one(&db.state.db)
        .await
        .unwrap(),
        "unknown"
    );
    db.close().await;
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn onebot_sender_confirms_once_and_never_replays_unknown() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc)VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = sent.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/send_group_msg",
        post(move |Json(v): Json<Value>| {
            let count = count.clone();
            async move {
                assert_eq!(v["message"][0]["type"], "text");
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Json(json!({"status":"ok","retcode":0,"data":{"message_id":1}}))
            }
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let input = json!({"revision":"environment","enabled":true,"provider":"napcat","url":url,"selfId":"123456","secret":"fixture-onebot-secret","token":"fixture-onebot-token","policies":[{"serverId":"server","groups":["567890"],"maps":["Europe","Africa"]}]});
    let (status, v) = db.call("PUT", "/api/admin/qq", input, &owner).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let c = qq_config::load(&db.state).await.unwrap();
    let m = qq_protocol::Message {
        id: "ob11:123456:567890:1".into(),
        group_id: "567890".into(),
        member_id: "ob11:876543".into(),
        content: "/帮助".into(),
    };
    qq_protocol::accept(&db.state, &c, &m).await.unwrap();
    qq_runtime::message(&db.state, &c).await.unwrap();
    qq_runtime::message(&db.state, &c).await.unwrap();
    assert_eq!(sent.load(std::sync::atomic::Ordering::SeqCst), 1);
    sqlx::query("UPDATE qq_inbox SET reply_state='sending'")
        .execute(&db.state.db)
        .await
        .unwrap();
    qq_runtime::recover(&db.state).await.unwrap();
    qq_runtime::message(&db.state, &c).await.unwrap();
    assert_eq!(sent.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT reply_state FROM qq_inbox")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        "unknown"
    );
    task.abort();
    db.close().await;
}
