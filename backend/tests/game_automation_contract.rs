mod common;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use warcon_backend::{
    config_document as ini, faction_lock as f, faction_quota as q, game, game_automation as a,
    leadership::Leadership,
    observation_state::{Player, PresenceDiff},
    observer::Memory,
    skill_balance as b,
};
fn normalize(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), normalize(v))).collect())
        }
        Value::Array(a) => json!(a.iter().map(normalize).collect::<Vec<_>>()),
        _ => v.clone(),
    }
}
#[test]
fn original_game_automation_policies() {
    let cases: Value =
        serde_json::from_str(include_str!("../fixtures/game-automation.json")).unwrap();
    for c in cases["decisions"].as_array().unwrap() {
        let i = &c["input"];
        assert_eq!(
            f::decision(
                i["from"].as_str(),
                i["to"].as_str(),
                &["Blue".into(), "Red".into(), "Green".into()],
                i["authorized"] == true,
                i["transition"] == true,
                i["full"].as_bool(),
                i["leading"].as_bool()
            ),
            c["output"]
        )
    }
    for c in cases["leading"].as_array().unwrap() {
        assert_eq!(
            json!(f::leading(
                c["scores"].as_array().unwrap(),
                c["faction"].as_str().unwrap()
            )),
            c["output"]
        )
    }
    for c in cases["balance"].as_array().unwrap() {
        assert_eq!(
            normalize(
                &b::plan(
                    &c["status"],
                    c["players"].as_array().unwrap(),
                    c["lead"].as_f64().unwrap()
                )
                .unwrap_or(Value::Null)
            ),
            normalize(&c["output"]),
            "{c}"
        )
    }
    for c in cases["quota"].as_array().unwrap() {
        let blocked = c["blocked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect::<HashSet<_>>();
        assert_eq!(
            normalize(&q::plan(
                c["players"].as_array().unwrap(),
                &c["teams"],
                &c["limits"],
                &blocked
            )),
            normalize(&c["output"]),
            "{c}"
        )
    }
    for c in cases["teamCases"].as_array().unwrap() {
        assert_eq!(
            q::teams(c["scores"].as_array().unwrap()).unwrap_or(Value::Null),
            c["output"]
        )
    }
    for c in cases["quotaConfigs"].as_array().unwrap() {
        let out = q::validate(&c["input"]);
        if c["error"] == true {
            assert!(out.is_err())
        } else {
            assert_eq!(out.unwrap(), c["output"])
        }
    }
    for c in cases["weapons"].as_array().unwrap() {
        assert_eq!(
            a::restricted(c["cause"].as_str().unwrap_or(""), &c["rule"]),
            c["output"] == true
        )
    }
    for c in cases["stages"].as_array().unwrap() {
        assert_eq!(
            a::restriction_stage(
                c["event"].as_f64().unwrap(),
                c["now"].as_f64().unwrap(),
                c["last"].as_f64()
            ),
            c["output"] == "kick"
        )
    }
    for c in cases["scalar"].as_array().unwrap() {
        let text = c["text"].as_str().unwrap();
        let section = c["section"].as_str().unwrap();
        let key = c["key"].as_str().unwrap();
        assert_eq!(json!(ini::scalar(text, section, key)), c["before"]);
        assert_eq!(ini::set_scalar(text, section, key, "false"), c["after"])
    }
    let selected = Utc::now() - chrono::Duration::seconds(1);
    let p = json!({"instanceId":"boot","gameMatchId":"game","selectedClock":100});
    let k = json!({"killer":{"steamId":"76561198000000001"},"victim":{"steamId":"76561198000000002"},"ts":Utc::now(),"eventTime":101,"instanceId":"boot","matchId":"game"});
    assert!(b::fresh_death(
        &k,
        selected,
        &p,
        Utc::now().timestamp_millis()
    ));
    for field in ["teamKill", "suicide"] {
        let mut invalid = k.clone();
        invalid[field] = json!(true);
        assert!(!b::fresh_death(
            &invalid,
            selected,
            &p,
            Utc::now().timestamp_millis()
        ))
    }
    assert!(!b::fresh_death(
        &k,
        selected,
        &p,
        Utc::now().timestamp_millis() + 6000
    ));
}
#[derive(Clone, Default)]
struct Mock {
    calls: Arc<Mutex<Vec<(String, String, Value)>>>,
    fail: Arc<Mutex<bool>>,
}
async fn mock(State(m): State<Mock>, r: Request) -> Response {
    let path = r.uri().path().to_owned();
    let method = r.method().to_string();
    let body = to_bytes(r.into_body(), 100000).await.unwrap();
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    m.calls.lock().unwrap().push((method, path, body));
    if *m.fail.lock().unwrap() {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"code":"fixture"}})),
        )
            .into_response()
    } else {
        Json(json!({"ok":true})).into_response()
    }
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn held_effects_warning_unknown_vip_restore_and_dedupe() {
    let db = common::Db::new().await;
    let owner = db.user("owner", true).await;
    let mock_state = Mock::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn({
        let m = mock_state.clone();
        async move {
            axum::serve(listener, Router::new().fallback(mock).with_state(m))
                .await
                .unwrap()
        }
    });
    sqlx::query("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org')")
        .execute(&db.state.db)
        .await
        .unwrap();
    let secret =
        warcon_backend::crypto::encrypt_secret(&db.state.config.encryption_key, "fixture").unwrap();
    sqlx::query("INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc,allow_private)VALUES('server','org','Server','127.0.0.1',$1,'http',$2,true)").bind(port as i32).bind(secret).execute(&db.state.db).await.unwrap();
    let leader = Arc::new(Leadership::new(db.state.db.clone(), "test".into()));
    assert!(leader.renew().await.unwrap());
    assert!(db.state.runtime.leader.set(leader).is_ok());
    let mid:i64=sqlx::query_scalar("INSERT INTO matches(server_id,map,started_at)VALUES('server','Europe',now()-interval '20 minutes')RETURNING id").fetch_one(&db.state.db).await.unwrap();
    let now = Utc::now();
    let steam = "76561198000000001";
    let mut m = Memory::new(
        "server".into(),
        "org".into(),
        "Server".into(),
        now.timestamp_millis() - 1200000,
        30000,
    );
    m.started_at = now.timestamp_millis() - 1200000;
    m.status_at = now.timestamp_millis();
    m.observed_at = now.timestamp_millis();
    m.status = json!({"map":"Europe","matchSeconds":1000,"scoreCap":1000,"scores":[{"name":"Blue","score":150},{"name":"Red","score":30},{"name":"Green","score":0}]});
    m.players = vec![Player {
        steam_id: steam.into(),
        name: "P".into(),
        faction: Some("Red".into()),
        kills: 10,
        deaths: 1,
        cash: 0,
        ping: None,
    }];
    let client = game::Client::for_server(&db.state, "server").await.unwrap();
    sqlx::query("INSERT INTO player_sessions(server_id,steam_id,name,joined_at,last_seen)VALUES('server',$1,'P',now()-interval '20 minutes',now())").bind(steam).execute(&db.state.db).await.unwrap();
    let (s, v) = db
        .call(
            "POST",
            "/api/servers/server/numeric-limits",
            json!({"enabled":true,"kpm":2,"kd":null,"cash":null,"windowSeconds":60,"minKills":1}),
            &owner,
        )
        .await;
    assert_eq!(s.as_u16(), 200, "{v}");
    sqlx::query("UPDATE numeric_limit_rules SET updated_at=now()-interval '10 minutes'")
        .execute(&db.state.db)
        .await
        .unwrap();
    for (offset, kills) in [(70, 0), (40, 1), (10, 2)] {
        sqlx::query("INSERT INTO player_progress_samples(server_id,match_id,bucket,observed_at,players)VALUES('server',$1,$2,$3,$4)").bind(mid).bind((now.timestamp_millis()-offset*1000)/30000).bind(now-chrono::Duration::seconds(offset)).bind(json!([{"steamId":steam,"kills":kills,"deaths":1,"cash":0}])).execute(&db.state.db).await.unwrap();
    }
    a::numeric(&db.state, &client, &m, true, false)
        .await
        .unwrap();
    assert_eq!(mock_state.calls.lock().unwrap().len(), 1);
    assert!(mock_state.calls.lock().unwrap()[0].1.ends_with("/message"));
    a::numeric(&db.state, &client, &m, true, false)
        .await
        .unwrap();
    assert_eq!(mock_state.calls.lock().unwrap().len(), 1);
    sqlx::query("UPDATE numeric_limit_events SET updated_at=now()-interval '130 seconds'")
        .execute(&db.state.db)
        .await
        .unwrap();
    a::numeric(&db.state, &client, &m, true, false)
        .await
        .unwrap();
    assert!(mock_state.calls.lock().unwrap()[1].1.ends_with("/kick"));
    sqlx::query("DELETE FROM numeric_limit_events")
        .execute(&db.state.db)
        .await
        .unwrap();
    *mock_state.fail.lock().unwrap() = true;
    a::numeric(&db.state, &client, &m, true, false)
        .await
        .unwrap();
    *mock_state.fail.lock().unwrap() = false;
    let count = mock_state.calls.lock().unwrap().len();
    a::numeric(&db.state, &client, &m, true, false)
        .await
        .unwrap();
    assert_eq!(mock_state.calls.lock().unwrap().len(), count);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM numeric_limit_events LIMIT 1")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        "unknown"
    );
    sqlx::query("DELETE FROM numeric_limit_events")
        .execute(&db.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO site_settings(key,value)VALUES('qqVip',$1)").bind(json!({"entries":[{"serverId":"server","steamId":steam,"enabled":true,"allowOverkill":true,"whitelist":false}]})).execute(&db.state.db).await.unwrap();
    a::numeric(&db.state, &client, &m, true, false)
        .await
        .unwrap();
    assert_eq!(mock_state.calls.lock().unwrap().len(), count);
    sqlx::query(
        "INSERT INTO faction_lock_rules(server_id,enabled,grace_seconds)VALUES('server',true,30)",
    )
    .execute(&db.state.db)
    .await
    .unwrap();
    let diff = PresenceDiff {
        factioned: vec![json!({"from":"Blue","player":m.players[0]})],
        ..Default::default()
    };
    f::run(&db.state, &client, &m, &diff, true, false)
        .await
        .unwrap();
    assert_eq!(mock_state.calls.lock().unwrap().len(), count);
    sqlx::query("UPDATE faction_lock_events SET updated_at=now()-interval '9 seconds'")
        .execute(&db.state.db)
        .await
        .unwrap();
    f::run(
        &db.state,
        &client,
        &m,
        &PresenceDiff::default(),
        true,
        false,
    )
    .await
    .unwrap();
    let calls = mock_state.calls.lock().unwrap().clone();
    assert_eq!(calls[count].0, "PATCH");
    assert_eq!(calls[count].2["faction"], "Blue");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM faction_lock_events LIMIT 1")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        "restored"
    );
    sqlx::query("INSERT INTO weapon_restriction_rules(server_id,enabled,groups,updated_at)VALUES('server',true,'[\"items\"]',now()-interval '5 minutes')").execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO kills(server_id,ts,event_id,instance_id,match_id,match_row,event_time,map,killer_steam_id,victim_steam_id,victim_name,cause,tags)VALUES('server',now(),'k','boot','game',$1,1000,'Europe',$2,'76561198000000002','V','Id.Item.AK74M','[]')").bind(mid).bind(steam).execute(&db.state.db).await.unwrap();
    a::weapons(&db.state, &client, &m, false).await.unwrap();
    let count = mock_state.calls.lock().unwrap().len();
    assert!(
        mock_state
            .calls
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .ends_with("/kick")
    );
    a::weapons(&db.state, &client, &m, false).await.unwrap();
    assert_eq!(mock_state.calls.lock().unwrap().len(), count);
    task.abort();
    db.close().await;
}

#[derive(Clone)]
struct RichMock {
    calls: Arc<Mutex<Vec<(String, String, Value)>>>,
    roster: Arc<Mutex<Vec<Value>>>,
    text: Arc<Mutex<String>>,
}
async fn rich_mock(State(m): State<RichMock>, r: Request) -> Response {
    let path = r.uri().path().to_owned();
    let method = r.method().to_string();
    let body = to_bytes(r.into_body(), 100000).await.unwrap();
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    m.calls
        .lock()
        .unwrap()
        .push((method.clone(), path.clone(), value.clone()));
    let reply = match (method.as_str(), path.as_str()) {
        ("GET", "/v1/capabilities") => {
            json!({"routes":["PATCH /v1/players/:steamId","PUT /v1/config"],"config":{"writable":true}})
        }
        ("GET", "/v1/config") => {
            json!({"revision":"fixture","text":m.text.lock().unwrap().clone(),"sections":[],"writable":true})
        }
        ("PUT", "/v1/config") => {
            *m.text.lock().unwrap() = String::from_utf8(body.to_vec()).unwrap();
            json!({"ok":true,"revision":"fixture"})
        }
        ("GET", "/v1/status") => {
            json!({"map":"Europe","matchSeconds":1001,"scoreCap":1000,"factionScores":[{"name":"Blue","score":150},{"name":"Red","score":30},{"name":"Green","score":0}]})
        }
        ("GET", "/v1/players") => json!({"players":m.roster.lock().unwrap().clone()}),
        ("PATCH", _) if path.starts_with("/v1/players/") => {
            let steam = path.trim_start_matches("/v1/players/");
            for p in m
                .roster
                .lock()
                .unwrap()
                .iter_mut()
                .filter(|p| p["steamId"] == steam)
            {
                p["faction"] = value["faction"].clone();
            }
            json!({"ok":true})
        }
        _ => json!({"ok":true}),
    };
    Json(reply).into_response()
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn death_only_balance_and_quota_config_round_trip() {
    let db = common::Db::new().await;
    let owner = db.user("owner", true).await;
    let roster: Vec<_> = (1..=6).map(|n|json!({"steamId":format!("7656119800000000{n}"),"name":"P","faction":if n%2==1{"Blue"}else{"Green"},"kills":n,"deaths":1})).collect();
    let mock = RichMock {
        calls: Arc::default(),
        roster: Arc::new(Mutex::new(roster.clone())),
        text: Arc::new(Mutex::new(
            "[/Script/WDGame.WDGameStateSession]\nbLockOverpopulatedTeamsConfig=true\n".into(),
        )),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn({
        let mock = mock.clone();
        async move {
            axum::serve(listener, Router::new().fallback(rich_mock).with_state(mock))
                .await
                .unwrap()
        }
    });
    sqlx::query("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org')")
        .execute(&db.state.db)
        .await
        .unwrap();
    let secret =
        warcon_backend::crypto::encrypt_secret(&db.state.config.encryption_key, "fixture").unwrap();
    sqlx::query("INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc,allow_private)VALUES('server','org','Server','127.0.0.1',$1,'http',$2,true)").bind(port as i32).bind(secret).execute(&db.state.db).await.unwrap();
    let leader = Arc::new(Leadership::new(db.state.db.clone(), "test".into()));
    assert!(leader.renew().await.unwrap());
    assert!(db.state.runtime.leader.set(leader).is_ok());
    let mid:i64=sqlx::query_scalar("INSERT INTO matches(server_id,map,started_at)VALUES('server','Europe',now()-interval '20 minutes')RETURNING id").fetch_one(&db.state.db).await.unwrap();
    let rule_at:chrono::DateTime<Utc>=sqlx::query_scalar("INSERT INTO skill_balance_rules(server_id,enabled,lead_points,grace_seconds)VALUES('server',true,40,180)RETURNING updated_at").fetch_one(&db.state.db).await.unwrap();
    // JavaScript freezes milliseconds; PostgreSQL retains the full microsecond timestamp.
    let ranked: Vec<_> = roster
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut p = p.clone();
            p["kpm"] = json!(if i % 2 == 0 { 4.0 } else { 1.0 });
            p["kd"] = json!(if i % 2 == 0 { 4.0 } else { 1.0 });
            p
        })
        .collect();
    let mut plan=b::plan(&json!({"scores":[{"name":"Blue","score":150},{"name":"Red","score":30},{"name":"Green","score":0}]}),&ranked,40.0).unwrap();
    plan["selectedClock"] = json!(1000);
    plan["instanceId"] = json!("boot");
    plan["gameMatchId"] = json!("game");
    plan["ruleUpdatedAt"] = json!(a::version(rule_at));
    let moves:Vec<_>=plan["pairs"].as_array().unwrap().iter().flat_map(|p|[json!({"steamId":p["strong"]["steamId"],"from":"Blue","to":"Green","state":"waiting"}),json!({"steamId":p["weak"]["steamId"],"from":"Green","to":"Blue","state":"waiting"})]).collect();
    sqlx::query("INSERT INTO skill_balance_runs(id,server_id,match_id,state,reason,plan,moves,created_at)VALUES('plan','server',$1,'waiting_death','fixture',$2,$3,now()-interval '1 second')").bind(mid).bind(&plan).bind(json!(moves)).execute(&db.state.db).await.unwrap();
    let death = json!({"killer":{"steamId":"76561198000000009"},"victim":{"steamId":moves[0]["steamId"]},"eventId":"death","ts":Utc::now(),"eventTime":1001,"instanceId":"boot","matchId":"game","matchRow":mid});
    b::deaths(&db.state, "server", &[death.clone()])
        .await
        .unwrap();
    let calls = mock.calls.lock().unwrap().clone();
    assert_eq!(
        calls.iter().filter(|(m, _, _)| m == "PATCH").count(),
        1,
        "{calls:?}"
    );
    assert!(!calls.iter().any(|(_, p, _)| p.ends_with("/kill")));
    let saved: Value = sqlx::query_scalar("SELECT moves FROM skill_balance_runs WHERE id='plan'")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert_eq!(saved[0]["state"], "confirmed");
    b::deaths(&db.state, "server", &[death]).await.unwrap();
    assert_eq!(
        mock.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _, _)| m == "PATCH")
            .count(),
        1
    );
    sqlx::query("UPDATE skill_balance_rules SET enabled=false")
        .execute(&db.state.db)
        .await
        .unwrap();
    let (_, current) = db
        .call(
            "GET",
            "/api/servers/server/faction-quota",
            Value::Null,
            &owner,
        )
        .await;
    let mut config = current["config"].clone();
    config["enabled"] = json!(true);
    config["limits"] = json!({"blue":2,"red":2,"green":2});
    config["graceSeconds"] = json!(30);
    let (status, result) = db
        .call("POST", "/api/servers/server/faction-quota", config, &owner)
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["config"]["originalJoinLock"], "true");
    assert_eq!(result["config"]["waitMatchId"], mid);
    assert_eq!(
        ini::scalar(
            &mock.text.lock().unwrap(),
            "/Script/WDGame.WDGameStateSession",
            "bLockOverpopulatedTeamsConfig"
        )
        .as_deref(),
        Some("false")
    );
    let mut config = result["config"].clone();
    config["enabled"] = json!(false);
    let (status, result) = db
        .call("POST", "/api/servers/server/faction-quota", config, &owner)
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        ini::scalar(
            &mock.text.lock().unwrap(),
            "/Script/WDGame.WDGameStateSession",
            "bLockOverpopulatedTeamsConfig"
        )
        .as_deref(),
        Some("true")
    );
    assert_eq!(result["config"]["originalJoinLock"], Value::Null);
    task.abort();
    db.close().await;
}
