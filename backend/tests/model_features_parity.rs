use serde_json::Value;
use warcon_backend::{model::Contract, model_features};
#[test]
fn expanded_features_and_missing_masks_match_existing_bun() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/model-features.json")).unwrap();
    let contract: Contract = serde_json::from_value(fixture["contract"].clone()).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let rows = model_features::build(&case["source"], &contract);
        let expected = case["rows"].as_array().unwrap();
        assert_eq!(rows.len(), expected.len(), "{}", case["name"]);
        for (t, row) in rows.iter().enumerate() {
            for (i, v) in row.iter().enumerate() {
                let e = &expected[t][i];
                if e.is_null() {
                    assert!(
                        v.is_nan(),
                        "{} at {t}/{} must be missing",
                        case["name"],
                        contract.features[i]
                    );
                } else {
                    let e = e.as_f64().unwrap() as f32;
                    assert!(
                        (*v - e).abs() <= 1e-5 * e.abs().max(1.),
                        "{} at {t}/{}: {v} != {e}",
                        case["name"],
                        contract.features[i]
                    );
                }
            }
        }
        let input = model_features::normalize(&rows, &contract);
        for (i, v) in input.iter().enumerate() {
            let e = case["input"][i].as_f64().unwrap() as f32;
            assert!(
                (*v - e).abs() <= 1e-5 * e.abs().max(1.),
                "{} normalized {i}: {v} != {e}",
                case["name"]
            );
        }
    }
}
