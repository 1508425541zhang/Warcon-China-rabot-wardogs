mod common;
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::path::Path;

fn sources(root: &Path, result: &mut Vec<String>) {
    for e in std::fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, result);
        } else if matches!(
            p.file_name().unwrap().to_str().unwrap(),
            "+page.server.ts" | "+layout.server.ts"
        ) {
            let project = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap();
            result.push(
                p.canonicalize()
                    .unwrap()
                    .strip_prefix(project)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/"),
            );
        }
    }
}
async fn load(db: &common::Db, source: &str, search: &str, cookie: &str) -> (StatusCode, Value) {
    db.call("POST", "/api/page/load", json!({"source": source,"search":search,"params":{
        "id":if source.contains("/orgs/[id]/") {"org"} else {"server"},
        "steamId":"76561198000000001","matchId":"1","token":"missing-invite","pluginId":"round-summary"
    }}), cookie).await
}

#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn all_page_queries_deep_link_access_and_archive_contract() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("error")
        .try_init();
    let mut db = common::Db::new().await;
    db.state.config.identity.allow_signup = true;
    db.app = warcon_backend::api::router(db.state.clone());
    let owner = db.user("owner", true).await;
    let viewer = db.user("viewer", false).await;
    let outsider = db.user("outsider", false).await;
    sqlx::raw_sql(r##"
        INSERT INTO organizations(id,name,slug,allow_public_status,allow_public_leaderboards) VALUES('org','Fixture Org','org',true,true);
        INSERT INTO org_members(org_id,user_id,role)VALUES('org','owner','owner'),('org','viewer','member');
        INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc,public_status,public_leaderboards)VALUES('server','org','Fixture Server','example.invalid',1,'http','fixture',true,true);
        INSERT INTO lists(id,org_id,kind)VALUES('ban','org','ban'),('reserve','org','reserve');
        INSERT INTO org_roles(id,org_id,name,capabilities)VALUES('view','org','Viewer','["server.view"]');
        INSERT INTO server_grants(server_id,user_id,role_id)VALUES('server','viewer','view');
        INSERT INTO matches(id,server_id,map,started_at,ended_at)VALUES(1,'server','Europe',now()-interval '30 minutes',now()-interval '1 minute');
        INSERT INTO match_players(match_id,server_id,steam_id,name,faction,seconds,kills,deaths) VALUES(1,'server','76561198000000001','Player','Blue',1700,12,3);
        INSERT INTO player_sessions(server_id,steam_id,name,faction,joined_at,last_seen,kills,deaths,cash)VALUES('server','76561198000000001','Player','Blue',now()-interval '20 minutes',now(),12,3,300);
        INSERT INTO server_live(server_id,updated_at,ok,players,status,players_at,status_at)VALUES('server',now(),true,'[{"steamId":"76561198000000001","name":"Player","faction":"Blue"}]','{"clock":400,"map":"Europe"}',now(),now());
        INSERT INTO integrity_rules(org_id,assessment_mode,config)VALUES('org','statistical_shadow','{}');
        INSERT INTO integrity_cases(id,org_id,server_id,steam_id,created_at,status,confidence,trigger,rule_version,risk_score,risk_breakdown,snapshot,statistical)VALUES
          ('pending','org','server','76561198000000001',now()-interval '1 second','OPEN','B','STATISTICAL_WINDOW',1,6,'[]','{}','{"level":"WATCH"}'),
          ('archived','org','server','76561198000000001',now()-interval '2 seconds','REVIEWED','B','STATISTICAL_WINDOW',1,6,'[]','{}','{"level":"WATCH"}');
        INSERT INTO personal_plugins(user_id,plugin_id,manifest,server_id)VALUES('owner','round-summary','{"apiVersion":1,"id":"round-summary","name":"Summary","version":"1.0.0","renderer":"round-summary","widgets":[],"style":{"accent":"#69d6e3","columns":2,"density":"comfortable"}}','server');
    "##).execute(&db.state.db).await.unwrap();
    for i in 0..12 {
        sqlx::query("INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,killer_name,killer_faction,victim_steam_id,victim_name,victim_faction,cause,distance_m,headshot,tags,faction_bracketed,faction_observed_at)VALUES(now()-interval '10 seconds','server',$1,'boot','round',$2,'Europe','76561198000000001','Player','Blue',$3,'Victim','Red','Id.Item.AK74M',$4,true,'[]',true,now()-interval '10 seconds')").bind(format!("event-{i}")).bind(280.+i as f32*10.).bind(format!("765611980000000{:02}",i+2)).bind(120.+i as f32).execute(&db.state.db).await.unwrap();
    }
    let mut routes = vec![];
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/routes"),
        &mut routes,
    );
    routes.sort();
    assert_eq!(routes.len(), 51);
    let fields: Value =
        serde_json::from_str(include_str!("../assets/page-dto-fields.json")).unwrap();
    let mut failures = vec![];
    for source in &routes {
        let (status, v) = load(&db, source, "", &owner).await;
        if status != StatusCode::OK {
            failures.push(format!("{source}: {status} {v}"));
        } else {
            assert!(
                v.get("data").is_some() || v.get("redirect").is_some(),
                "{source}: {v}"
            );
            if let Some(data) = v.get("data") {
                for key in fields[source].as_array().unwrap() {
                    if data.get(key.as_str().unwrap()).is_none() {
                        failures.push(format!("{source}: missing DTO field {key}"));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for source in [
        "src/routes/(app)/server/[id]/integrity/+page.server.ts",
        "src/routes/(app)/server/[id]/automation/+page.server.ts",
        "src/routes/(app)/orgs/[id]/integrity/+page.server.ts",
        "src/routes/(app)/admin/+page.server.ts",
    ] {
        let (status, _) = load(&db, source, "", &viewer).await;
        assert!(
            [StatusCode::FORBIDDEN, StatusCode::NOT_FOUND].contains(&status),
            "{source}: {status}"
        );
    }
    let (status, _) = load(
        &db,
        "src/routes/(app)/server/[id]/players/[steamId]/+page.server.ts",
        "",
        &outsider,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, v) = load(
        &db,
        "src/routes/(app)/server/[id]/players/[steamId]/+page.server.ts",
        "",
        &viewer,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(
        v["data"]["integrity"].is_null(),
        "viewer must not receive Integrity private evidence: {v}"
    );
    let (_, v) = load(
        &db,
        "src/routes/(app)/server/[id]/integrity/cases/+page.server.ts",
        "view=archived",
        &owner,
    )
    .await;
    assert_eq!(v["data"]["view"], "archived");
    assert_eq!(v["data"]["cases"][0]["id"], "archived");
    let (_, v) = load(&db, "src/routes/(public)/s/[id]/+page.server.ts", "", "").await;
    assert_eq!(v["data"]["view"]["name"], "Fixture Server", "{v}");
    let (status, _) = db
        .call(
            "POST",
            "/api/page/load",
            json!({"source":"../../settings","params":{}}),
            &owner,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = db
        .call(
            "POST",
            "/api/page/load",
            json!({"source":routes[0]}),
            "Bearer forged",
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    db.close().await;
}
