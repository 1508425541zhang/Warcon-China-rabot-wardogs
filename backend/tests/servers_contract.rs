mod common;
use common::Db;
use serde_json::json;
use warcon_backend::crypto;
#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn server_target_secrets_quotas_grants_and_visibility() {
    let mut db = Db::new().await;
    db.state.config.identity.allow_signup = true;
    db.app = warcon_backend::api::router(db.state.clone());
    let owner = db.user("owner", true).await;
    let manager = db.user("manager", false).await;
    let viewer = db.user("viewer", false).await;
    let stranger = db.user("stranger", false).await;
    let (s, v) = db
        .call("POST", "/api/orgs", json!({"name":"Managed"}), &manager)
        .await;
    assert_eq!(s, 201, "{v}");
    let org = v["id"].as_str().unwrap();
    sqlx::query("UPDATE organizations SET allow_public_status=false WHERE id=$1")
        .bind(org)
        .execute(&db.state.db)
        .await
        .unwrap();
    let input = json!({"orgId":org,"name":"Fixture","host":"127.0.0.1","port":30001,"scheme":"http","password":"private-fixture-secret"});
    assert_eq!(
        db.call("POST", "/api/servers", input.clone(), &manager)
            .await
            .0,
        400
    );
    assert_eq!(
        db.call("POST", "/api/servers", input.clone(), &stranger)
            .await
            .0,
        404
    );
    let (s, v) = db.call("POST", "/api/servers", input.clone(), &owner).await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_owned();
    let path = format!("/api/servers/{id}");
    let stored: String = sqlx::query_scalar("SELECT password_enc FROM servers WHERE id=$1")
        .bind(&id)
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert!(!stored.contains("private-fixture-secret"));
    assert_eq!(
        crypto::decrypt_secret(&db.state.config.encryption_key, &stored).unwrap(),
        "private-fixture-secret"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM server_lists WHERE server_id=$1")
            .bind(&id)
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        db.call("PATCH", &path, json!({"port":30002}), &owner)
            .await
            .0,
        400
    );
    assert_eq!(
        db.call(
            "PATCH",
            &path,
            json!({"host":"169.254.169.254","password":"new"}),
            &owner
        )
        .await
        .0,
        400
    );
    assert_eq!(
        db.call(
            "PATCH",
            &path,
            json!({"name":"New","notes":"Note"}),
            &manager
        )
        .await
        .0,
        200
    );
    assert_eq!(
        db.call("PATCH", &path, json!({"name":"New"}), &manager)
            .await
            .0,
        200
    );
    assert_eq!(
        db.call("PATCH", &path, json!({"publicStatus":true}), &manager)
            .await
            .0,
        403
    );
    sqlx::query("UPDATE organizations SET allow_public_status=true,server_limit=1 WHERE id=$1")
        .bind(org)
        .execute(&db.state.db)
        .await
        .unwrap();
    assert_eq!(
        db.call("PATCH", &path, json!({"publicStatus":true}), &manager)
            .await
            .0,
        200
    );
    assert_eq!(
        db.call(
            "POST",
            "/api/servers",
            json!({"orgId":org,"name":"Too many","host":"8.8.8.8","port":30001,"password":"x"}),
            &manager
        )
        .await
        .0,
        403
    );
    sqlx::query("INSERT INTO org_members(org_id,user_id,role) VALUES($1,'viewer','member')")
        .bind(org)
        .execute(&db.state.db)
        .await
        .unwrap();
    let role: String = sqlx::query_scalar(
        "SELECT id FROM org_roles WHERE org_id=$1 AND capabilities='[\"server.view\"]'::jsonb",
    )
    .bind(org)
    .fetch_one(&db.state.db)
    .await
    .unwrap();
    let(s,v)=db.call("PUT",&format!("{path}/grants"),json!({"grants":[{"userId":"viewer","roleId":role},{"userId":"stranger","roleId":role},{"userId":"viewer","roleId":"other-org-role"}]}),&manager).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM server_grants WHERE server_id=$1")
            .bind(&id)
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        1
    );
    let (_, v) = db.call("GET", "/api/servers", json!({}), &viewer).await;
    assert_eq!(v["servers"][0]["host"], "");
    assert_eq!(v["servers"][0]["port"], 0);
    assert_eq!(v["servers"][0]["notes"], "");
    assert!(!v.to_string().contains("private-fixture-secret"));
    assert_eq!(
        db.call("PATCH", &path, json!({"name":"Intrude"}), &viewer)
            .await
            .0,
        403
    );
    assert_eq!(
        db.call("GET", &format!("{path}/grants"), json!({}), &viewer)
            .await
            .0,
        403
    );
    let (_, v) = db.call("GET", "/api/servers", json!({}), &stranger).await;
    assert!(v["servers"].as_array().unwrap().is_empty());
    assert_eq!(db.call("DELETE", &path, json!({}), &manager).await.0, 200);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM lists WHERE server_id=$1")
            .bind(&id)
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        0
    );
    db.close().await;
}
