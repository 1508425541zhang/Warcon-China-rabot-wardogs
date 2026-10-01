mod common;
use axum::http::StatusCode;
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{webhook_status, webhook_worker, webhooks as h};
#[tokio::test]
async fn credential_validation_privacy_and_card_limits() {
    let valid = "https://discord.com/api/webhooks/123456789012345/ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef?ignored=true";
    assert!(!h::validate_url(valid).unwrap().0.contains('?'));
    for url in [
        "http://discord.com/api/webhooks/123456789012345/ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef",
        "https://discord.com.evil.invalid/api/webhooks/123456789012345/ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef",
        "https://user@discord.com/api/webhooks/123456789012345/ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef",
    ] {
        assert!(h::validate_url(url).is_err())
    }
    assert!(
        h::classify(&json!({"category":"player","action":"integrity.report.create"})).is_none()
    );
    let embed = h::audit_embed(
        "Warcon",
        &json!({"actor_name":"Owner","action":"server.update","target":"private-host:token","message":"private error","outcome":"error","server_name":"Server","ts":"2026-10-01T00:00:00Z"}),
    );
    assert!(!embed.to_string().contains("private"));
    let mut cfg = warcon_backend::config::Config::for_test();
    cfg.origin = "https://panel.example.invalid".into();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://postgres@localhost/unused")
        .unwrap();
    let state = warcon_backend::config::AppState {
        db: pool,
        config: cfg,
        runtime: Default::default(),
    };
    let players:Vec<_>=(0..200).map(|i|json!({"name":format!("Player{i} @everyone **name**{}","字".repeat(100)),"kills":i,"deaths":2,"faction":(["RED","BLU","GRN"][i%3])})).collect();
    let live = json!({"ok":true,"observedAt":"2026-10-01T00:00:00Z","startedAt":"2026-09-29T00:00:00Z","gameServerId":"join","status":{"map":"Europe","lighting":"DayClear","playerCount":200,"maxPlayers":220,"scores":[{"name":"RED","score":80},{"name":"BLU","score":50},{"name":"GRN","score":45}],"matchSeconds":300},"players":players});
    for style in ["banner", "compact", "scoreboard"] {
        let card = webhook_status::render(
            &state,
            &json!({"status_style":style,"link_status":true,"link_panel":true}),
            &json!({"id":"server","name":"Server","org_name":"Org","public_status":false,"allow_public_status":true}),
            &live,
            1790812800000,
        );
        assert!(webhook_status::embed_length(&card) <= 6000);
        assert_eq!(card["url"], "https://panel.example.invalid/server/server");
        for f in card["fields"].as_array().unwrap() {
            assert!(h::text(&f["value"]).encode_utf16().count() <= 1024)
        }
    }
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn webhook_scope_transactional_fanout_and_unknown_not_replayed() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    let user = db.user("member", false).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org'),('other','Other','other'); INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc) VALUES('server','org','Server','example.invalid',1234,'http','fixture'),('other-server','other','Other','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let path = "/api/orgs/org/webhooks";
    let input = json!({"url":"https://discord.com/api/webhooks/123456789012345/ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef","events":["management","integrity"],"serverIds":["server"]});
    assert_eq!(
        db.call("POST", path, input.clone(), &user).await.0,
        StatusCode::NOT_FOUND
    );
    let mut invalid = input.clone();
    invalid["serverIds"] = json!(["other-server"]);
    assert_eq!(
        db.call("POST", path, invalid, &owner).await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, result) = db.call("POST", path, input, &owner).await;
    assert_eq!(status, StatusCode::CREATED, "{result}");
    assert!(!result.to_string().contains("ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
    let id = result["webhook"]["id"].as_str().unwrap();
    let edit = format!("{path}/{id}");
    // Scoped hooks receive server events; they do not receive org-wide management changes.
    assert!(webhook_worker::fanout(&db.state).await.unwrap() > 0);
    assert!(webhook_worker::claim(&db.state).await.unwrap().is_empty());
    let mut tx = db.state.db.begin().await.unwrap();
    h::input(&mut tx,"case:1","org","server","integrity",&json!({"caseId":"case:1","serverId":"server","serverName":"Server","steamId":"76561198000000001","score":6.5,"level":"CASE","infantryKills":5,"kpm180":1.7,"uniqueVictims":5,"uniqueReporters":0,"breakdown":[],"createdAt":h::stamp()})).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(webhook_worker::fanout(&db.state).await.unwrap(), 0);
    let mut tx = db.state.db.begin().await.unwrap();
    h::input(&mut tx,"case:1","org","server","integrity",&json!({"caseId":"case:1","serverId":"server","serverName":"Server","steamId":"76561198000000001","score":6.5,"level":"CASE","infantryKills":5,"kpm180":1.7,"uniqueVictims":5,"uniqueReporters":0,"breakdown":[],"createdAt":h::stamp()})).await.unwrap();
    tx.commit().await.unwrap();
    webhook_worker::fanout(&db.state).await.unwrap();
    let claim = webhook_worker::claim(&db.state).await.unwrap();
    assert_eq!(claim.len(), 1);
    assert!(claim[0]["payload"].to_string().contains("6.5"));
    assert_eq!(claim[0]["payload"]["allowed_mentions"]["parse"], json!([]));
    webhook_worker::complete(
        &db.state,
        &claim,
        &h::PostResult {
            error: "timeout".into(),
            ..Default::default()
        },
        false,
    )
    .await
    .unwrap();
    assert!(webhook_worker::claim(&db.state).await.unwrap().is_empty());
    assert_eq!(
        db.call("PATCH", &edit, json!({"enabled":false}), &owner)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        db.call("DELETE", &edit, Value::Null, &owner).await.0,
        StatusCode::OK
    );
    db.close().await;
}
