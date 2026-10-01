use serde_json::{Value, json};
fn numeric(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), numeric(v))).collect()),
        Value::Array(a) => json!(a.iter().map(numeric).collect::<Vec<_>>()),
        _ => v.clone(),
    }
}
#[test]
fn persisted_status_uses_original_normalization_contract() {
    let rows: Vec<Value> = serde_json::from_str(include_str!("../fixtures/live.json")).unwrap();
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(
            numeric(&warcon_backend::live::status(&row["input"])),
            numeric(&row["expected"]),
            "fixture {i}"
        )
    }
    for n in ["", " 1", "1 ", "1e3", ".5", "1.", "NaN", "--1", "0x10"] {
        assert!(warcon_backend::live::finite(&json!(n)).is_none())
    }
}
