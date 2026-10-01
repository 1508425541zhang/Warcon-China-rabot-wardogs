use serde_json::Value;
use warcon_backend::{
    automation_policy::{LimitObservation, NumericLimits, numeric_breaches},
    playtime,
};
fn close(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-8,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b)
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, v) in a {
                close(v, &b[k])
            }
        }
        _ => assert_eq!(a, b),
    }
}
#[test]
fn numeric_limits_and_playtime_match_existing_typescript() {
    let fixture: Value = serde_json::from_str(include_str!("../fixtures/analytics.json")).unwrap();
    for case in fixture["limits"].as_array().unwrap() {
        assert_eq!(
            NumericLimits::parse(&case["input"]).is_some(),
            case["valid"].as_bool().unwrap()
        );
    }
    for case in fixture["breaches"].as_array().unwrap() {
        let rule = NumericLimits::parse(&case["rule"]).unwrap();
        let points: Vec<LimitObservation> = serde_json::from_value(case["points"].clone()).unwrap();
        close(
            &serde_json::to_value(numeric_breaches(&rule, &points)).unwrap(),
            &case["expected"],
        );
    }
    for case in fixture["playtime"].as_array().unwrap() {
        let groups = case["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| playtime::Group {
                minutes: g["minutes"].as_i64(),
                state: g["state"].as_str().unwrap().into(),
                count: g["count"].as_i64().unwrap(),
            })
            .collect::<Vec<_>>();
        close(&playtime::summarize(&groups, true), &case["expected"]);
    }
}
