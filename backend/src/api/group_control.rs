use crate::{auth, config::AppState, error::Result, http::ApiJson};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
pub async fn post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = auth::authenticate(&state, &h, &Method::POST).await?;
    let server = auth::server_scope(&state, &actor, &id, "automation.manage").await?;
    let mut tx = state.db.begin().await?;
    let scan = b["operation"] == "scan";
    let cfg = if scan {
        json!({})
    } else {
        crate::group_control::validate(&b["config"])?
    };
    if scan {
        crate::group_control::scan(&mut tx, &id, true).await?
    } else {
        sqlx::query("INSERT INTO group_control_rules(server_id,config,updated_at)VALUES($1,$2,now())ON CONFLICT(server_id)DO UPDATE SET config=excluded.config,updated_at=now()").bind(&id).bind(&cfg).execute(&mut *tx).await?;
        if cfg["mode"] == "off" {
            sqlx::query("UPDATE group_control_scans SET ai_status='not_requested',ai_result=NULL WHERE server_id=$1 AND ai_status IN('pending','processing')").bind(&id).execute(&mut *tx).await?;
        }
    }
    crate::trigger_store::audit(
        &mut tx,
        &actor,
        &server,
        &h,
        if scan {
            "group_control.scan"
        } else {
            "group_control.settings"
        },
        "",
        &cfg,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
