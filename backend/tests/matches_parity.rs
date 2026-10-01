use serde_json::Value;
#[test]
fn match_awards_preserve_ties_and_javascript_rounding() {
    let cases: Value = serde_json::from_str(include_str!("../fixtures/matches.json")).unwrap();
    for c in cases.as_array().unwrap() {
        assert_eq!(
            serde_json::json!(warcon_backend::api::matches::awards(
                c["lines"].as_array().unwrap(),
                c["duration"].as_f64().unwrap()
            )),
            c["expected"]
        );
    }
}
