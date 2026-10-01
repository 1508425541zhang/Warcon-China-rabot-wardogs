mod common;
use common::Db;
use serde_json::{Value, json};
use warcon_backend::leaderboards;
#[test]
fn original_board_query_and_grouping() {
    let v: Value = serde_json::from_str(include_str!("../fixtures/leaderboards.json")).unwrap();
    for r in v["queries"].as_array().unwrap() {
        assert_eq!(
            json!(leaderboards::parse(
                r["raw"].as_str(),
                r["max"].as_i64().unwrap()
            )),
            r["result"]
        )
    }
    let actual = leaderboards::group(v["group"]["lines"].as_array().unwrap(), "key");
    let expected = &v["group"]["result"];
    for (a, b) in actual
        .as_array()
        .unwrap()
        .iter()
        .zip(expected.as_array().unwrap())
    {
        for (k, v) in b.as_object().unwrap() {
            if v.is_number() {
                assert_eq!(a[k].as_f64(), v.as_f64())
            } else {
                assert_eq!(a[k], *v)
            }
        }
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL TEST_DATABASE_URL"]
async fn public_feature_gates_private_field_redaction_board_and_career() {
    let db = Db::new().await;
    sqlx::query("INSERT INTO organizations(id,name,slug,allow_public_status,allow_public_leaderboards) VALUES('org','Org','org',true,true)").execute(&db.state.db).await.unwrap();
    sqlx::query("INSERT INTO servers(id,org_id,name,host,port,password_enc,public_status,public_leaderboards) VALUES('server','org','Server','hidden-host.invalid',123,'private-secret',true,false),('hidden','org','Hidden','hidden-host.invalid',123,'private-secret',false,false)").execute(&db.state.db).await.unwrap();
    let status = json!({"map":"MapA","experiences":[],"lighting":"Day","playerCount":1,"maxPlayers":100,"scores":[{"name":"Red","colorHex":"ff0000","score":60}],"scoreCap":100,"matchSeconds":60});
    let players = json!([{"steamId":"76561198000000001","name":"Player","faction":"Red","kills":3,"deaths":1,"cash":9999,"ping":123,"ip":"sensitive"}]);
    sqlx::query("INSERT INTO server_live(server_id,ok,status,players,players_at,status_at,observed_at,build,error) VALUES('server',true,$1,$2,now(),now(),now(),'private-build','private-error')").bind(&status).bind(players).execute(&db.state.db).await.unwrap();
    let (s, v) = db
        .call("GET", "/api/public/servers/server", json!({}), "")
        .await;
    assert_eq!(s, 200, "{v}");
    assert!(v["server"]["roster"][0]["steamId"].is_null());
    for word in [
        "hidden-host",
        "private-secret",
        "private-build",
        "private-error",
        "9999",
        "sensitive",
    ] {
        assert!(!v.to_string().contains(word), "{v}")
    }
    assert_eq!(
        db.call("GET", "/api/public/servers/hidden", json!({}), "")
            .await
            .0,
        404
    );
    assert_eq!(
        db.call(
            "GET",
            "/api/public/servers/server/leaderboard",
            json!({}),
            ""
        )
        .await
        .0,
        404
    );
    sqlx::query("UPDATE servers SET public_leaderboards=true WHERE id='server'")
        .execute(&db.state.db)
        .await
        .unwrap();
    let (s, v) = db
        .call(
            "GET",
            "/api/public/servers/server/leaderboard",
            json!({}),
            "",
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert!(v["rows"].as_array().unwrap().is_empty());
    sqlx::query("INSERT INTO player_sessions(server_id,steam_id,name,joined_at,last_seen,left_at,kills,deaths,cash) VALUES('server','76561198000000001','Player',now()-interval '3 hours',now()-interval '1 hour',now()-interval '1 hour',9,3,1000),('hidden','76561198000000001','HiddenPlayer',now()-interval '10 hours',now()-interval '1 hour',now()-interval '1 hour',9000,3,1000000)").execute(&db.state.db).await.unwrap();
    let mut last = 0i64;
    for (map, winner, kills, deaths, minutes) in [
        ("MapA", Some("Red"), 5, 1, 3),
        ("MapA", None, 2, 2, 2),
        ("MapB", Some("Red"), 2, 0, 1),
    ] {
        let id:i64=sqlx::query_scalar("INSERT INTO matches(server_id,started_at,ended_at,map,winner,final_scores) VALUES('server',now()-($1*interval '1 hour'),now()-($1*interval '1 hour')+interval '30 minutes',$2,$3,'[]') RETURNING id").bind(minutes as f64).bind(map).bind(winner).fetch_one(&db.state.db).await.unwrap();
        sqlx::query("INSERT INTO match_players(server_id,match_id,steam_id,name,faction,seconds,kills,deaths,headshots,kill_streak) VALUES('server',$1,'76561198000000001','Player','Red',1800,$2,$3,1,2)").bind(id).bind(kills).bind(deaths).execute(&db.state.db).await.unwrap();
        last = id;
    }
    let (s, v) = db
        .call(
            "GET",
            "/api/public/servers/server/leaderboard?range=all&scope=org",
            json!({}),
            "",
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 1);
    assert_eq!(v["rows"][0]["kills"], 9);
    assert_eq!(v["rows"][0]["minutes"].as_f64(), Some(120.));
    assert_eq!(v["rows"][0]["name"], "Player");
    assert_eq!(v["maxPage"], 20);
    let (s, v) = db
        .call(
            "GET",
            "/api/public/servers/server/players/76561198000000001",
            json!({}),
            "",
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["career"]["rank"]["server"], 1);
    assert_eq!(v["career"]["streak"], json!({"kind":"win","n":2}));
    assert_eq!(v["career"]["matches"], 3);
    assert_eq!(v["career"]["minutes"].as_f64(), Some(90.));
    assert_eq!(v["career"]["maps"][0]["key"], "MapA");
    assert_eq!(v["career"]["last"][0]["matchId"], last);
    let (s, v) = db
        .call(
            "GET",
            &format!("/api/public/servers/server/matches/{last}"),
            json!({}),
            "",
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert!(v["feed"].as_array().unwrap().is_empty());
    assert_eq!(v["more"], false);
    sqlx::query("UPDATE organizations SET allow_public_leaderboards=false WHERE id='org'")
        .execute(&db.state.db)
        .await
        .unwrap();
    assert_eq!(
        db.call(
            "GET",
            "/api/public/servers/server/players/76561198000000001",
            json!({}),
            ""
        )
        .await
        .0,
        404
    );
    sqlx::query("UPDATE organizations SET suspended_at=now() WHERE id='org'")
        .execute(&db.state.db)
        .await
        .unwrap();
    assert_eq!(
        db.call("GET", "/api/public/servers/server", json!({}), "")
            .await
            .0,
        404
    );
    db.close().await;
}
