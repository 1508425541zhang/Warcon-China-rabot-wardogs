use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::{PgPool, postgres::PgConnectOptions};
use tower::ServiceExt;
use warcon_backend::{
    api,
    config::{AppState, Config},
    crypto,
    leadership::Leadership,
    migrations,
};

async fn request(
    app: axum::Router,
    method: &str,
    path: &str,
    key: &str,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(cookie) = key.strip_prefix("cookie:") {
        builder = builder
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
    } else {
        builder = builder.header("authorization", format!("Bearer {key}"))
    }
    let response = app
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn original_schema_auth_pagination_transactions_and_worker_fencing() {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let options: PgConnectOptions = url.parse().unwrap();
    let admin = PgPool::connect_with(options.clone()).await.unwrap();
    let database = format!("warcon_rust_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let db = PgPool::connect_with(options.database(&database))
        .await
        .unwrap();
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../drizzle");
    let count = migrations::migrate(&db, &folder).await.unwrap();
    assert!(count > 40);
    assert_eq!(migrations::migrate(&db, &folder).await.unwrap(), 0);
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug) VALUES('org-a','A community','a'),('org-b','B community','b'); INSERT INTO servers(id,org_id,name,host,port,password_enc,feed_token_hash) VALUES('server-a','org-a','A','example.org',8080,'unused','configured'),('server-b','org-b','B','example.net',8080,'unused',NULL)").execute(&db).await.unwrap();
    let key = crypto::mint_token();
    sqlx::query("INSERT INTO api_keys(id,org_id,label,key_hash,hint,capabilities) VALUES('test-key','org-a','fixture',$1,'test',$2)").bind(crypto::hash_token(&key)).bind(json!(["server.view","players.notes"])).execute(&db).await.unwrap();
    let state = AppState {
        db: db.clone(),
        config: Config::for_test(),
    };
    let app = api::router(state.clone());
    let (status, _) = request(
        app.clone(),
        "GET",
        "/api/servers/server-b/kills",
        &key,
        Value::Null,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "API keys must not cross organisations"
    );
    sqlx::raw_sql("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,killer_name,victim_steam_id,victim_name,cause,headshot,tags) VALUES ('2026-10-01T00:00:00Z','server-a','k1','boot','round',10,'map','76561198388453389','A_%','76561198000000001','Victim','Id.Item.AK74M',true,'[]'),('2026-10-01T00:00:00Z','server-a','k2','boot','round',20,'map','76561198388453389','A_%','76561198000000002','Victim','Id.Item.AK74M',false,'[]')").execute(&db).await.unwrap();
    let (status, value) = request(
        app.clone(),
        "GET",
        "/api/servers/server-a/kills?limit=1&count=1",
        &key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["total"], 2);
    assert_eq!(value["kills"][0]["eventId"], "k2");
    let (_, value) = request(
        app.clone(),
        "GET",
        "/api/servers/server-a/kills?before=2026-10-01T00:00:00Z&beforeTime=20",
        &key,
        Value::Null,
    )
    .await;
    assert_eq!(value["kills"].as_array().unwrap().len(), 1);
    assert_eq!(value["kills"][0]["eventId"], "k1");
    let (_, value) = request(
        app.clone(),
        "GET",
        "/api/servers/server-a/kills?killer=A_%25&kind=headshot",
        &key,
        Value::Null,
    )
    .await;
    assert_eq!(
        value["kills"].as_array().unwrap().len(),
        1,
        "name wildcards are literal"
    );
    let (status, note) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/players/76561198388453389/notes",
        &key,
        json!({"body":"  测试备注  "}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(note["note"]["body"], "测试备注");
    let note_id = note["note"]["id"].as_i64().unwrap();
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action='player.note'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(audits, 1);
    // A database-side audit failure must roll back the note, too.
    sqlx::raw_sql("CREATE FUNCTION fixture_fail_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture'; END $$; CREATE TRIGGER fixture_audit_error BEFORE INSERT ON audit_log FOR EACH ROW EXECUTE FUNCTION fixture_fail_audit()").execute(&db).await.unwrap();
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/players/76561198388453389/notes",
        &key,
        json!({"body":"must roll back"}),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let notes: i64 = sqlx::query_scalar("SELECT count(*) FROM player_notes")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(notes, 1);
    sqlx::query("DROP TRIGGER fixture_audit_error ON audit_log")
        .execute(&db)
        .await
        .unwrap();
    let (status, _) = request(
        app.clone(),
        "DELETE",
        &format!("/api/servers/server-a/players/76561198388453389/notes/{note_id}"),
        &key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE api_keys SET expires_at=now()-interval '1 second' WHERE id='test-key'")
        .execute(&db)
        .await
        .unwrap();
    let (status, _) = request(
        app.clone(),
        "GET",
        "/api/servers/server-a/kills",
        &key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Cookie mutations retain the original origin check and enrolment grace policy.
    sqlx::raw_sql("INSERT INTO \"user\"(id,name,email,username,role,auth_grace_started_at) VALUES('owner','Owner','owner@fixture.invalid','owner','owner',now()); INSERT INTO session(id,token,user_id,expires_at) VALUES('session','fixture-token','owner',now()+interval '1 hour'); INSERT INTO site_settings(key,value) VALUES('authEnforce','2'),('authGraceDays','14')").execute(&db).await.unwrap();
    let mut mac = Hmac::<Sha256>::new_from_slice(state.config.auth_secret.as_bytes()).unwrap();
    mac.update(b"fixture-token");
    let cookie = format!(
        "malformed; warcon.session_token=fixture-token.{}",
        STANDARD.encode(mac.finalize().into_bytes())
    );
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("cookie", cookie.parse().unwrap());
    assert!(
        warcon_backend::auth::authenticate(&state, &headers, &axum::http::Method::GET)
            .await
            .is_ok()
    );
    assert!(
        warcon_backend::auth::authenticate(&state, &headers, &axum::http::Method::POST)
            .await
            .is_err()
    );
    headers.insert("origin", state.config.origin.parse().unwrap());
    assert!(
        warcon_backend::auth::authenticate(&state, &headers, &axum::http::Method::POST)
            .await
            .is_ok()
    );
    sqlx::query(
        "UPDATE \"user\" SET auth_grace_started_at=now()-interval '15 days' WHERE id='owner'",
    )
    .execute(&db)
    .await
    .unwrap();
    assert!(
        warcon_backend::auth::authenticate(&state, &headers, &axum::http::Method::GET)
            .await
            .is_err()
    );
    sqlx::query("UPDATE \"user\" SET auth_complete=true WHERE id='owner'")
        .execute(&db)
        .await
        .unwrap();
    let cookie_key = format!("cookie:{cookie}");
    // Settings written through the API keep the original {n} representation and enforce bounds atomically.
    let (status, settings) = request(
        app.clone(),
        "PUT",
        "/api/settings",
        &cookie_key,
        json!({"values":{"sampleMs":15000},"reset":[]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["changed"]["sampleMs"].as_f64(), Some(15000.));
    let stored: Value = sqlx::query_scalar("SELECT value FROM site_settings WHERE key='sampleMs'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(stored["n"].as_f64(), Some(15000.));
    let (status, _) = request(
        app.clone(),
        "PUT",
        "/api/settings",
        &cookie_key,
        json!({"values":{"sampleMs":1},"reset":["sampleMs"]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let stored: Value = sqlx::query_scalar("SELECT value FROM site_settings WHERE key='sampleMs'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(
        stored["n"].as_f64(),
        Some(15000.),
        "invalid settings must not apply resets either"
    );
    sqlx::query("UPDATE site_settings SET value='{\"n\":2}' WHERE key='authEnforce'")
        .execute(&db)
        .await
        .unwrap();
    // Keys can be issued once, cannot cross organisations, and never expose hashes or tokens on GET.
    let (status,issued)=request(app.clone(),"POST","/api/orgs/org-a/keys",&cookie_key,json!({"label":"Bot key","capabilities":["server.view","players.notes"],"serverIds":["server-a"],"expiresDays":1})).await;
    assert_eq!(status, StatusCode::CREATED);
    let issued_token = issued["token"].as_str().unwrap();
    let issued_id = issued["key"]["id"].as_str().unwrap();
    let stored_hash: String = sqlx::query_scalar("SELECT key_hash FROM api_keys WHERE id=$1")
        .bind(issued_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(stored_hash, crypto::hash_token(issued_token));
    let (status, listed) = request(
        app.clone(),
        "GET",
        "/api/orgs/org-a/keys",
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!listed.to_string().contains(issued_token));
    assert!(!listed.to_string().contains(&stored_hash));
    let (status, _) = request(
        app.clone(),
        "GET",
        "/api/orgs/org-b/keys",
        issued_token,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/orgs/org-a/keys",
        &cookie_key,
        json!({"label":"Bad scope","capabilities":["server.view"],"serverIds":["server-b"]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status,_)=request(app.clone(),"POST","/api/orgs/org-a/keys",&cookie_key,json!({"label":"Global list","capabilities":["server.view","lists.ban"],"serverIds":["server-a"]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(
        app.clone(),
        "DELETE",
        &format!("/api/orgs/org-b/keys/{issued_id}"),
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = request(
        app.clone(),
        "DELETE",
        &format!("/api/orgs/org-a/keys/{issued_id}"),
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, result) = request(
        app.clone(),
        "GET",
        "/api/servers/server-a/kills",
        issued_token,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(result["error"]["code"], "api_key_revoked");
    // Automation edits enforce both management and moderation, and cancellation is atomic.
    sqlx::query("UPDATE api_keys SET expires_at=NULL WHERE id='test-key'")
        .execute(&db)
        .await
        .unwrap();
    let limit =
        json!({"enabled":true,"kpm":4,"kd":null,"cash":null,"windowSeconds":180,"minKills":10});
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/numeric-limits",
        &key,
        limit.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/numeric-limits",
        &cookie_key,
        limit.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let saved: Value =
        sqlx::query_scalar("SELECT config FROM numeric_limit_rules WHERE server_id='server-a'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(saved["kpm"].as_f64(), Some(4.));
    assert_eq!(saved["kd"], Value::Null);
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/numeric-limits",
        &cookie_key,
        json!({"enabled":true,"kpm":null,"kd":null,"cash":null,"windowSeconds":180,"minKills":10}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    sqlx::raw_sql("INSERT INTO faction_lock_events(server_id,match_id,steam_id,from_faction,to_faction,state,reason) VALUES('server-a',1,'76561198388453389','A','B','pending','fixture'); INSERT INTO skill_balance_runs(id,server_id,match_id,state,reason,plan) VALUES('balance-fixture','server-a',1,'waiting_death','fixture','{}')").execute(&db).await.unwrap();
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/faction-lock",
        &cookie_key,
        json!({"enabled":false,"graceSeconds":120,"capacities":{"A":30}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let lock_state: String =
        sqlx::query_scalar("SELECT state FROM faction_lock_events WHERE server_id='server-a'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(lock_state, "skipped");
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/skill-balance",
        &cookie_key,
        json!({"enabled":true,"graceSeconds":300,"leadPoints":40}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let balance_state: String =
        sqlx::query_scalar("SELECT state FROM skill_balance_runs WHERE id='balance-fixture'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(balance_state, "cancelled");
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/weapon-restrictions",
        &cookie_key,
        json!({"enabled":true,"causes":[" Id.Item.AK74M ","Id.Item.AK74M"],"groups":["vehicles"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let causes: Value = sqlx::query_scalar(
        "SELECT causes FROM weapon_restriction_rules WHERE server_id='server-a'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(causes, json!(["Id.Item.AK74M"]));
    // Feed tokens stay encrypted, and a key with view permission never sees their plaintext.
    let (status, feed) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/feed",
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let feed_token = feed["token"].as_str().unwrap();
    assert!(feed_token.starts_with("wkf_"));
    let (status, feed) = request(
        app.clone(),
        "GET",
        "/api/servers/server-a/feed",
        &key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(feed["token"], "");
    assert_eq!(feed["configured"], true);
    let encrypted: String =
        sqlx::query_scalar("SELECT feed_token_enc FROM servers WHERE id='server-a'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_ne!(encrypted, feed_token);
    assert_eq!(
        crypto::decrypt_secret(&state.config.encryption_key, &encrypted).unwrap(),
        feed_token
    );
    let (status, _) = request(
        app.clone(),
        "DELETE",
        "/api/servers/server-a/feed",
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Analytics distinguish missing cohorts from churn and keep retained+lost equal to the previous roster.
    sqlx::raw_sql("INSERT INTO servers(id,org_id,name,host,port,password_enc) VALUES('analytics-server','org-a','Analytics','example.org',8080,'unused'); INSERT INTO samples(server_id,ts,ok,player_count,max_players,map,cash) VALUES('analytics-server',date_trunc('hour',now())-interval '2 hours',true,10,40,'Map A','[{\"name\":\"A\",\"cash\":1000}]'),('analytics-server',date_trunc('hour',now())-interval '2 hours'+interval '5 minutes',true,20,40,'Map B','[{\"name\":\"B\",\"cash\":2000}]'); INSERT INTO matches(id,server_id,started_at,ended_at,map) VALUES(9001,'analytics-server',now()-interval '2 hours',now()-interval '1 hour','Map A'),(9002,'analytics-server',now()-interval '1 hour',now()-interval '10 minutes','Map B'),(9003,'analytics-server',now()-interval '10 minutes',NULL,'Map C'); INSERT INTO match_players(match_id,server_id,steam_id,name) VALUES(9001,'analytics-server','76561198000000001','Kept'),(9001,'analytics-server','76561198000000002','Gone'),(9002,'analytics-server','76561198000000001','Kept'); INSERT INTO player_sessions(server_id,steam_id,name,joined_at,last_seen,left_at) VALUES('analytics-server','76561198000000001','Kept',now()-interval '2 hours',now(),NULL),('analytics-server','76561198000000002','Gone',now()-interval '2 hours',now()-interval '1 hour',now()-interval '1 hour'); INSERT INTO steam_game_playtime(steam_id,minutes,state,checked_at) VALUES('76561198000000001',600,'known',now())").execute(&db).await.unwrap();
    let (status, analysis) = request(
        app.clone(),
        "GET",
        "/api/servers/analytics-server/analytics?range=24h",
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{analysis}");
    assert_eq!(analysis["retention"][0]["retained"], 1);
    assert_eq!(analysis["retention"][0]["lost"], 1);
    assert_eq!(analysis["retention"][0]["percent"].as_f64(), Some(50.));
    assert_eq!(
        analysis["retention"][1]["retained"],
        Value::Null,
        "missing roster is unknown"
    );
    assert_eq!(analysis["playtime"]["available"], 1);
    assert_eq!(analysis["playtime"]["unknown"], 1);
    assert_eq!(analysis["sampleSeconds"].as_f64(), Some(15.));
    assert_eq!(analysis["cash"][0]["total"].as_f64(), Some(1000.));
    assert_eq!(analysis["combat"], Value::Null);
    let (status, cash) = request(
        app.clone(),
        "GET",
        "/api/servers/analytics-server/cash?since=2000-01-01T00:00:00Z",
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cash["points"].as_array().unwrap().len(), 2);
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/servers/server-a/weapon-restrictions",
        &cookie_key,
        json!({"enabled":true,"causes":[],"groups":[]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status,role)=request(app.clone(),"POST","/api/orgs/org-a/roles",&cookie_key,json!({"name":"Review team","capabilities":["players.notes","server.view","players.notes"]})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        role["role"]["capabilities"],
        json!(["server.view", "players.notes"])
    );
    let role_id = role["role"]["id"].as_str().unwrap();
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/orgs/org-a/roles",
        &cookie_key,
        json!({"name":"review TEAM"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = request(
        app.clone(),
        "PATCH",
        &format!("/api/orgs/org-b/roles/{role_id}"),
        &cookie_key,
        json!({"name":"foreign"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    sqlx::query("UPDATE api_keys SET expires_at=NULL WHERE id='test-key'")
        .execute(&db)
        .await
        .unwrap();
    let (status, _) = request(
        app.clone(),
        "GET",
        "/api/orgs/org-a/roles",
        &key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO org_members(org_id,user_id,role) VALUES('org-a','owner','owner')")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO server_grants(server_id,user_id,role_id) VALUES('server-a','owner',$1)",
    )
    .bind(role_id)
    .execute(&db)
    .await
    .unwrap();
    let (status, _) = request(
        app.clone(),
        "DELETE",
        &format!("/api/orgs/org-a/roles/{role_id}"),
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    sqlx::query("DELETE FROM server_grants WHERE user_id='owner'")
        .execute(&db)
        .await
        .unwrap();
    let (status, _) = request(
        app.clone(),
        "DELETE",
        &format!("/api/orgs/org-a/roles/{role_id}"),
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("INSERT INTO org_roles(id,org_id,name,capabilities,builtin) VALUES('builtin','org-a','Custom viewer','[\"server.view\",\"players.notes\"]','viewer')").execute(&db).await.unwrap();
    let (status, role) = request(
        app.clone(),
        "POST",
        "/api/orgs/org-a/roles/builtin/reset",
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(role["role"]["name"], "viewer");
    assert_eq!(role["role"]["capabilities"], json!(["server.view"]));
    let (status, _) = request(
        app.clone(),
        "DELETE",
        "/api/orgs/org-a/roles/builtin",
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let first = Leadership::new(db.clone(), "first".into());
    let second = Leadership::new(db.clone(), "second".into());
    assert!(first.renew().await.unwrap());
    assert!(!second.renew().await.unwrap());
    assert!(second.transaction().await.is_err());
    first.transaction().await.unwrap().rollback().await.unwrap();
    first.release().await.unwrap();
    assert!(second.renew().await.unwrap());
    assert!(first.transaction().await.is_err());
    let feed_token = format!("wkf_{}", "A".repeat(43));
    sqlx::query("UPDATE servers SET feed_token_hash=$1 WHERE id='server-a'")
        .bind(crypto::hash_token(&feed_token))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO server_live(server_id,players,players_at,status,status_at) VALUES('server-a',$1,now(),$2,now()) ON CONFLICT(server_id) DO UPDATE SET players=excluded.players,players_at=excluded.players_at,status=excluded.status,status_at=excluded.status_at").bind(json!([{"steamId":"76561198388453389","faction":"Red"},{"steamId":"76561198000000001","faction":"Blue"}])).bind(json!({"map":"NorthAmerica","scores":[{"name":"Red"},{"name":"Blue"}]})).execute(&db).await.unwrap();
    let event = json!({"type":"killed","eventId":"feed-1","matchId":"round","eventTime":30,"mapName":"Zestafona","killerSteamId":"76561198388453389","victimSteamId":"76561198000000001","contextTags":["Meta.PlayerKillFlag.Player.Headshot"],"distance":15000,"weaponDetails":{"custom":"preserved"}});
    let body = json!({"serverId":"game-boot","events":[event.clone(),event.clone(),{"type":"KO","eventId":"raw-ko","damage":88}]});
    let (status, receipt) = request(
        app.clone(),
        "POST",
        "/api/ingest/events",
        &feed_token,
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["accepted"], 1);
    assert_eq!(receipt["duplicates"], 1);
    assert_eq!(receipt["skipped"], 1);
    let raw: Value =
        sqlx::query_scalar("SELECT payload FROM training_feed_batches ORDER BY id DESC LIMIT 1")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(raw, body);
    let attribution:(Option<String>,Option<String>,bool,f32)=sqlx::query_as("SELECT killer_faction,victim_faction,headshot,distance_m FROM kills WHERE event_id='feed-1'").fetch_one(&db).await.unwrap();
    assert_eq!(
        attribution,
        (Some("Red".into()), Some("Blue".into()), true, 150.)
    );
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM feed_processing_jobs")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(jobs, 2);
    let (_, receipt) = request(app.clone(), "POST", "/api/ingest/events", &feed_token, body).await;
    assert_eq!(receipt["accepted"], 0);
    assert_eq!(receipt["duplicates"], 2);
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM feed_processing_jobs")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(jobs, 2, "retries must not duplicate consumer jobs");
    sqlx::raw_sql("CREATE TRIGGER fixture_job_error BEFORE INSERT ON feed_processing_jobs FOR EACH ROW EXECUTE FUNCTION fixture_fail_audit()").execute(&db).await.unwrap();
    let mut event = event;
    event["eventId"] = json!("feed-2");
    event["distance"] = json!(1e100);
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/ingest/events",
        &feed_token,
        json!({"events":[event.clone()]}),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let kills: i64 = sqlx::query_scalar("SELECT count(*) FROM kills WHERE event_id='feed-2'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(kills, 0);
    sqlx::query("DROP TRIGGER fixture_job_error ON feed_processing_jobs")
        .execute(&db)
        .await
        .unwrap();
    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/ingest/events",
        &feed_token,
        json!({"events":[event]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let invalid: (bool, Option<f32>, Option<f32>) = sqlx::query_as(
        "SELECT distance_invalid,distance_m,raw_distance_cm FROM kills WHERE event_id='feed-2'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(invalid, (true, None, None));
    sqlx::query(
        "UPDATE server_live SET players_at=now()+interval '1 second' WHERE server_id='server-a'",
    )
    .execute(&db)
    .await
    .unwrap();
    let legacy = warcon_backend::feed_jobs::claim(
        &second,
        warcon_backend::feed_jobs::Consumer::Legacy,
        None,
    )
    .await
    .unwrap()
    .unwrap();
    let integrity = warcon_backend::feed_jobs::claim(
        &second,
        warcon_backend::feed_jobs::Consumer::Integrity,
        None,
    )
    .await
    .unwrap()
    .unwrap();
    assert_ne!(legacy.id, integrity.id);
    assert!(
        warcon_backend::feed_jobs::claim(
            &second,
            warcon_backend::feed_jobs::Consumer::Legacy,
            None
        )
        .await
        .unwrap()
        .is_none(),
        "jobs stay ordered per server per consumer"
    );
    warcon_backend::feed_jobs::finish(&second, &legacy, None)
        .await
        .unwrap();
    assert!(
        warcon_backend::feed_jobs::claim(
            &second,
            warcon_backend::feed_jobs::Consumer::Legacy,
            None
        )
        .await
        .unwrap()
        .is_some()
    );
    sqlx::query("UPDATE feed_processing_jobs SET attempts=attempts+1 WHERE id=$1")
        .bind(integrity.id)
        .execute(&db)
        .await
        .unwrap();
    assert!(
        warcon_backend::feed_jobs::finish(&second, &integrity, None)
            .await
            .is_err(),
        "an obsolete attempt cannot acknowledge a reclaimed job"
    );
    sqlx::raw_sql("INSERT INTO integrity_rules(org_id,config,assessment_mode) VALUES('org-a','{}','legacy'); INSERT INTO integrity_scores(org_id,server_id,steam_id,scored_at,rule_version,score,level,breakdown,current_behavior_anomaly,source,report_id) VALUES('org-a','server-a','76561198388453389',now(),1,6,'WATCH','[]',false,'report',1); INSERT INTO integrity_profile_refresh_jobs(steam_id) VALUES('76561198388453389')").execute(&db).await.unwrap();
    let fixture=axum::Router::new().route("/ISteamUser/GetPlayerSummaries/v2/",axum::routing::get(||async {axum::Json(json!({"response":{"players":[{"steamid":"76561198388453389","personaname":"Queue player","communityvisibilitystate":3}]}}))})).route("/ISteamUser/GetPlayerBans/v1/",axum::routing::get(||async {axum::Json(json!({"players":[{"SteamId":"76561198388453389","NumberOfVACBans":1,"NumberOfGameBans":0,"DaysSinceLastBan":15}]}))})).route("/IPlayerService/GetOwnedGames/v1/",axum::routing::get(|axum::extract::RawQuery(query):axum::extract::RawQuery|async move {
        let pairs=url::form_urlencoded::parse(query.as_deref().unwrap_or("").as_bytes()).collect::<std::collections::HashMap<_,_>>();
        let input:Value=serde_json::from_str(pairs["input_json"].as_ref()).unwrap();assert_eq!(input["appids_filter"],json!([1867240]));
        axum::Json(if input["steamid"]=="76561198000000001" {json!({"response":{"games":[{"appid":1867240,"playtime_forever":1234}]}})}else{json!({"response":{}})})
    }));
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(socket, fixture).await.unwrap() });
    second.renew().await.unwrap();
    let steam = warcon_backend::steam::SteamClient::loopback_fixture(
        "fixture-secret".into(),
        format!("http://127.0.0.1:{port}/").parse().unwrap(),
    )
    .unwrap();
    assert!(
        warcon_backend::steam::process_next(&second, &steam)
            .await
            .unwrap()
    );
    assert!(
        !warcon_backend::steam::process_next(&second, &steam)
            .await
            .unwrap()
    );
    let profile: (String, i32) = sqlx::query_as(
        "SELECT persona,vac_bans FROM steam_profiles WHERE steam_id='76561198388453389'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(profile, ("Queue player".into(), 1));
    let job_state: String = sqlx::query_scalar(
        "SELECT state FROM integrity_profile_refresh_jobs WHERE steam_id='76561198388453389'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(job_state, "done");
    second.renew().await.unwrap();
    assert_eq!(
        warcon_backend::playtime::refresh(&second, &steam)
            .await
            .unwrap(),
        2
    );
    let known: (Option<i32>, String) = sqlx::query_as(
        "SELECT minutes,state FROM steam_game_playtime WHERE steam_id='76561198000000001'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(known, (Some(1234), "known".into()));
    let private: (Option<i32>, String) = sqlx::query_as(
        "SELECT minutes,state FROM steam_game_playtime WHERE steam_id='76561198000000002'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(private, (None, "unavailable".into()));
    assert_eq!(
        warcon_backend::playtime::refresh(&second, &steam)
            .await
            .unwrap(),
        0,
        "completed playtime work is not immediately re-requested"
    );
    server.abort();
    second.renew().await.unwrap();
    warcon_backend::rollups::rollup(&second).await.unwrap();
    warcon_backend::rollups::rollup(&second).await.unwrap();
    let coverage: f64 = sqlx::query_scalar(
        "SELECT sum(up_s)::float8 FROM sample_rollups WHERE server_id='analytics-server'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(coverage, 900., "repeated rollups must not double count");
    let (status, rolled) = request(
        app.clone(),
        "GET",
        "/api/servers/analytics-server/analytics?range=30d",
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rolled}");
    assert_eq!(rolled["summary"]["avgPlayers"].as_f64(), Some(16.7));
    let finished_id:i64=sqlx::query_scalar("INSERT INTO matches(server_id,started_at,ended_at,map,final_scores,winner) VALUES('server-a','2025-01-01T00:00:00Z','2025-01-01T00:30:00Z','DetailMap','[{\"name\":\"Red\",\"score\":\"100\"},{\"name\":\"Blue\",\"score\":80}]','Red') RETURNING id").fetch_one(&db).await.unwrap();
    sqlx::query("INSERT INTO match_players(match_id,server_id,steam_id,name,faction,seconds,kills,deaths,cash_delta,longest_m,kill_streak) VALUES($1,'server-a','76561198000000001','MVP','Red',1800,17,8,1234567,120.5,3),($1,'server-a','76561198000000002','Observer','White',1800,0,0,0,NULL,0)").bind(finished_id).execute(&db).await.unwrap();
    sqlx::raw_sql("INSERT INTO samples(ts,server_id,ok,player_count,scores) VALUES('2025-01-01T00:00:15Z','server-a',true,2,'[{\"name\":\"Red\",\"score\":1},{\"name\":\"Blue\",\"score\":2}]'),('2025-01-01T00:30:00Z','server-a',true,2,'[{\"name\":\"OtherMap\",\"score\":999}]')").execute(&db).await.unwrap();
    let (status, detail) = request(
        app.clone(),
        "GET",
        &format!("/api/servers/server-a/matches/{finished_id}"),
        &cookie_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["match"]["players"], 2);
    assert_eq!(detail["lines"][0]["result"], "win");
    assert!(detail["lines"][1]["result"].is_null());
    assert_eq!(detail["timeline"], json!([[15.0, 1.0, 2.0, 0]]));
    assert_eq!(detail["awards"][1]["value"], "2.13");
    assert_eq!(detail["awards"][4]["value"], "+$1,234,567");
    assert_eq!(detail["factions"].as_array().unwrap().len(), 3);
    assert_eq!(
        request(
            app.clone(),
            "GET",
            &format!("/api/servers/server-b/matches/{finished_id}"),
            &cookie_key,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/servers/server-a/matches/1.0",
            &cookie_key,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Corrupt history is rejected rather than silently rewriting the database.
    sqlx::query("UPDATE drizzle.__drizzle_migrations SET hash='invalid' WHERE id=(SELECT min(id) FROM drizzle.__drizzle_migrations)").execute(&db).await.unwrap();
    assert!(migrations::migrate(&db, &folder).await.is_err());
    db.close().await;
    sqlx::query(&format!("DROP DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
