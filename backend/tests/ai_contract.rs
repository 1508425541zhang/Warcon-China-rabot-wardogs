mod common;
use axum::http::StatusCode;
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{ai_evidence, ai_protocol as p, ai_queue, integrity_ai as ai};
#[test]
fn original_protocol_and_lossless_tables() {
    let fixtures: Value = serde_json::from_str(include_str!("../fixtures/ai.json")).unwrap();
    assert_eq!(p::SYSTEM, fixtures["system"].as_str().unwrap());
    assert_eq!(p::PROMPT_VERSION, fixtures["promptVersion"]);
    for f in fixtures["reviews"].as_array().unwrap() {
        let r = p::review(&f["input"]);
        assert_eq!(r.is_ok(), f["valid"].as_bool().unwrap(), "{}", f["input"]);
        if let Ok(r) = r {
            assert_eq!(
                json!([p::triage(&r, true), p::triage(&r, false)]),
                f["expected"]
            );
        }
    }
    for f in fixtures["snapshots"].as_array().unwrap() {
        let actual = p::numeric_checks(&f["input"]);
        assert_eq!(actual["scope"], f["expected"]["scope"]);
        assert_eq!(
            actual["kpm180"]["matches"],
            f["expected"]["kpm180"]["matches"]
        );
        for key in ["infantryKills", "recorded", "recomputed"] {
            assert_eq!(
                actual["kpm180"][key].as_f64(),
                f["expected"]["kpm180"][key].as_f64()
            )
        }
    }
    for f in fixtures["urls"].as_array().unwrap() {
        assert_eq!(
            p::api_base(f["input"].as_str().unwrap()).ok(),
            f["expected"].as_str().map(str::to_owned)
        )
    }
    let records = vec![
        json!({"name":"A","value":null,"nested":{"v":5}}),
        json!({"name":"B","value":3}),
        json!({"name":"A","value":4,"nested":[1,2]}),
    ];
    let table = ai_evidence::compact_table(&records);
    for (i, record) in records.iter().enumerate() {
        let mut decoded = json!({});
        for (j, key) in table["columns"].as_array().unwrap().iter().enumerate() {
            let key = key.as_str().unwrap();
            let v = &table["rows"][i][j];
            if v.get("$missing") == Some(&json!(true)) {
                continue;
            }
            decoded[key] = if !v.is_null() && table["dictionaries"].get(key).is_some() {
                table["dictionaries"][key][v.as_u64().unwrap() as usize].clone()
            } else {
                v.clone()
            };
        }
        assert_eq!(&decoded, record)
    }
}
fn review(percent: i32) -> Value {
    json!({"verdict":if percent>=65{"建议复核"}else{"建议通过"},"suspicionPercent":percent,"evidenceQuality":"中","summary":"核对原始字段后完成审核","reasons":[{"text":"核对数字","evidence":"case.snapshot.kpm180"}],"alternatives":[],"contradictions":[],"missingEvidence":[]})
}
async fn case(db: &Db, id: &str) {
    sqlx::query("INSERT INTO integrity_cases(id,org_id,server_id,steam_id,created_at,confidence,trigger,rule_version,risk_score,risk_breakdown,snapshot,statistical) VALUES($1,'org','server','76561198000000001',date_trunc('milliseconds',now()),'B','STATISTICAL_WINDOW',1,6,'[]','{\"roundId\":\"boot:match:1\",\"instanceId\":\"boot\",\"clockTo\":200,\"infantryKills\":6,\"kpm180\":2,\"eventIds\":[\"a\"]}','{\"level\":\"WATCH\"}')").bind(id).execute(&db.state.db).await.unwrap();
}
async fn finish(db: &Db, id: &str, r: Value) {
    sqlx::query("INSERT INTO integrity_ai_jobs(case_id,state,claim_token,lease_until)VALUES($1,'running','token',now()+interval '3 minutes') ON CONFLICT(case_id) DO UPDATE SET state='running',claim_token='token',lease_until=now()+interval '3 minutes'").bind(id).execute(&db.state.db).await.unwrap();
    let settings = ai::settings(&db.state, "org").await.unwrap().unwrap();
    let version = warcon_backend::integrity_enforcement::date(&settings["updated_at"]).unwrap();
    let mut tx = db.state.db.begin().await.unwrap();
    ai_queue::finish(&mut tx, id, "token", &r, version)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn scoped_evidence_quota_cache_and_human_precedence() {
    let db = Db::new().await;
    let owner = db.user("owner", true).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc)VALUES('server','org','Server','example.invalid',1234,'http','fixture');INSERT INTO integrity_rules(org_id,assessment_mode,config)VALUES('org','statistical','{}');").execute(&db.state.db).await.unwrap();
    let path = "/api/servers/server/integrity/ai";
    let config = json!({"baseUrl":"https://api.example.com/v1/","model":"test","apiKey":"fixture-private-key","dailyLimit":500});
    let (status, result) = db
        .call(
            "POST",
            path,
            json!({"operation":"save","settings":config}),
            &owner,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(!result.to_string().contains("fixture-private-key"));
    assert!(result["settings"].get("keyEnc").is_none());
    assert_eq!(result["settings"]["dailyLimit"], 500);
    let raw = ai::settings(&db.state, "org").await.unwrap().unwrap();
    assert!(raw["key_enc"].as_str().unwrap().starts_with("v1."));
    case(&db, "low").await;
    case(&db, "mid").await;
    case(&db, "high").await;
    case(&db, "human").await;
    case(&db, "protected").await;
    case(&db, "changed").await;
    sqlx::query("INSERT INTO matches(server_id,map,started_at,ended_at)VALUES('server','Europe',now()-interval '10 minutes',now())").execute(&db.state.db).await.unwrap();
    let match_id: i64 = sqlx::query_scalar("SELECT id FROM matches WHERE server_id='server'")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    for (id, killer, victim, clock, post) in [
        ("a", "76561198000000001", "76561198000000002", 190., false),
        (
            "death",
            "76561198000000002",
            "76561198000000001",
            195.,
            false,
        ),
        (
            "context",
            "76561198000000003",
            "76561198000000004",
            180.,
            false,
        ),
        ("post", "76561198000000001", "76561198000000005", 240., true),
        (
            "other",
            "76561198000000001",
            "76561198000000006",
            200.,
            false,
        ),
    ] {
        sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,victim_steam_id,victim_name,cause,headshot,distance_m,tags,match_row)VALUES(now()+make_interval(secs=>$1),'server',$2,$3,'round',$4,'Europe',$5,$6,'Victim','Id.Item.AK74M',true,150,'[\"Penetration\"]',$7)").bind(if post{10.}else{-5.}).bind(id).bind(if id=="other"{"anotherBoot"}else{"boot"}).bind(clock).bind(killer).bind(victim).bind(match_id).execute(&db.state.db).await.unwrap();
    }
    sqlx::query("INSERT INTO integrity_case_events(case_id,instance_id,event_id,event)VALUES('low','boot','a','{\"killerSteamId\":\"76561198000000001\",\"ts\":\"2026-01-01T00:00:00.000Z\",\"headshot\":true,\"distanceM\":150}')").execute(&db.state.db).await.unwrap();
    let bundle = ai_evidence::bundle(&db.state, "org", "server", "low")
        .await
        .unwrap();
    assert_eq!(
        bundle["evidence"]["counts"],
        json!({"records":4,"subjectKills":2,"subjectDeaths":1,"subjectSuicides":0,"postCaseRecords":1})
    );
    assert_eq!(bundle["evidence"]["combatEvents"]["count"], 4);
    assert_eq!(bundle["evidence"]["scope"]["kind"], "resolved_round");
    assert_eq!(bundle["numericChecks"]["kpm180"]["matches"], true);
    assert_eq!(
        bundle["evidence"]["coverage"]["nonFatalDamage"]["available"],
        false
    );
    assert!(
        ai_evidence::bundle(&db.state, "wrong", "server", "low")
            .await
            .is_err()
    );
    let output = review(20);
    let response = json!({"choices":[{"message":{"content":output.to_string()},"finish_reason":"stop"}],"usage":{"total_tokens":100}});
    let called = ai::call_with(
        &db.state,
        "org",
        "review",
        Some(&bundle),
        true,
        move |base, key, path, body| async move {
            assert_eq!(base, "https://api.example.com/v1");
            assert_eq!(key, "fixture-private-key");
            assert_eq!(path, "chat/completions");
            let body = body.unwrap();
            assert_eq!(body["max_tokens"], 1200);
            assert!(
                body["messages"][1]["content"]
                    .as_str()
                    .unwrap()
                    .contains("combatEvents")
            );
            Ok(response)
        },
    )
    .await
    .unwrap();
    assert_eq!(called.response["cached"], false);
    let cached = ai::call_with(
        &db.state,
        "org",
        "review",
        Some(&bundle),
        true,
        |_, _, _, _| async { panic!("cached review must not request provider") },
    )
    .await
    .unwrap();
    assert_eq!(cached.response["cached"], true);
    assert_eq!(
        ai::settings(&db.state, "org").await.unwrap().unwrap()["daily_requests"],
        1
    );
    let limited = ai::call_with(&db.state, "org", "test", None, true, |_, _, _, _| async {
        panic!("rate limited")
    })
    .await
    .err()
    .unwrap();
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    finish(&db, "low", called.response["result"].clone()).await;
    finish(&db, "mid", review(50)).await;
    finish(&db, "high", review(95)).await;
    sqlx::query("UPDATE integrity_cases SET status='REVIEWED',reviewed_at=now(),reviewed_by='owner' WHERE id='human'").execute(&db.state.db).await.unwrap();
    finish(&db, "human", review(20)).await;
    sqlx::query("INSERT INTO integrity_action_eligibility(case_id,last_attempt_at)VALUES('protected',now())").execute(&db.state.db).await.unwrap();
    finish(&db, "protected", review(20)).await;
    for (id, status, disposition) in [
        ("low", "AI_CLEARED", "AI_CLEARED"),
        ("mid", "AI_ARCHIVED", "AI_ARCHIVED"),
        ("high", "OPEN", "ADMIN_REVIEW"),
        ("human", "REVIEWED", "SKIPPED_REVIEWED"),
        ("protected", "OPEN", "ADMIN_REVIEW"),
    ] {
        let c:Value=sqlx::query_scalar("SELECT jsonb_build_object('status',c.status,'disposition',j.result->>'disposition') FROM integrity_cases c JOIN integrity_ai_jobs j ON j.case_id=c.id WHERE c.id=$1").bind(id).fetch_one(&db.state.db).await.unwrap();
        assert_eq!(c, json!({"status":status,"disposition":disposition}));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM kills")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        5
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM integrity_actions")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        0
    );
    assert!(
        ai_evidence::bundle(&db.state, "org", "server", "low")
            .await
            .err()
            .is_some_and(|e| e.status == StatusCode::GONE)
    );
    sqlx::query("INSERT INTO integrity_ai_jobs(case_id,state,claim_token,lease_until)VALUES('changed','running','old',now()+interval '3 minutes')").execute(&db.state.db).await.unwrap();
    let version = called.settings_version;
    sqlx::query("UPDATE integrity_ai_settings SET updated_at=updated_at+interval '1 second' WHERE org_id='org'").execute(&db.state.db).await.unwrap();
    let mut tx = db.state.db.begin().await.unwrap();
    ai_queue::finish(&mut tx, "changed", "old", &review(20), version)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM integrity_ai_jobs WHERE case_id='changed'"
        )
        .fetch_one(&db.state.db)
        .await
        .unwrap(),
        "pending"
    );
    let mut large = bundle.clone();
    large["oversize"] = json!("x".repeat(256000));
    let e = ai::call_with(
        &db.state,
        "org",
        "review",
        Some(&large),
        true,
        |_, _, _, _| async { panic!("oversized evidence") },
    )
    .await
    .err()
    .unwrap();
    assert_eq!(e.status, StatusCode::PAYLOAD_TOO_LARGE);
    db.close().await;
}
