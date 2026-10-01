use crate::{
    api::roles::require_org_owner,
    auth::{authenticate, server_scope},
    config::AppState,
    error::{ApiError, Result},
    http::ApiJson,
    integrity_ai as ai,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde_json::{Value, json};
pub async fn get(
    State(state): State<AppState>,
    Path(server): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::GET).await?;
    let scope = server_scope(&state, &actor, &server, "integrity.view").await?;
    require_org_owner(&state, &actor, &scope.org_id).await?;
    Ok(Json(
        json!({"ok":true,"settings":ai::view(ai::settings(&state,&scope.org_id).await?.as_ref()),"promptVersion":crate::ai_protocol::PROMPT_VERSION}),
    ))
}
pub async fn post(
    State(state): State<AppState>,
    Path(server): Path<String>,
    headers: HeaderMap,
    ApiJson(input): ApiJson<Value>,
) -> Result<Json<Value>> {
    let actor = authenticate(&state, &headers, &Method::POST).await?;
    let scope = server_scope(&state, &actor, &server, "integrity.view").await?;
    let op = input["operation"].as_str().unwrap_or("");
    if ["preview", "review"].contains(&op) {
        let id = input["caseId"]
            .as_str()
            .filter(|s| crate::ai_protocol::length(s) <= 200)
            .ok_or_else(|| ApiError::bad("请选择案件。"))?;
        let bundle = crate::ai_evidence::bundle(&state, &scope.org_id, &server, id).await?;
        if op == "preview" {
            return Ok(Json(json!({"bundle":bundle})));
        }
        let result = ai::call(&state, &scope.org_id, "review", Some(&bundle), false).await?;
        let mut tx = state.db.begin().await?;
        crate::audit::event(
            &mut tx,
            &actor,
            Some(&scope.org_id),
            &headers,
            "server",
            "integrity.ai.review",
            id,
            json!({"serverId":server}),
        )
        .await?;
        tx.commit().await?;
        return Ok(Json(result.response));
    }
    require_org_owner(&state, &actor, &scope.org_id).await?;
    match op {
        "save" => Ok(Json(
            json!({"ok":true,"settings":ai::save(&state,&actor,&headers,&scope.org_id,&input["settings"]).await?}),
        )),
        "delete" => {
            let mut tx = state.db.begin().await?;
            sqlx::query("DELETE FROM integrity_ai_settings WHERE org_id=$1")
                .bind(&scope.org_id)
                .execute(&mut *tx)
                .await?;
            crate::audit::org_event(
                &mut tx,
                &actor,
                &scope.org_id,
                &headers,
                "integrity.ai.delete",
                "",
                json!({}),
            )
            .await?;
            tx.commit().await?;
            Ok(Json(json!({"ok":true})))
        }
        "models" | "test" => Ok(Json(
            ai::call(&state, &scope.org_id, op, None, false)
                .await?
                .response,
        )),
        _ => Err(ApiError::bad("未知操作。")),
    }
}
