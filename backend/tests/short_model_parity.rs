use serde_json::{Value, json};
use warcon_backend::{short_features as features, short_model::Model};
fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../services/short-risk/artifacts-rust")
}
fn fixtures() -> [(Value, Model); 2] {
    [
        (
            serde_json::from_str(include_str!("../fixtures/short-isolation.json")).unwrap(),
            Model::load(&root().join("isolation.json")).unwrap(),
        ),
        (
            serde_json::from_str(include_str!("../fixtures/short-xgboost.json")).unwrap(),
            Model::load(&root().join("xgboost.json")).unwrap(),
        ),
    ]
}
fn comparable(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| k.as_str() != "inference_ms")
                .map(|(k, v)| (k.clone(), comparable(v)))
                .collect(),
        ),
        Value::Array(a) => json!(a.iter().map(comparable).collect::<Vec<_>>()),
        Value::Number(n) => json!(n.as_f64()),
        _ => v.clone(),
    }
}
#[test]
fn original_observer_history_and_policy_agree() {
    use warcon_backend::short_observer as observer;
    let (f, model) = fixtures().into_iter().next().unwrap();
    let mut history = observer::History::new();
    for c in f["observers"].as_array().unwrap() {
        let actual = observer::evaluate(&model, &c["data"], &mut history).unwrap();
        assert_eq!(comparable(&actual), comparable(&c["result"]));
    }
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/short-policy.json")).unwrap();
    assert_eq!(model.threshold(0.996), fixture["warning"].as_f64());
    assert_eq!(model.threshold(0.999), fixture["kick"].as_f64());
    for c in fixture["cases"].as_array().unwrap() {
        let actual =
            observer::consensus(&c["windows"], &c["end"], fixture["kick"].as_f64().unwrap());
        if c["expected"].is_null() {
            assert!(actual.is_none())
        } else {
            assert!((actual.unwrap() - c["expected"].as_f64().unwrap()).abs() < 1e-12)
        }
    }
}
#[test]
fn frozen_python_expectations_match_source_hashes() {
    use sha2::{Digest, Sha256};
    let (f, _) = fixtures().into_iter().next().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    for (path, hash) in f["original_sources"].as_object().unwrap() {
        assert_eq!(
            hex::encode(Sha256::digest(std::fs::read(root.join(path)).unwrap())),
            hash.as_str().unwrap()
        );
    }
}
#[test]
fn original_features_and_real_trained_tree_scores_agree() {
    for (fixture, model) in fixtures() {
        for case in fixture["feature_cases"].as_array().unwrap() {
            let r = &case["request"];
            let events = features::events(&r["events"]).unwrap();
            let vector = features::vector(
                &events,
                r["player_id"].as_str().unwrap(),
                r["decision_game_seconds"].as_f64().unwrap(),
                features::utc(&r["decision_received_utc"]).unwrap(),
                &model.distance_baseline,
            )
            .unwrap();
            for (actual, expected) in vector.iter().zip(case["features"].as_array().unwrap()) {
                if expected.is_null() {
                    assert!(actual.is_nan())
                } else {
                    assert!((*actual as f64 - expected.as_f64().unwrap()).abs() < 1e-6)
                }
            }
            let result = model.predict(r).unwrap();
            if r["events"].as_array().unwrap().is_empty() {
                assert_eq!(result["status"], "insufficient_data")
            } else {
                assert_eq!(result["feature_count"], 24);
                assert!(
                    result["ShortRisk"]
                        .as_f64()
                        .is_some_and(|v| (0. ..=1.).contains(&v))
                );
            }
        }
        for case in fixture["vectors"].as_array().unwrap() {
            let x: [f32; 24] = case["features"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().map(|v| v as f32).unwrap_or(f32::NAN))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            let actual = model.score(&x).unwrap();
            let expected = case["score"].as_f64().unwrap();
            assert!(
                (actual - expected).abs() < 1e-6,
                "{}: {actual} != {expected}",
                model.algorithm
            );
        }
    }
}
#[test]
fn late_events_rounds_duplicates_and_missing_flags() {
    let (f, model) = fixtures().into_iter().next().unwrap();
    let mut r = f["feature_cases"][7]["request"].clone();
    let original = model.predict(&r).unwrap();
    let duplicate = r["events"][0].clone();
    r["events"].as_array_mut().unwrap().push(duplicate);
    assert_eq!(
        model.predict(&r).unwrap()["anomaly_score"],
        original["anomaly_score"]
    );
    let i = r["events"].as_array().unwrap().len() - 1;
    r["events"][i]["headshot"] = json!(!r["events"][i]["headshot"].as_bool().unwrap());
    assert!(features::events(&r["events"]).is_err());
    r["events"].as_array_mut().unwrap().pop();
    r["events"][0]["instance_id"] = json!("different");
    assert!(features::events(&r["events"]).is_err());
    r["events"][0]["instance_id"] = json!("fixture-boot");
    for e in r["events"].as_array_mut().unwrap() {
        e["ts"] = json!(r#"2026-09-02T00:00:00Z"#)
    }
    assert_eq!(model.predict(&r).unwrap()["status"], "insufficient_data");
    let mut events = f["feature_cases"][7]["request"]["events"].clone();
    for e in events.as_array_mut().unwrap() {
        e.as_object_mut().unwrap().remove("tags");
    }
    let events = features::events(&events).unwrap();
    let x = features::vector(
        &events,
        "player",
        130.,
        1788220930.,
        &model.distance_baseline,
    )
    .unwrap();
    assert!(x[3].is_nan() && x[4].is_nan() && x[14].is_nan() && x[15].is_nan());
}
