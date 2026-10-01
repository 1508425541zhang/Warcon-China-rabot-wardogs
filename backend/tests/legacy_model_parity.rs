use serde_json::Value;
use warcon_backend::{legacy_features, legacy_model::Predictor};
fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/legacy-model.json")).unwrap()
}
fn close(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            assert!((a - b).abs() <= 1e-6 * (1. + b.abs()), "{a} != {b}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b)
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, a) in a {
                close(a, &b[k])
            }
        }
        _ => assert_eq!(a, b),
    }
}
#[test]
fn original_four_table_features_and_missing_masks() {
    for case in fixture()["cases"].as_array().unwrap() {
        let rows = legacy_features::build(&case["request"]);
        if case["error"] == true {
            assert!(rows.is_err(), "{}", case["name"])
        } else {
            close(&serde_json::json!(rows.unwrap()), &case["rows"]);
        }
    }
}
#[test]
fn real_27_channel_checkpoints_and_scores() {
    let f = fixture();
    for model in f["models"].as_array().unwrap() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../services/integrity-model-bun")
            .join(model["dir"].as_str().unwrap());
        let p = Predictor::load(&root).unwrap();
        assert_eq!(p.manifest["model_id"], model["modelId"]);
        for case in model["cases"].as_array().unwrap() {
            close(
                &serde_json::json!(p.score(case["rows"].as_array().unwrap()).unwrap()),
                &case["score"],
            );
        }
    }
}
#[test]
fn legacy_assessment_eligibility_and_wire_response() {
    let p = Predictor::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../services/integrity-model-bun/artifacts-30m"),
    )
    .unwrap();
    for c in fixture()["assessments"].as_array().unwrap() {
        close(&p.assess(&c["request"]).unwrap(), &c["result"]);
    }
}
#[tokio::test]
async fn legacy_http_auth_and_score_contract() {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let p = Predictor::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../services/integrity-model-bun/artifacts-30m"),
    )
    .unwrap();
    let app = warcon_backend::model_service::router(
        warcon_backend::model_service::ModelState::legacy(
            p,
            "fixture-model-token-with-at-least-32-characters",
        )
        .unwrap(),
    );
    let f = fixture();
    let case = &f["assessments"][0];
    let request = |token: &str| {
        Request::post("/v1/assess")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(case["request"].to_string()))
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(request("wrong"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = app
        .oneshot(request("fixture-model-token-with-at-least-32-characters"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    close(
        &serde_json::from_slice(
            &to_bytes(response.into_body(), 8 * 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap(),
        &case["result"],
    );
}
