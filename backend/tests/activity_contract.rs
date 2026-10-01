mod common;
use axum::http::StatusCode;
use common::Db;
use serde_json::{Value, json};
use warcon_backend::api::activity;
#[test]
fn audit_csv_prevents_formulas_and_preserves_objects() {
    for (value, expected) in [
        (json!("=1+2"), "\"'=1+2\""),
        (json!("+cmd"), "\"'+cmd\""),
        (json!(-2), "-2"),
        (Value::Null, ""),
        (json!("a,b"), "\"a,b\""),
        (json!("a\"b"), "\"a\"\"b\""),
        (json!("safe"), "safe"),
    ] {
        assert_eq!(activity::csv_cell(&value), expected)
    }
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn audit_visibility_pagination_metadata_and_browser_scope() {
    let db = Db::new().await;
    let owner = db.user("site", true).await;
    let manager = db.user("manager", false).await;
    let staff = db.user("staff", false).await;
    let stranger = db.user("stranger", false).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org'),('other','Other','other'); INSERT INTO org_members(org_id,user_id,role) VALUES('org','manager','owner'),('org','staff','member'); INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc) VALUES('server','org','Server','example.invalid',1234,'http','fixture'); INSERT INTO org_roles(id,org_id,name,capabilities) VALUES('audit-role','org','Audit','[\"server.view\",\"audit.read\"]'); INSERT INTO server_grants(server_id,user_id,role_id) VALUES('server','staff','audit-role');").execute(&db.state.db).await.unwrap();
    for (actor, org, server, action, ua) in [
        (
            "manager",
            Some("org"),
            Some("server"),
            "server.create",
            "owner-browser",
        ),
        (
            "site",
            Some("org"),
            Some("server"),
            "player.note",
            "site-browser",
        ),
        ("staff", None, None, "auth.login", "staff-browser"),
        (
            "stranger",
            Some("other"),
            None,
            "org.update",
            "other-browser",
        ),
    ] {
        sqlx::query("INSERT INTO audit_log(actor_id,actor_name,org_id,server_id,server_name,category,action,outcome,user_agent,message) VALUES($1,$1,$2,$3,'Server','system',$4,'ok',$5,'test')").bind(actor).bind(org).bind(server).bind(action).bind(ua).execute(&db.state.db).await.unwrap();
    }
    let (status, result) = db
        .call("GET", "/api/audit?limit=1", Value::Null, &staff)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["entries"][0]["actorId"], "staff");
    assert_eq!(result["entries"][0]["userAgent"], "staff-browser");
    let cursor = result["nextBefore"].as_i64().unwrap();
    let (status, result) = db
        .call(
            "GET",
            &format!("/api/audit?limit=1&before={cursor}"),
            Value::Null,
            &staff,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["entries"][0]["action"], "player.note");
    assert_eq!(result["entries"][0]["userAgent"], "");
    assert!(result["nextBefore"].is_null());
    let (_, result) = db.call("GET", "/api/audit", Value::Null, &manager).await;
    assert_eq!(result["entries"].as_array().unwrap().len(), 2);
    assert_eq!(result["entries"][0]["userAgent"], "site-browser");
    let (_, result) = db.call("GET", "/api/audit/meta", Value::Null, &staff).await;
    assert_eq!(result["actions"].as_array().unwrap().len(), 2);
    assert!(
        result["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["actorId"] == "staff")
    );
    let (_, result) = db.call("GET", "/api/audit", Value::Null, &owner).await;
    assert_eq!(result["entries"].as_array().unwrap().len(), 4);
    let (_, result) = db
        .call("GET", "/api/audit?from=2099-01-01", Value::Null, &owner)
        .await;
    assert!(result["entries"].as_array().unwrap().is_empty());
    let (_, result) = db.call("GET", "/api/audit", Value::Null, &stranger).await;
    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        db.call("PUT", "/api/scope", json!({"orgId":"org"}), &staff)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        db.call("PUT", "/api/scope", json!({"orgId":"other"}), &staff)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        db.call("DELETE", "/api/scope", Value::Null, &staff).await.0,
        StatusCode::OK
    );
    sqlx::query("UPDATE organizations SET suspended_at=now() WHERE id='org'")
        .execute(&db.state.db)
        .await
        .unwrap();
    let (_, result) = db.call("GET", "/api/audit", Value::Null, &manager).await;
    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
    db.close().await;
}
