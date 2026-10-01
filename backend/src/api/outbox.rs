use crate::{
    auth::{authenticate, server_scope},
    config::AppState,
    error::Result,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    server_scope(&state, &actor, &id, "automation.manage").await?;
    let items:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'triggerId',trigger_id,'triggerName',trigger_name,'triggerKind',trigger_kind,'action',action,'target',target,'state',state,'attempts',attempts,'outcome',outcome,'createdAt',to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),'doneAt',to_char(done_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')) FROM outbox WHERE server_id=$1 ORDER BY id DESC LIMIT 40").bind(id).fetch_all(&state.db).await?;
    Ok(Json(json!({"ok":true,"items":items})))
}
