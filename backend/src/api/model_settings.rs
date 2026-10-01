use crate::{
    api::roles::require_org_owner, audit, auth::authenticate, config::AppState, error::Result,
    http::ApiJson, model_http as model,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
pub async fn get(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    require_org_owner(&state, &actor, &org).await?;
    let (config, runs) = tokio::try_join!(
        model::config(&state.db, &org),
        model::runs(&state.db, &org, None)
    )?;
    Ok(Json(
        json!({"ok":true,"config":model::view(&config),"runs":runs}),
    ))
}
pub async fn put(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    ApiJson(input): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::PUT).await?;
    require_org_owner(&state, &actor, &org).await?;
    let key = format!("integrityModel:{org}");
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    let old: Option<Value> = sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1")
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await?;
    let config = model::parse_input(&input, &model::parse_config(old)?, &state)?;
    sqlx::query("INSERT INTO site_settings(key,value,updated_by,updated_at) VALUES($1,$2,$3,now()) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_by=excluded.updated_by,updated_at=now()").bind(key).bind(serde_json::to_value(&config).unwrap()).bind(&actor.id).execute(&mut *tx).await?;
    let view = model::view(&config);
    audit::org_event(
        &mut tx,
        &actor,
        &org,
        &headers,
        "integrity.model.configure",
        "",
        view.clone(),
    )
    .await?;
    crate::integrity_rules::changed(&mut tx, &org).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"config":view})))
}
pub async fn test(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::POST).await?;
    require_org_owner(&state, &actor, &org).await?;
    model::verify_health(
        &model::call(&state, &model::config(&state.db, &org).await?, None).await?,
    )?;
    Ok(Json(
        json!({"ok":true,"modelId":model::manifest()["model_id"],"stage":"A测","message":"HTTP 连接及模型指纹验证通过。"}),
    ))
}
