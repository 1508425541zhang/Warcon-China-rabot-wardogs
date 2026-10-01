use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use warcon_backend::observation_state as s;
fn sorted(mut a: Vec<Value>) -> Value {
    a.sort_by(|a, b| a["steamId"].as_str().cmp(&b["steamId"].as_str()));
    json!(a)
}
fn numeric(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), numeric(v))).collect()),
        Value::Array(a) => json!(a.iter().map(numeric).collect::<Vec<_>>()),
        _ => v.clone(),
    }
}
#[test]
fn original_presence_tallies_boundaries_and_scheduler_agree() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/observation.json")).unwrap();
    for case in fixture["presence"].as_array().unwrap() {
        let sessions: Vec<s::Session> = serde_json::from_value(case["open"].clone()).unwrap();
        let open = sessions
            .into_iter()
            .map(|s| (s.steam_id.clone(), s))
            .collect();
        let players = serde_json::from_value::<Vec<s::Player>>(case["players"].clone()).unwrap();
        let teams = serde_json::from_value::<Vec<String>>(case["teams"].clone()).unwrap();
        let mut value = json!(s::diff(
            &open,
            &players,
            case["now"].as_i64().unwrap(),
            case["grace"].as_i64().unwrap(),
            case["previous"].as_i64().unwrap(),
            Some(&teams)
        ));
        value["left"] = sorted(value["left"].as_array().unwrap().clone());
        assert_eq!(value, case["expected"]);
    }
    for case in fixture["follow"].as_array().unwrap() {
        let mut session: s::Session = serde_json::from_value(case["before"].clone()).unwrap();
        let player = serde_json::from_value::<s::Player>(case["player"].clone()).unwrap();
        let teams = serde_json::from_value::<Vec<String>>(case["teams"].clone()).unwrap();
        s::follow(
            &mut session,
            &player,
            case["now"].as_i64().unwrap(),
            Some(&teams),
        );
        assert_eq!(json!(session), case["expected"]);
    }
    for case in fixture["tallies"].as_array().unwrap() {
        let mut tally = HashMap::new();
        let teams = serde_json::from_value::<Vec<String>>(case["teams"].clone()).unwrap();
        for step in case["steps"].as_array().unwrap() {
            let players =
                serde_json::from_value::<Vec<s::Player>>(step["players"].clone()).unwrap();
            let stayed = serde_json::from_value::<HashSet<String>>(step["stayed"].clone()).unwrap();
            s::tally(
                &mut tally,
                &players,
                step["now"].as_i64().unwrap(),
                step["gap"].as_i64().unwrap(),
                &stayed,
                Some(&teams),
            );
        }
        let (closed, carried) = s::close(&tally, case["cut"].as_i64().unwrap());
        let actual = json!({"tallies":sorted(tally.values().map(|v|json!(v)).collect()),"rows":sorted(tally.values().map(|v|json!(s::row(v))).collect()),"closed":sorted(closed.iter().map(|v|json!(v)).collect()),"carried":sorted(carried.values().map(|v|json!(v)).collect())});
        assert_eq!(actual, case["expected"]);
    }
    for case in fixture["boundaries"].as_array().unwrap() {
        assert_eq!(
            numeric(&json!(s::boundary(
                (!case["previous"].is_null()).then_some(&case["previous"]),
                &case["next"]
            ))),
            numeric(&case["expected"])
        )
    }
    for c in fixture["phases"].as_array().unwrap() {
        assert_eq!(
            s::phase(c["id"].as_str().unwrap(), c["interval"].as_u64().unwrap()),
            c["expected"].as_u64().unwrap()
        )
    }
    for c in fixture["due"].as_array().unwrap() {
        assert_eq!(
            s::next_due(
                c["due"].as_i64().unwrap(),
                c["interval"].as_i64().unwrap(),
                c["now"].as_i64().unwrap()
            ),
            c["expected"].as_i64().unwrap()
        )
    }
    for c in fixture["feed"].as_array().unwrap() {
        assert_eq!(
            numeric(&warcon_backend::match_feed::record(
                c["kills"].as_array().unwrap()
            )),
            numeric(&c["expected"])
        );
    }
}
