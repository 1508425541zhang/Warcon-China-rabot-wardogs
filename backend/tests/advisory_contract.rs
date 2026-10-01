mod common;
use serde_json::{Value, json};
use warcon_backend::{group_control as g, player_risk as r, trigger_policy};
fn normalized(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(normalized).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), normalized(v))).collect())
        }
        _ => v.clone(),
    }
}
#[test]
fn original_advisory_policy_parity() {
    let f: Value = serde_json::from_str(include_str!("../fixtures/advisory.json")).unwrap();
    for c in f["risk"].as_array().unwrap() {
        assert_eq!(
            normalized(&r::assess(&c["input"], 1790769600000)),
            normalized(&c["output"]),
            "{c}"
        );
    }
    for c in f["prefixes"].as_array().unwrap() {
        assert_eq!(g::prefix(c["name"].as_str().unwrap(), 4), c["output"]);
        assert_eq!(r::normal_name(c["name"].as_str().unwrap()), c["normal"]);
    }
    for c in f["comparisons"].as_array().unwrap() {
        let a = c["a"].as_str().unwrap();
        let b = c["b"].as_str().unwrap();
        assert!(
            (g::similarity(a, b) - c["similarity"].as_f64().unwrap()).abs() < 1e-10,
            "{c}"
        );
        assert_eq!(r::resembles(a, b), c["resembles"] == true);
    }
    for c in f["groups"].as_array().unwrap() {
        let factions = c["factions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            normalized(&json!(g::detect(
                c["players"].as_array().unwrap(),
                &c["cfg"],
                &factions
            ))),
            normalized(&c["output"])
        );
    }
    for c in f["awardSets"].as_array().unwrap() {
        assert_eq!(
            normalized(&json!(trigger_policy::award_winners(
                c["lines"].as_array().unwrap()
            ))),
            normalized(&c["output"])
        );
    }
    assert!(
        g::advice(
            &json!({"groups":[{"id":"wrong","assessment":"possible_group","reason":"test"}]}),
            &[json!({"id":"right"})]
        )
        .is_err()
    );
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn persisted_scan_scoping_and_mode_guards() {
    let db = common::Db::new().await;
    let owner = db.user("owner", true).await;
    let outsider = db.user("outsider", false).await;
    sqlx::raw_sql("INSERT INTO organizations(id,name,slug)VALUES('org','Org','org');INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc)VALUES('server','org','Server','example.invalid',1234,'http','fixture')").execute(&db.state.db).await.unwrap();
    let cfg = json!({"mode":"manual","engine":"structured","prefixLength":4,"similarityPercent":70,"minPlayers":2});
    let (s, v) = db
        .call(
            "POST",
            "/api/servers/server/group-control",
            json!({"config":cfg}),
            &owner,
        )
        .await;
    assert_eq!(s.as_u16(), 200, "{v}");
    let (s, _) = db
        .call(
            "POST",
            "/api/servers/server/group-control",
            json!({"operation":"scan"}),
            &outsider,
        )
        .await;
    assert_eq!(s.as_u16(), 404);
    let (s, _) = db
        .call(
            "POST",
            "/api/servers/server/group-control",
            json!({"operation":"scan"}),
            &owner,
        )
        .await;
    assert_eq!(s.as_u16(), 409);
    sqlx::query("INSERT INTO server_live(server_id,ok,status,players,status_at,players_at)VALUES('server',true,$1,$2,now(),now())").bind(json!({"scores":[{"name":"Blue"}]})).bind(json!([{"steamId":"76561198000000001","name":"CLAN Alice","faction":"Blue"},{"steamId":"76561198000000002","name":"ＣＬＡＮ Bob","faction":"Blue"}])).execute(&db.state.db).await.unwrap();
    let (s, v) = db
        .call(
            "POST",
            "/api/servers/server/group-control",
            json!({"operation":"scan"}),
            &owner,
        )
        .await;
    assert_eq!(s.as_u16(), 200, "{v}");
    let view = g::view(&db.state, "server").await.unwrap();
    assert_eq!(view["scan"]["groups"].as_array().unwrap().len(), 1);
    assert_eq!(view["scan"]["aiStatus"], "not_requested");
    let (s, _) = db
        .call(
            "POST",
            "/api/servers/server/group-control",
            json!({"operation":"scan"}),
            &owner,
        )
        .await;
    assert_eq!(s.as_u16(), 429);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox")
            .fetch_one(&db.state.db)
            .await
            .unwrap(),
        0
    );
    db.close().await;
}
