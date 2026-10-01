use serde_json::Value;
use warcon_backend::{name_filter as n, trigger_policy as p};
#[test]
fn original_trigger_policies_and_matching() {
    let f: Value = serde_json::from_str(include_str!("../fixtures/triggers.json")).unwrap();
    for c in f["configs"].as_array().unwrap() {
        let out = p::validate(c["kind"].as_str().unwrap(), &c["input"]);
        if c["error"] == true {
            assert!(out.is_err(), "{c}")
        } else {
            assert_eq!(out.unwrap(), c["output"], "{c}")
        }
    }
    for c in f["folding"].as_array().unwrap() {
        assert_eq!(
            n::fold(c["name"].as_str().unwrap(), c["leet"] == true),
            c["output"].as_str().unwrap(),
            "{c}"
        )
    }
    for c in f["nameTests"].as_array().unwrap() {
        assert_eq!(
            n::verdict(&c["cfg"], c["name"].as_str().unwrap()).unwrap_or(Value::Null),
            c["output"],
            "{c}"
        )
    }
    for c in f["ping"].as_array().unwrap() {
        assert_eq!(
            p::ping_step(
                &c["cfg"],
                &c["prev"],
                c["players"].as_array().unwrap(),
                c["now"].as_i64().unwrap(),
                c["gap"].as_i64().unwrap()
            ),
            c["output"]
        )
    }
    let mut t = p::Track::default();
    for c in f["rates"]["steps"].as_array().unwrap() {
        assert_eq!(
            p::rate_step(
                &f["rates"]["cfg"],
                &mut t,
                c["at"].as_i64().unwrap(),
                c["head"] == true
            )
            .map(Value::from)
            .unwrap_or(Value::Null),
            c["output"]
        )
    }
    for c in f["restarted"].as_array().unwrap() {
        assert_eq!(
            p::restart_stage(
                &c["cfg"],
                &c["prev"],
                c["startedAt"].as_i64().unwrap(),
                c["playerCount"].as_i64().unwrap(),
                c["now"].as_i64().unwrap()
            )
            .unwrap_or(Value::Null),
            c["output"]
        )
    }
    assert_eq!(
        Value::from(p::award_winners(f["awards"]["lines"].as_array().unwrap())),
        f["awards"]["output"]
    );
    let m = &f["match"];
    assert_eq!(
        Value::from(p::match_messages(
            &m["cfg"],
            &m["end"],
            2,
            &m["vars"],
            m["lines"].as_array().unwrap()
        )),
        m["output"]
    );
    for c in f["templates"].as_array().unwrap() {
        assert_eq!(
            p::render(c["input"].as_str().unwrap(), &c["vars"]),
            c["output"].as_str().unwrap()
        )
    }
}
