mod common;
use common::Db;
use serde_json::{Value, json};
use warcon_backend::api::plugins;
#[test]
fn original_zod_manifest_parity() {
    let rows: Vec<Value> = serde_json::from_str(include_str!("../fixtures/plugins.json")).unwrap();
    for row in rows {
        let result = plugins::manifest(row["input"].clone());
        assert_eq!(result.is_ok(), row["valid"], "{}", row["input"]);
        if let Ok(result) = result {
            assert_eq!(result, row["result"])
        }
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn plugin_ownership_validation_snapshot_scope_and_limit() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    let other = db.user("other", false).await;
    let manifest = json!({"apiVersion":1,"id":"test-plugin","name":"测试","widgets":[{"type":"metric","title":"在线","metric":"online"}]});
    let (s, v) = db
        .call("POST", "/api/plugins", json!({"manifest":manifest}), &owner)
        .await;
    assert_eq!(s, 201, "{v}");
    assert_eq!(v["plugin"]["manifest"]["style"]["accent"], "#69d6e3");
    assert_eq!(
        db.call("POST", "/api/plugins", json!({"manifest":manifest}), &owner)
            .await
            .0,
        409
    );
    assert_eq!(
        db.call("GET", "/api/plugins/test-plugin", json!({}), &other)
            .await
            .0,
        404
    );
    assert_eq!(
        db.call("DELETE", "/api/plugins/test-plugin", json!({}), &other)
            .await
            .0,
        404
    );
    assert_eq!(
        db.call(
            "PUT",
            "/api/plugins/test-plugin",
            json!({"manifest":manifest,"enabled":false}),
            &owner
        )
        .await
        .0,
        200
    );
    assert_eq!(
        db.call("GET", "/api/plugins/test-plugin/data", json!({}), &owner)
            .await
            .0,
        409
    );
    sqlx::query("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org')")
        .execute(&db.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO servers(id,org_id,name,host,port,password_enc) VALUES('s','org','Server','example.invalid',1,'unused')").execute(&db.state.db).await.unwrap();
    let live = json!([{"steamId":"76561198000000001","name":"Name","faction":"Red","kills":3,"deaths":1,"cash":100,"ping":20},{"steamId":"76561198000000002","name":"Name2","kills":1,"deaths":2,"cash":50,"ping":30}]);
    sqlx::query("INSERT INTO server_live(server_id,players,players_at,status,status_at,observed_at) VALUES('s',$1,now(),$2,now(),now())").bind(live).bind(json!({"map":"Map"})).execute(&db.state.db).await.unwrap();
    assert_eq!(
        db.call(
            "PUT",
            "/api/plugins/test-plugin",
            json!({"manifest":manifest,"serverId":"s"}),
            &owner
        )
        .await
        .0,
        200
    );
    let (s, v) = db
        .call("GET", "/api/plugins/test-plugin/data", json!({}), &owner)
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["snapshot"]["metrics"]["online"], 2);
    assert_eq!(v["snapshot"]["metrics"]["kills"].as_f64(), Some(4.));
    assert_eq!(v["snapshot"]["metrics"]["averagePing"].as_f64(), Some(25.));
    assert_eq!(v["snapshot"]["stale"], false);
    assert!(!v.to_string().contains("password"));
    assert_eq!(
        db.call(
            "POST",
            "/api/plugins",
            json!({"manifest":manifest,"serverId":"s"}),
            &other
        )
        .await
        .0,
        404
    );
    for i in 1..20 {
        let mut m = manifest.clone();
        m["id"] = json!(format!("plugin-{i}"));
        assert_eq!(
            db.call("POST", "/api/plugins", json!({"manifest":m}), &owner)
                .await
                .0,
            201
        )
    }
    let mut m = manifest.clone();
    m["id"] = json!("plugin-overflow");
    assert_eq!(
        db.call("POST", "/api/plugins", json!({"manifest":m}), &owner)
            .await
            .0,
        409
    );
    assert_eq!(
        db.call("DELETE", "/api/plugins/test-plugin", json!({}), &owner)
            .await
            .0,
        200
    );
    assert_eq!(
        db.call("GET", "/api/plugins/test-plugin", json!({}), &owner)
            .await
            .0,
        404
    );
    db.close().await;
}
