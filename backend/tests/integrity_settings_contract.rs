mod common;
use axum::http::StatusCode;
use common::Db;
use serde_json::{Value, json};
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn permissions_defaults_versioning_atomic_save_and_weapon_invalidation() {
    let db = Db::new().await;
    let site = db.user("site", true).await;
    let manager = db.user("manager", false).await;
    let member = db.user("member", false).await;
    let stranger = db.user("stranger", false).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org'); INSERT INTO org_members(org_id,user_id,role) VALUES('org','manager','owner'),('org','member','member');").execute(&db.state.db).await.unwrap();
    let path = "/api/orgs/org/integrity/rules";
    let (status, row) = db.call("GET", path, Value::Null, &manager).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(row["rules"]["version"], 1);
    assert_eq!(row["rules"]["assessmentMode"], "statistical_shadow");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM integrity_rules")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        db.call("GET", path, Value::Null, &member).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        db.call("GET", path, Value::Null, &stranger).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        db.call(
            "PUT",
            path,
            json!({"values":{"headshotMinKills":10}}),
            &manager
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        db.call(
            "PUT",
            path,
            json!({"values":{"committeeKpmMinutes":2}}),
            &manager
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        db.call(
            "PUT",
            "/api/orgs/org/integrity/mode",
            json!({"mode":"statistical"}),
            &manager
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        db.call(
            "PUT",
            "/api/orgs/org/integrity/mode",
            json!({"mode":"model_only"}),
            &manager
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, row) = db
        .call(
            "PUT",
            "/api/orgs/org/integrity/mode",
            json!({"mode":"legacy"}),
            &site,
        )
        .await;
    assert_eq!(row["rules"]["version"], 2);
    let (a, b, c) = tokio::join!(
        db.call(
            "PUT",
            path,
            json!({"values":{"headshotMinKills":5}}),
            &manager
        ),
        db.call(
            "PUT",
            path,
            json!({"values":{"headshotMinKills":6}}),
            &manager
        ),
        db.call(
            "PUT",
            path,
            json!({"values":{"headshotMinKills":7}}),
            &manager
        )
    );
    for (status, _) in [a, b, c] {
        assert_eq!(status, StatusCode::OK);
    }
    let (_, row) = db.call("GET", path, Value::Null, &manager).await;
    assert_eq!(row["rules"]["version"], 5);
    assert_eq!(
        db.call("PUT", path, json!({"values":{"mode":"enforce"}}), &manager)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let ep = "/api/orgs/org/integrity/enforcement";
    assert_eq!(
        db.call(
            "PUT",
            ep,
            json!({"values":{"autoKickEnabled":true}}),
            &manager
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status,row)=db.call("PUT",ep,json!({"values":{"autoKickEnabled":true,"confirmation":"ENABLE_EXPERIMENTAL_INTEGRITY"}}),&manager).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(row["enforcement"]["autoKickEnabled"], true);
    let (_, row) = db.call("GET", path, Value::Null, &manager).await;
    assert_eq!(row["rules"]["version"], 5);
    sqlx::query("UPDATE integrity_rules SET auto_suspended_at=now() WHERE org_id='org'")
        .execute(&db.state.db)
        .await
        .unwrap();
    assert_eq!(
        db.call("PUT", ep, json!({"values":{"resume":true}}), &manager)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (_, row) = db
        .call(
            "PUT",
            ep,
            json!({"values":{"resume":true,"confirmation":"ENABLE_EXPERIMENTAL_INTEGRITY"}}),
            &manager,
        )
        .await;
    assert!(row["enforcement"]["autoSuspendedAt"].is_null());
    let wp = "/api/orgs/org/integrity/weapons";
    assert_eq!(
        db.call(
            "PUT",
            wp,
            json!({"cause":"Id.Item.New","category":"bad"}),
            &manager
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, row) = db
        .call(
            "PUT",
            wp,
            json!({"cause":"Id.Item.New","category":"INFANTRY"}),
            &manager,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(row["mapping"]["orgId"], "org");
    let state: Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id='org'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(state["weapon_map_version"], 2);
    assert_eq!(state["baseline_status"], "STALE");
    assert_eq!(
        db.call("DELETE", wp, json!({"cause":"Id.Item.NoSuch"}), &manager)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        db.call("DELETE", wp, json!({"cause":"Id.Item.New"}), &manager)
            .await
            .0,
        StatusCode::OK
    );
    let version: i32 = sqlx::query_scalar(
        "SELECT weapon_map_version FROM integrity_model_state WHERE org_id='org'",
    )
    .fetch_one(&db.state.db)
    .await
    .unwrap();
    assert_eq!(version, 3);
    sqlx::query("UPDATE organizations SET suspended_at=now() WHERE id='org'")
        .execute(&db.state.db)
        .await
        .unwrap();
    assert_eq!(
        db.call("GET", path, Value::Null, &manager).await.0,
        StatusCode::FORBIDDEN
    );
    db.close().await;
}
