use serde_json::Value;
use warcon_backend::feed;
fn compare(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64(), "{path}"),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                compare(a, b, &format!("{path}/{i}"))
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (key, value) in a {
                compare(value, &b[key], &format!("{path}/{key}"))
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}
#[test]
fn protocol_parser_matches_existing_feed() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("../fixtures/feed.json")).unwrap();
    for (i, case) in cases.iter().enumerate() {
        compare(
            &serde_json::to_value(feed::parse_batch(&case["input"]).unwrap()).unwrap(),
            &case["expected"],
            &format!("case-{i}"),
        );
    }
    assert!(feed::parse_batch(&serde_json::json!({"events":vec![Value::Null;201]})).is_err());
    assert!(feed::parse_batch(&Value::Null).is_err());
}
