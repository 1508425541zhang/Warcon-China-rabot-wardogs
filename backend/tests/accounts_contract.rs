use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgConnectOptions};
use tower::ServiceExt;
use warcon_backend::{
    api,
    config::{AppState, Config},
    migrations,
};
async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Value,
    cookie: &str,
) -> (StatusCode, Value, Vec<String>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", "http://localhost:3000")
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().into())
        .collect();
    let bytes = to_bytes(response.into_body(), 1048576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap(), cookies)
}
async fn create_user(app: &axum::Router, cookie: &str, username: &str) -> (String, String) {
    let (status, body, _) = call(
        app,
        "POST",
        "/api/users",
        json!({"username":username,"password":"PasswordForTest123!","mustChangePassword":false}),
        cookie,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let id = body["id"].as_str().unwrap().to_owned();
    let (status, body, cookies) = call(
        app,
        "POST",
        "/api/identity/login",
        json!({"username":username,"password":"PasswordForTest123!"}),
        "",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let cookie = cookies
        .into_iter()
        .find(|s| s.starts_with("warcon.session_token="))
        .unwrap();
    (id, cookie)
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn ownership_grants_invite_quota_and_atomic_revocation() {
    let options: PgConnectOptions = std::env::var("TEST_DATABASE_URL").unwrap().parse().unwrap();
    let admin = PgPool::connect_with(options.clone()).await.unwrap();
    let name = format!("warcon_rust_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let db = PgPool::connect_with(options.database(&name)).await.unwrap();
    migrations::migrate(
        &db,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../drizzle"),
    )
    .await
    .unwrap();
    let mut config = Config::for_test();
    config.identity.allow_signup = true;
    config.organizations.max_per_user = 1;
    let app = api::router(AppState {
        runtime: Default::default(),
        db: db.clone(),
        config,
    });
    let (status, body, cookies) = call(
        &app,
        "POST",
        "/api/identity/setup",
        json!({"username":"Owner","password":"PasswordForTest123!","again":"PasswordForTest123!"}),
        "",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let owner = cookies
        .into_iter()
        .find(|c| c.starts_with("warcon.session_token="))
        .unwrap();
    let (u1, c1) = create_user(&app, &owner, "MemberOne").await;
    let (u2, c2) = create_user(&app, &owner, "MemberTwo").await;
    let (u3, c3) = create_user(&app, &owner, "MemberThree").await;
    assert_eq!(call(&app, "GET", "/api/users", json!({}), &c1).await.0, 403);
    let (_, users, _) = call(&app, "GET", "/api/users", json!({}), &owner).await;
    assert_eq!(users["users"].as_array().unwrap().len(), 4);
    assert!(!users.to_string().contains("PasswordForTest"));
    assert!(
        users["users"]
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u.get("email").is_none() && u.get("recovery_key_hash").is_none())
    );
    let (status, body, _) = call(
        &app,
        "POST",
        "/api/orgs",
        json!({"name":"测试 Community"}),
        &c1,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let org = body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        call(&app, "POST", "/api/orgs", json!({"name":"Another"}), &c1)
            .await
            .0,
        403
    );
    let (_, body, _) = call(
        &app,
        "POST",
        "/api/orgs",
        json!({"name":"Other Community"}),
        &owner,
    )
    .await;
    let other = body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/orgs/{org}/members"),
            json!({}),
            &c2
        )
        .await
        .0,
        404
    );
    let role: String =
        sqlx::query_scalar("SELECT id FROM org_roles WHERE org_id=$1 AND builtin='operator'")
            .bind(&org)
            .fetch_one(&db)
            .await
            .unwrap();
    let other_role: String =
        sqlx::query_scalar("SELECT id FROM org_roles WHERE org_id=$1 AND builtin='operator'")
            .bind(&other)
            .fetch_one(&db)
            .await
            .unwrap();
    for (id, o) in [("test-server", &org), ("other-server", &other)] {
        sqlx::query("INSERT INTO servers(id,name,host,port,password_enc,org_id) VALUES($1,$1,'8.8.8.8',8080,'test',$2)").bind(id).bind(o).execute(&db).await.unwrap();
    }
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/orgs/{org}/invites"),
            json!({"serverRoleId":other_role}),
            &c1
        )
        .await
        .0,
        404
    );
    let (status, body, _) = call(
        &app,
        "POST",
        &format!("/api/orgs/{org}/invites"),
        json!({"serverRoleId":role,"orgRole":"member","maxUses":1}),
        &c1,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let iid = body["invite"]["id"].as_str().unwrap().to_owned();
    let token = body["invite"]["url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let (status, body, _) = call(
        &app,
        "GET",
        &format!("/api/identity/invites/{token}"),
        json!({}),
        "",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["invite"]["status"], "live");
    let path = format!("/api/identity/invites/{token}");
    let (a, b) = tokio::join!(
        call(&app, "POST", &path, json!({}), &c2),
        call(&app, "POST", &path, json!({}), &c3)
    );
    assert!(
        matches!((a.0.as_u16(), b.0.as_u16()), (200, 410) | (410, 200)),
        "{a:?} {b:?}"
    );
    let (winner, wcookie) = if a.0 == 200 { (&u2, &c2) } else { (&u3, &c3) };
    let uses: i32 = sqlx::query_scalar("SELECT uses FROM org_invites WHERE id=$1")
        .bind(&iid)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(uses, 1);
    assert_eq!(call(&app, "POST", &path, json!({}), wcookie).await.0, 200);
    let uses: i32 = sqlx::query_scalar("SELECT uses FROM org_invites WHERE id=$1")
        .bind(&iid)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(uses, 1);
    let grants: i64 =
        sqlx::query_scalar("SELECT count(*) FROM server_grants WHERE user_id=$1 AND role_id=$2")
            .bind(winner)
            .bind(&role)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(grants, 1);
    let (_,body,_)=call(&app,"PUT",&format!("/api/users/{winner}/grants"),json!({"grants":[{"serverId":"test-server","roleId":other_role},{"serverId":"test-server","roleId":role},{"serverId":"test-server","roleId":role},{"serverId":"other-server","roleId":other_role}]}),&owner).await;
    assert_eq!(body["grants"].as_array().unwrap().len(), 2, "{body}");
    call(
        &app,
        "PUT",
        &format!("/api/orgs/{org}/members/{winner}/grants"),
        json!({"grants":[]}),
        &c1,
    )
    .await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM server_grants WHERE user_id=$1")
        .bind(winner)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/orgs/{org}"),
            json!({"suspended":true}),
            &c1
        )
        .await
        .0,
        403
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/orgs/{org}"),
            json!({"banMessage":"{unknown}"}),
            &c1
        )
        .await
        .0,
        400
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/orgs/{org}"),
            json!({"discordInviteUrl":"evil.test"}),
            &c1
        )
        .await
        .0,
        400
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/orgs/{org}"),
            json!({"banMessage":"{reason} / {uid}"}),
            &c1
        )
        .await
        .0,
        200
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/orgs/{org}/members/{u1}"),
            json!({"role":"member"}),
            &c1
        )
        .await
        .0,
        400
    );
    call(
        &app,
        "PATCH",
        &format!("/api/orgs/{org}/members/{winner}"),
        json!({"role":"owner"}),
        &c1,
    )
    .await;
    // Scoped revocation must leave credentials minted for another organisation intact.
    for (key, o) in [("owned-key", &org), ("elsewhere-key", &other)] {
        sqlx::query("INSERT INTO api_keys(id,org_id,label,key_hash,hint,capabilities,created_by) VALUES($1,$2,$1,$1,'test','[]',$3)").bind(key).bind(o).bind(&u1).execute(&db).await.unwrap();
    }
    let path1 = format!("/api/orgs/{org}/members/{u1}");
    let path2 = format!("/api/orgs/{org}/members/{winner}");
    let (a, b) = tokio::join!(
        call(&app, "PATCH", &path1, json!({"role":"member"}), &owner),
        call(&app, "PATCH", &path2, json!({"role":"member"}), &owner)
    );
    assert!(
        matches!((a.0.as_u16(), b.0.as_u16()), (200, 400) | (400, 200)),
        "{a:?} {b:?}"
    );
    let elsewhere_active: bool =
        sqlx::query_scalar("SELECT revoked_at IS NULL FROM api_keys WHERE id='elsewhere-key'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(elsewhere_active);
    // Restore the first owner to test account disable revokes all minted credentials and sessions.
    call(&app, "PATCH", &path1, json!({"role":"owner"}), &owner).await;
    call(&app, "PATCH", &path2, json!({"role":"member"}), &owner).await;
    let (status, body, _) = call(
        &app,
        "PATCH",
        &format!("/api/users/{u1}"),
        json!({"disabled":true}),
        &owner,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(call(&app, "GET", "/api/orgs", json!({}), &c1).await.0, 401);
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_keys WHERE created_by=$1 AND revoked_at IS NULL",
    )
    .bind(&u1)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(active, 0);
    call(
        &app,
        "PATCH",
        &format!("/api/users/{u1}"),
        json!({"disabled":false}),
        &owner,
    )
    .await;
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_keys WHERE created_by=$1 AND revoked_at IS NULL",
    )
    .bind(&u1)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(active, 0);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/users/{u1}"),
            json!({}),
            &owner
        )
        .await
        .0,
        400
    ); // sole owner restored
    let (status, body, _) = call(
        &app,
        "DELETE",
        &format!("/api/orgs/{org}"),
        json!({}),
        &owner,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let orphan: i64 =
        sqlx::query_scalar("SELECT count(*) FROM server_grants WHERE server_id='test-server'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(orphan, 0);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/users/{u1}"),
            json!({}),
            &owner
        )
        .await
        .0,
        200
    );
    db.close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
