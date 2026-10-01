mod common;
use axum::http::StatusCode;
use chrono::{Duration, SecondsFormat, Utc};
use common::Db;
use serde_json::{Value, json};
use std::collections::HashMap;
use warcon_backend::{integrity_cases as cases, integrity_score, integrity_windows};
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn frozen_evidence_human_review_idempotence_and_atomic_ban() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    let member = db.user("member", false).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org'); INSERT INTO org_members(org_id,user_id,role) VALUES('org','member','member'); INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc) VALUES('server','org','Server','example.invalid',1234,'http','fixture');").execute(&db.state.db).await.unwrap();
    let at = Utc::now();
    let received = at - Duration::seconds(5);
    let timestamp = received.to_rfc3339_opts(SecondsFormat::Millis, true);
    let steam = "76561198000000001";
    let mut batch = Vec::new();
    for (id, clock, headshot) in [("a", 100., true), ("b", 110., false)] {
        sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,killer_name,killer_faction,victim_steam_id,victim_name,victim_faction,cause,distance_m,headshot,tags,faction_bracketed,faction_observed_at) VALUES($1,'server',$2,'boot','round',$3,'Europe',$4,'Name','A','76561198000000002','Victim','B','Id.Item.AK74M',123,$5,'[]',true,$1)").bind(received).bind(id).bind(clock).bind(steam).bind(headshot).execute(&db.state.db).await.unwrap();
        batch.push(json!({"eventId":id,"instanceId":"boot","eventTime":clock,"map":"Europe","ts":timestamp,"killer":{"steamId":steam,"faction":"A"},"victim":{"steamId":"76561198000000002","faction":"B"},"cause":"Id.Item.AK74M","distanceM":123,"headshot":headshot,"suicide":false,"teamKill":false,"tags":[],"factionBracketed":true,"factionObservedAt":timestamp}));
    }
    sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,victim_steam_id,victim_name,tags) VALUES($1,'server','future','boot','round',105,'Europe',$2,'76561198000000002','Victim','[]')").bind(at+Duration::seconds(10)).bind(steam).execute(&db.state.db).await.unwrap();
    let mut windows = integrity_windows::Windows::default();
    let output = integrity_windows::generate(
        &mut windows,
        "server",
        &batch,
        &HashMap::new(),
        &integrity_score::defaults(),
    );
    let finding = output.snapshots.last().unwrap();
    let mut tx = db.state.db.begin().await.unwrap();
    let score = json!({"score":40,"breakdown":[]});
    let signals = json!({"vacBans":0,"gameBans":0,"daysSinceLastBan":null,"repeatHighRiskWindow":false,"uniqueReporters":0});
    let rules = integrity_score::defaults();
    let case = cases::freeze(
        &mut tx,
        &cases::Freeze {
            org: "org",
            server: "server",
            steam,
            finding,
            score: &score,
            signals: &signals,
            steam_known: false,
            version: 1,
            rules: &rules,
            created: at,
            score_id: None,
            statistical: None,
            trigger: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let row: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM integrity_cases c WHERE id=$1")
        .bind(&case)
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(row["confidence"], "B");
    assert_eq!(row["snapshot"]["eventIds"], json!(["a", "b"]));
    let events: Vec<Value> = sqlx::query_scalar(
        "SELECT event FROM integrity_case_events WHERE case_id=$1 ORDER BY event_id",
    )
    .bind(&case)
    .fetch_all(&db.state.db)
    .await
    .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["distanceM"], 123);
    assert_eq!(events[0]["ts"], timestamp);
    let path = format!("/api/orgs/org/integrity/cases/{case}/labels");
    let review = json!({"label":"CONFIRMED_ABUSE","reason":"已经核对原始击杀证据，确认违规。"});
    assert_eq!(
        db.call("POST", &path, review.clone(), &member).await.0,
        StatusCode::FORBIDDEN
    );
    let (status, label) = db.call("POST", &path, review.clone(), &owner).await;
    assert_eq!(status, StatusCode::OK, "{label}");
    assert_eq!(label["label"]["penalty"]["source"], "REVIEW");
    assert_eq!(label["label"]["penalty"]["reused"], false);
    assert!(!label["label"]["penalty"]["effectiveAt"].is_null());
    let expiry = label["label"]["penalty"]["expiresAt"].clone();
    let (status, repeated) = db.call("POST", &path, review.clone(), &owner).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated["label"]["id"], label["label"]["id"]);
    assert_eq!(repeated["label"]["penalty"]["expiresAt"], expiry);
    assert_eq!(repeated["label"]["penalty"]["reused"], true);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let row: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM integrity_cases c WHERE id=$1")
        .bind(&case)
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(row["status"], "REVIEWED");
    assert_eq!(row["reviewed_by"], "owner");
    let list: String =
        sqlx::query_scalar("SELECT list_id FROM server_lists WHERE server_id='server'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    let entry: Value = sqlx::query_scalar(
        "SELECT to_jsonb(e) FROM list_entries e WHERE list_id=$1 AND steam_id=$2",
    )
    .bind(&list)
    .bind(steam)
    .fetch_one(&db.state.db)
    .await
    .unwrap();
    assert!(entry["removed_at"].is_null());
    for (id, status, player) in [
        ("cleared", "AI_CLEARED", steam),
        ("invalid", "OPEN", "invalid"),
    ] {
        sqlx::query("INSERT INTO integrity_cases(id,org_id,server_id,steam_id,created_at,status,confidence,trigger,rule_version,risk_score,risk_breakdown,snapshot) VALUES($1,'org','server',$2,now(),$3,'B','test',1,10,'[]','{}')").bind(id).bind(player).bind(status).execute(&db.state.db).await.unwrap();
        assert_eq!(
            db.call(
                "POST",
                &format!("/api/orgs/org/integrity/cases/{id}/labels"),
                review.clone(),
                &owner
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM integrity_labels WHERE case_id IN ('cleared','invalid')",
    )
    .fetch_one(&db.state.db)
    .await
    .unwrap();
    assert_eq!(count, 0);
    let reviewed: bool = sqlx::query_scalar(
        "SELECT reviewed_at IS NOT NULL FROM integrity_cases WHERE id='invalid'",
    )
    .fetch_one(&db.state.db)
    .await
    .unwrap();
    assert!(!reviewed);
    db.close().await;
}
