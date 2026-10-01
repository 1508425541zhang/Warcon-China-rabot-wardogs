//! Authenticated native model service. Sources are selected exclusively by a saved
//! observation job, never by client supplied arbitrary player/server selectors.
use crate::{model::Predictor, model_features};
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
const MAX_BODY: usize = 8 * 1024 * 1024;
#[derive(Clone)]
pub struct ModelState {
    pub predictor: Arc<Predictor>,
    pub pool: PgPool,
    authorization: [u8; 32],
    pub busy: Arc<Semaphore>,
}
impl ModelState {
    pub fn new(predictor: Predictor, pool: PgPool, token: &str) -> Result<Self> {
        ensure!(
            token.encode_utf16().count() >= 32 && !token.chars().any(char::is_whitespace),
            "Invalid MODEL_API_TOKEN"
        );
        Ok(Self {
            predictor: Arc::new(predictor),
            pool,
            authorization: Sha256::digest(format!("Bearer {token}")).into(),
            busy: Arc::new(Semaphore::new(1)),
        })
    }
}
pub fn router(state: ModelState) -> Router {
    Router::new().fallback(handle).with_state(state)
}
fn reply(status: StatusCode, body: Value) -> Response {
    (
        status,
        [
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        Json(body),
    )
        .into_response()
}
async fn handle(State(state): State<ModelState>, request: Request) -> Response {
    let actual: [u8; 32] = Sha256::digest(
        request
            .headers()
            .get("authorization")
            .map(|h| h.as_bytes())
            .unwrap_or(&[]),
    )
    .into();
    if !bool::from(actual.ct_eq(&state.authorization)) {
        return reply(StatusCode::UNAUTHORIZED, json!({"error":"Unauthorized"}));
    }
    if request.method() == Method::GET && request.uri().path() == "/v1/health" {
        let mut metadata = state.predictor.manifest_json.clone();
        metadata["ok"] = json!(true);
        return reply(StatusCode::OK, metadata);
    }
    if request.method() != Method::POST || request.uri().path() != "/v1/assess" {
        return reply(StatusCode::NOT_FOUND, json!({"error":"Not found"}));
    }
    let Ok(permit) = state.busy.clone().try_acquire_owned() else {
        return reply(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"Inference busy"}),
        );
    };
    let bytes = match tokio::time::timeout(
        Duration::from_secs(30),
        to_bytes(request.into_body(), MAX_BODY),
    )
    .await
    {
        Ok(Ok(b)) if !b.is_empty() => b,
        _ => {
            return reply(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"error":"Invalid body size"}),
            );
        }
    };
    let body: Value = match serde_json::from_slice(&bytes) {
        Ok(b) => b,
        Err(_) => return reply(StatusCode::BAD_REQUEST, json!({"error":"Invalid JSON"})),
    };
    let predictor = state.predictor.clone();
    if body["schema"] != predictor.manifest.feature_schema || !body["requestId"].is_string() {
        return reply(
            StatusCode::BAD_REQUEST,
            json!({"error":"Invalid inference input"}),
        );
    }
    let source = match read_source(&state.pool, &body).await {
        Ok(s) => s,
        Err(_) => {
            return reply(
                StatusCode::BAD_REQUEST,
                json!({"error":"Invalid inference input"}),
            );
        }
    };
    // Move the permit into the blocking job so disconnecting a request cannot
    // release capacity while its CPU inference is still running.
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        assess_source(&predictor, &body, &source)
    })
    .await
    {
        Ok(Ok(result)) => reply(StatusCode::OK, result),
        Ok(Err(_)) => reply(
            StatusCode::BAD_REQUEST,
            json!({"error":"Invalid inference input"}),
        ),
        Err(_) => reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error":"Inference failed"}),
        ),
    }
}
pub fn assess_source(predictor: &Predictor, request: &Value, source: &Value) -> Result<Value> {
    ensure!(
        request["schema"] == predictor.manifest.feature_schema && request["requestId"].is_string(),
        "Invalid request"
    );
    let mut identity = json!({"modelId":predictor.manifest.model_id,"checkpointSha256":predictor.manifest.checkpoint_sha256,"schema":predictor.manifest.feature_schema,"requestId":request["requestId"],"stage":"A测","calibrationSha256":predictor.manifest.calibration_sha256,"windowSeconds":1800,"bucketSeconds":30});
    let rows = model_features::build(source, &predictor.contract);
    let k = predictor
        .contract
        .features
        .iter()
        .position(|n| n == "small_arm_kills_60s")
        .ok_or_else(|| anyhow::anyhow!("Missing core feature"))?;
    let observed = rows.iter().filter(|r| r[k].is_finite()).count();
    identity["observedBuckets"] = json!(observed);
    identity["requiredBuckets"] = json!(60);
    if rows.len() != 60 || observed < 48 {
        identity["status"] = json!("INSUFFICIENT_DATA");
        identity["reason"] = json!("需要同一对局连续30分钟序列及至少80%的步兵事件覆盖。");
        return Ok(identity);
    }
    let score = predictor.score(&model_features::normalize(&rows, &predictor.contract))?;
    identity["status"] = json!("READY");
    identity["windowEnd"] = source["end"].clone();
    identity["featureCount"] = json!(predictor.contract.features.len());
    identity["score"] = json!(score.score);
    identity["pointScores"] = json!(score.point_scores);
    Ok(identity)
}

pub async fn read_source(pool: &PgPool, request: &Value) -> Result<Value> {
    let id = request["requestId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing registered job"))?;
    uuid::Uuid::parse_str(id)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout='10s'")
        .execute(&mut *tx)
        .await?;
    let job = sqlx::query(
        "SELECT server_id,steam_id,match_id,created_at FROM integrity_model_runs WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Missing registered job"))?;
    let server: String = job.try_get("server_id")?;
    let player: String = job.try_get("steam_id")?;
    let match_id: i64 = job.try_get("match_id")?;
    let created: chrono::DateTime<chrono::Utc> = job.try_get("created_at")?;
    ensure!(
        request["sources"]["matches"][0]["id"].as_i64() == Some(match_id)
            || request["sources"]["matches"][0]["id"].as_str()
                == Some(match_id.to_string().as_str()),
        "Observation scope mismatch"
    );
    let match_:Value=sqlx::query_scalar("SELECT to_jsonb(m) FROM (SELECT id,server_id,started_at,map FROM matches WHERE id=$1 AND server_id=$2) m").bind(&match_id).bind(&server).fetch_optional(&mut *tx).await?.ok_or_else(||anyhow::anyhow!("Match missing"))?;
    let started = chrono::DateTime::parse_from_rfc3339(
        match_["started_at"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing start"))?,
    )?
    .with_timezone(&chrono::Utc);
    let end =
        chrono::DateTime::<chrono::Utc>::from_timestamp(created.timestamp().div_euclid(30) * 30, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid end"))?;
    let since = (end - chrono::Duration::seconds(2100)).max(started);
    let kills:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM (SELECT ts,server_id,event_id,instance_id,match_row,event_time,map,killer_steam_id,victim_steam_id,cause,distance_m,distance_invalid,headshot,suicide,team_kill,tags FROM kills WHERE server_id=$1 AND match_row=$2 AND ts>=$3 AND ts<=$4 ORDER BY ts,event_id LIMIT 20001) k").bind(&server).bind(&match_id).bind(since).bind(end).fetch_all(&mut *tx).await?;
    let batches:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(b) FROM (SELECT received_at,instance_id,payload FROM training_feed_batches WHERE server_id=$1 AND received_at>=$2 AND received_at<=$3 ORDER BY received_at,id LIMIT 10001) b").bind(&server).bind(since).bind(end).fetch_all(&mut *tx).await?;
    let observations:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(o) FROM (SELECT received_at,endpoint,CASE WHEN endpoint='/v1/players' THEN jsonb_build_object('roster_size',jsonb_array_length(CASE WHEN jsonb_typeof(payload)='array' THEN payload ELSE COALESCE(payload->'players','[]'::jsonb) END),'players',(SELECT COALESCE(jsonb_agg(p-'name'),'[]'::jsonb) FROM jsonb_array_elements(CASE WHEN jsonb_typeof(payload)='array' THEN payload ELSE COALESCE(payload->'players','[]'::jsonb) END) p WHERE p->>'steamId'=$4)) ELSE payload END AS payload FROM training_observations WHERE server_id=$1 AND received_at>=$2 AND received_at<=$3 AND endpoint IN ('/v1/players','/v1/status') ORDER BY received_at,id LIMIT 20001) o").bind(&server).bind(since).bind(end).bind(&player).fetch_all(&mut *tx).await?;
    let progress:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(p) FROM (SELECT observed_at,jsonb_array_length(players) AS roster_size,(SELECT COALESCE(jsonb_agg(p-'name'),'[]'::jsonb) FROM jsonb_array_elements(players) p WHERE p->>'steamId'=$5) AS players FROM player_progress_samples WHERE server_id=$1 AND match_id=$2 AND observed_at>=$3 AND observed_at<=$4 ORDER BY observed_at LIMIT 10001) p").bind(&server).bind(&match_id).bind(since).bind(end).bind(&player).fetch_all(&mut *tx).await?;
    ensure!(
        kills.len() <= 20000
            && batches.len() <= 10000
            && observations.len() <= 20000
            && progress.len() <= 10000,
        "Observation limit exceeded; no silent truncation"
    );
    tx.commit().await?;
    Ok(
        json!({"match":match_,"player":player,"end":end.to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"kills":kills,"batches":batches,"observations":observations,"progress":progress}),
    )
}
