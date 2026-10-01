mod common;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use chrono::{Duration, SecondsFormat, Utc};
use common::Db;
use serde_json::{Value, json};
use tower::ServiceExt;
use warcon_backend::{integrity_imports as imports, integrity_retention as retention};
fn external() -> Value {
    json!({"eventId":"external-1","eventAt":(Utc::now()-Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Millis,true),"instanceId":"boot","matchId":"round","eventTime":180,"map":"Europe","killerSteamId":"76561198000000001","victimSteamId":"76561198000000002","killerFaction":"RED","victimFaction":"BLUE","cause":"Id.Item.AK74M","distanceM":120,"headshot":true,"penetration":false,"playerCount":20})
}
#[test]
fn strict_history_units_identity_and_retention_policy() {
    let row = external();
    let now = Utc::now();
    assert_eq!(imports::parse(&row.to_string(), now).unwrap().len(), 1);
    assert_eq!(
        imports::parse(&json!({"events":[row.clone()]}).to_string(), now)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        imports::parse(&format!("{}\n{}", row, json!({"x":1})), now).is_err(),
        true
    );
    for (key, bad) in [
        ("distanceM", json!(5001)),
        ("eventTime", json!(-1)),
        ("eventTime", json!("180")),
        ("killerSteamId", json!("bad")),
        ("headshot", json!(1)),
        ("playerCount", json!(201)),
        ("cause", json!("Vehicle.Armor")),
        ("killerFaction", json!("BLUE")),
    ] {
        let mut row = row.clone();
        row[key] = bad;
        assert!(imports::parse(&row.to_string(), now).is_err(), "{key}");
    }
    assert!(imports::parse(&json!([row.clone(), row]).to_string(), now).is_err());
    assert!(retention::parse(&json!(retention::Policy::default())).is_some());
    for (key, bad) in [
        ("pageSize", json!(5)),
        ("maxAgeDays", json!(6)),
        ("maxRecords", json!(99)),
        ("autoDeleteEnabled", json!("true")),
    ] {
        let mut p = json!(retention::Policy::default());
        p[key] = bad;
        assert!(retention::parse(&p).is_none());
    }
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn multipart_staging_review_revocation_and_terminal_retention() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc)VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let raw = external().to_string();
    let data = format!(
        "--warconfixture\r\nContent-Disposition: form-data; name=\"sourceServer\"\r\n\r\nother-server\r\n--warconfixture\r\nContent-Disposition: form-data; name=\"file\"; filename=\"history.jsonl\"\r\nContent-Type: application/json\r\n\r\n{raw}\r\n--warconfixture--\r\n"
    );
    let request = Request::builder()
        .method("POST")
        .uri("/api/orgs/org/integrity/imports")
        .header("origin", "http://localhost:3000")
        .header("cookie", &owner)
        .header(
            "content-type",
            "multipart/form-data; boundary=warconfixture",
        )
        .body(Body::from(data))
        .unwrap();
    let response = db.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), 201);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let id = body["batch"]["id"].as_str().unwrap();
    assert_eq!(body["batch"]["status"], "STAGED");
    assert_eq!(body["batch"]["rowCount"], 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM kills")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        0
    );
    let path = format!("/api/orgs/org/integrity/imports/{id}");
    assert_eq!(
        db.call("POST", &path, json!({"decision":"APPROVED"}), &owner)
            .await
            .0,
        400
    );
    let (status, approved) = db
        .call(
            "POST",
            &path,
            json!({"decision":"APPROVED","confirmation":"APPROVE_EXTERNAL_INTEGRITY_DATA"}),
            &owner,
        )
        .await;
    assert_eq!(status, 200, "{approved}");
    assert_eq!(approved["batch"]["status"], "APPROVED");
    assert_eq!(
        db.call(
            "POST",
            &path,
            json!({"decision":"APPROVED","confirmation":"APPROVE_EXTERNAL_INTEGRITY_DATA"}),
            &owner
        )
        .await
        .0,
        409
    );
    assert_eq!(
        db.call("POST", &path, json!({"decision":"REJECTED"}), &owner)
            .await
            .0,
        200
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT baseline_status FROM integrity_model_state WHERE org_id='org'"
        )
        .fetch_one(&db.state.db)
        .await
        .unwrap(),
        "STALE"
    );
    let policy = json!({"autoDeleteEnabled":true,"pageSize":20,"maxRecords":100,"maxAgeDays":7});
    let path = "/api/server/server/integrity/history-retention";
    let (status, p) = db
        .call("PUT", path, json!({"revision":"","policy":policy}), &owner)
        .await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(
        db.call("PUT", path, json!({"revision":"","policy":policy}), &owner)
            .await
            .0,
        409
    );
    // Only terminal, inactive records older than seven days are eligible. Current match alerts stay.
    let match_id:i64=sqlx::query_scalar("INSERT INTO matches(server_id,map,started_at)VALUES('server','Europe',now()-interval '1 hour')RETURNING id").fetch_one(&db.state.db).await.unwrap();
    for (id, state, scope) in [
        ("old", "warning", 0),
        ("uncertain", "unknown", 0),
        ("current", "warning", match_id),
        ("recent", "warning", 0),
    ] {
        let record = json!({"serverId":"server","steamId":"76561198000000001","state":state,"scope":["server","boot",scope.to_string()]});
        sqlx::query("INSERT INTO site_settings(key,value,updated_at)VALUES($1,$2,now()-make_interval(days=>$3))").bind(format!("shortRisk:{id}")).bind(record).bind(if id=="recent"{1}else{20}).execute(&db.state.db).await.unwrap();
    }
    let result = retention::prune(&db.state, "server")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["short"], 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM site_settings WHERE key LIKE 'shortRisk:%'"
        )
        .fetch_one(&db.state.db)
        .await
        .unwrap(),
        3
    );
    let p = retention::policy(&db.state, "server").await.unwrap();
    assert_eq!(p["lastCleanup"]["removed"], 1);
    assert_eq!(p["policy"], policy);
    db.close().await;
}
