mod common;
use axum::http::StatusCode;
use serde_json::{Value, json};
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn visibility_history_marks_dossier_diagnostics_and_purge() {
    let db = common::Db::new().await;
    let owner = db.user("owner", true).await;
    let viewer = db.user("viewer", false).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org');INSERT INTO org_members(org_id,user_id,role)VALUES('org','viewer','member');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc)VALUES('server','org','Visible','example.invalid',1,'http','fixture'),('hidden','org','Hidden','example.invalid',1,'http','fixture');INSERT INTO org_roles(id,org_id,name,capabilities)VALUES('view','org','Viewer','[\"server.view\"]');INSERT INTO server_grants(server_id,user_id,role_id)VALUES('server','viewer','view')").execute(&db.state.db).await.unwrap();
    let steam = "76561198000000001";
    sqlx::query("INSERT INTO player_sessions(server_id,steam_id,name,faction,joined_at,last_seen,kills,deaths,cash)VALUES('server',$1,'VisibleAlias','Blue',now()-interval '10 minutes',now(),10,2,200),('hidden',$1,'HiddenAlias','Green',now()-interval '20 minutes',now(),100,10,2000)").bind(steam).execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO player_marks(org_id,steam_id,watched,reason,updated_by_name)VALUES('org',$1,true,'Private staff reason','owner')").bind(steam).execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO player_notes(org_id,steam_id,author_id,author_name,body)VALUES('org',$1,'owner','owner','Private staff note')").bind(steam).execute(&db.state.db).await.unwrap();
    for credential in [&owner, &viewer] {
        let (s, v) = db
            .call(
                "GET",
                "/api/servers/server/players/seen",
                Value::Null,
                credential,
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["total"], 1);
        assert_eq!(v["players"][0]["kills"], 10);
        assert_eq!(v["players"][0]["servers"], 1);
    }
    let (s, v) = db
        .call(
            "GET",
            &format!("/api/servers/server/players/{steam}"),
            Value::Null,
            &viewer,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let d = &v["dossier"];
    assert_eq!(d["names"], json!(["VisibleAlias"]));
    assert_eq!(d["notes"], json!([]));
    assert_eq!(d["watch"]["reason"], "");
    assert_eq!(d["summary"]["sessions"], 1);
    assert_eq!(d["orgServerCount"], 2);
    assert_eq!(d["perServer"].as_array().unwrap().len(), 1);
    assert_eq!(d["risk"]["score"], 15.0);
    assert!(!v.to_string().contains("HiddenAlias"));
    assert!(!v.to_string().contains("Private staff"));
    let (s, v) = db
        .call(
            "GET",
            &format!("/api/servers/server/players/{steam}"),
            Value::Null,
            &owner,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["dossier"]["summary"]["sessions"], 2);
    assert_eq!(v["dossier"]["notes"].as_array().unwrap().len(), 1);
    let (s, v) = db
        .call(
            "GET",
            &format!("/api/servers/server/players/marks?ids={steam}&names=VisibleAlias"),
            Value::Null,
            &viewer,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["marks"][0]["watched"], true);
    assert_eq!(v["marks"][0]["reason"], "");
    assert_eq!(v["marks"][0]["firstVisit"], true);
    let (s, v) = db
        .call(
            "GET",
            &format!("/api/servers/server/players/{steam}/career"),
            Value::Null,
            &viewer,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = db
        .call("GET", "/api/orgs/org/players", Value::Null, &viewer)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    let (s, v) = db
        .call(
            "GET",
            "/api/orgs/org/players?flag=watched",
            Value::Null,
            &owner,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["players"][0]["kills"], 110);
    let (s, v) = db.call("GET", "/api/health", Value::Null, "").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v.get("worker").is_none());
    let (s, v) = db.call("GET", "/api/health", Value::Null, &owner).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["worker"]["process"].is_object(), "{v}");
    let (s, v) = db
        .call("GET", "/api/admin/overview", Value::Null, &owner)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["overview"]["seen"]["all"], 1);
    assert_eq!(v["overview"]["fleet"]["servers"], 2);
    let (s, _) = db
        .call("GET", "/api/admin/overview", Value::Null, &viewer)
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = db
        .call(
            "GET",
            "/api/servers/server/plugin-snapshot",
            Value::Null,
            &viewer,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["snapshot"]["server"]["name"], "Visible");
    let (s, _) = db
        .call(
            "POST",
            "/api/servers/server/stats/purge",
            json!({"name":"Visible"}),
            &viewer,
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = db
        .call(
            "POST",
            "/api/servers/server/stats/purge",
            json!({"name":"wrong"}),
            &owner,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = db
        .call(
            "POST",
            "/api/servers/server/stats/purge",
            json!({"name":"Visible"}),
            &owner,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["counts"]["matches"], 0);
    let n: i64 = sqlx::query_scalar("SELECT count(*)FROM player_sessions")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(n, 2);
    db.close().await;
}
