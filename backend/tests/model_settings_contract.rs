mod common;
use axum::{Json, Router, http::StatusCode, routing::get};
use common::Db;
use serde_json::{Value, json};
use warcon_backend::{crypto, model_http as model};
#[test]
fn model_response_identity_score_shape_and_url_contract() {
    for value in ["http://localhost:4302", "https://model.example/"] {
        assert!(model::model_url(value).is_ok());
    }
    for value in [
        "ftp://model.example",
        "https://user@model.example",
        "http://model.example/v1",
        "https://model.example?token=x",
        "http://model.example/#hash",
    ] {
        assert!(model::model_url(value).is_err());
    }
    let manifest = model::manifest();
    let mut response = json!({"modelId":manifest["model_id"],"checkpointSha256":manifest["checkpoint_sha256"],"schema":manifest["feature_schema"],"requestId":"r","calibrationSha256":manifest["calibration_sha256"],"windowSeconds":1800,"bucketSeconds":30,"status":"READY","score":0.3,"pointScores":vec![0.2;60]});
    assert_eq!(model::verify_result(&response, "r").unwrap(), Some(0.3));
    assert!(model::verify_result(&response, "another").is_err());
    response["pointScores"] = json!(vec![0.2; 59]);
    assert!(model::verify_result(&response, "r").is_err());
    response["status"] = json!("INSUFFICIENT_DATA");
    assert_eq!(model::verify_result(&response, "r").unwrap(), None);
    response["checkpointSha256"] = json!("wrong");
    assert!(model::verify_result(&response, "r").is_err());
    assert!(model::parse_config(Some(json!("broken"))).is_err());
}
#[tokio::test]
#[ignore = "Requires isolated local PostgreSQL development database"]
async fn encrypted_token_optimistic_revision_and_fingerprint_http() {
    let db = Db::new().await;
    let cookie = db.user("owner", true).await;
    sqlx::query("INSERT INTO organizations(id,name,slug) VALUES('org','Org','org')")
        .execute(&db.state.db)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/v1/health",
                get(|headers: axum::http::HeaderMap| async move {
                    assert_eq!(
                        headers.get("authorization").unwrap(),
                        "Bearer fixture-model-token-12345678901234567890"
                    );
                    Json(model::manifest())
                }),
            ),
        )
        .await
        .unwrap();
    });
    let path = "/api/orgs/org/integrity/model";
    let (status, view) = db.call("GET", path, Value::Null, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["config"]["revision"], "empty");
    assert_eq!(view["config"]["hasToken"], false);
    let input = json!({"revision":"empty","developerEnabled":true,"url":format!("http://127.0.0.1:{port}"),"token":"fixture-model-token-12345678901234567890","intervalSeconds":1800});
    let (status, view) = db.call("PUT", path, input.clone(), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["config"]["hasToken"], true);
    assert!(view["config"].get("tokenEnc").is_none());
    let stored: Value =
        sqlx::query_scalar("SELECT value FROM site_settings WHERE key='integrityModel:org'")
            .fetch_one(&db.state.db)
            .await
            .unwrap();
    assert_eq!(
        crypto::decrypt_secret(
            &db.state.config.encryption_key,
            stored["tokenEnc"].as_str().unwrap()
        )
        .unwrap(),
        input["token"]
    );
    assert_eq!(
        db.call("PUT", path, input.clone(), &cookie).await.0,
        StatusCode::CONFLICT
    );
    let (status, test) = db.call("POST", path, json!({}), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{test}");
    let mut next = input.clone();
    next["revision"] = view["config"]["revision"].clone();
    next["token"] = json!("");
    next["autoPunishEnabled"] = json!(false);
    let (status, view) = db.call("PUT", path, next, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["config"]["hasToken"], true);
    assert_eq!(view["config"]["autoPunishEnabled"], false);
    let log: String = sqlx::query_scalar("SELECT string_agg(detail::text,' ') FROM audit_log")
        .fetch_one(&db.state.db)
        .await
        .unwrap();
    assert!(!log.contains("fixture-model-token"));
    assert!(!log.contains("v1."));
    server.abort();
    db.close().await;
}
