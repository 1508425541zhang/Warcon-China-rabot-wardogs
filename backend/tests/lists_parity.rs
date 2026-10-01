use serde_json::{Value, json};
use warcon_backend::{ban_message, list_plan};
#[test]
fn original_list_plan_errors_and_ban_message_parity() {
    let fixture: Value = serde_json::from_str(include_str!("../fixtures/lists.json")).unwrap();
    for row in fixture["plans"].as_array().unwrap() {
        let i = &row["input"];
        let result = list_plan::plan(
            serde_json::from_value(i["now"].clone()).unwrap(),
            i["retryAfterMs"].as_i64().unwrap(),
            &serde_json::from_value::<Vec<list_plan::Desired>>(i["desired"]["reserved"].clone())
                .unwrap(),
            &serde_json::from_value::<Vec<String>>(i["observed"]["reserved"].clone()).unwrap(),
            &serde_json::from_value::<Vec<list_plan::Stored>>(i["state"].clone()).unwrap(),
        );
        assert_eq!(json!(result), row["result"], "{i}");
    }
    assert_eq!(
        list_plan::desired(fixture["desired"]["rows"].as_array().unwrap()),
        fixture["desired"]["result"]
    );
    for row in fixture["failures"].as_array().unwrap() {
        let i = &row["input"];
        let s = i["status"].as_u64().unwrap() as u16;
        let c = i["code"].as_str().unwrap();
        assert_eq!(
            list_plan::already(s, c, i["message"].as_str().unwrap()),
            row["already"]
        );
        assert_eq!(list_plan::gone(s, c), row["gone"]);
        assert_eq!(list_plan::unreachable(s, c), row["unreachable"]);
    }
    for row in fixture["messages"].as_array().unwrap() {
        let f = serde_json::from_value(row["facts"].clone()).unwrap();
        assert_eq!(
            ban_message::render(row["template"].as_str().unwrap(), &f),
            row["result"]
        );
    }
}
