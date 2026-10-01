use crate::{
    api::roles::require_org_owner,
    auth::authenticate,
    config::AppState,
    error::{ApiError, Result},
    http::ApiJson,
    integrity_imports as imports,
};
use axum::{
    Json,
    body::to_bytes,
    extract::{FromRequest, Multipart, Path, Request, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
pub async fn get(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let a = authenticate(&state, &headers, &Method::GET).await?;
    require_org_owner(&state, &a, &org).await?;
    Ok(Json(
        json!({"ok":true,"batches":imports::list(&state,&org).await?}),
    ))
}
pub async fn post(
    State(state): State<AppState>,
    Path(org): Path<String>,
    request: Request,
) -> Result<(StatusCode, Json<Value>)> {
    let headers = request.headers().clone();
    let a = authenticate(&state, &headers, &Method::POST).await?;
    require_org_owner(&state, &a, &org).await?;
    if !headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|s| s.starts_with("multipart/form-data"))
    {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "import_content_type",
            "请使用 multipart/form-data 上传文件。",
        ));
    }
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, imports::MAX_BYTES + 100000)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "import_limit",
                "上传文件超过8 MiB。",
            )
        })?;
    let request = Request::from_parts(parts, axum::body::Body::from(bytes));
    let mut form = Multipart::from_request(request, &state)
        .await
        .map_err(|_| ApiError::bad("上传表单格式无效。"))?;
    let mut raw = None;
    let mut source = None;
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|_| ApiError::bad("上传表单格式无效。"))?
    {
        match field.name().unwrap_or("") {
            "file" => {
                if raw.is_some() {
                    return Err(ApiError::bad("只能上传一个文件。"));
                }
                let name = field
                    .file_name()
                    .ok_or_else(|| ApiError::bad("请选择 JSON 或 JSONL 文件。"))?
                    .to_ascii_lowercase();
                if !name.ends_with(".json") && !name.ends_with(".jsonl") {
                    return Err(ApiError::bad("文件扩展名必须是 .json 或 .jsonl。"));
                }
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| ApiError::bad("上传文件读取失败。"))?;
                if bytes.len() > imports::MAX_BYTES {
                    return Err(ApiError::bad("请选择小于8 MiB的文件。"));
                }
                raw = Some(
                    String::from_utf8(bytes.to_vec())
                        .map_err(|_| ApiError::bad("文件不是有效 UTF-8。"))?,
                );
            }
            "sourceServer" => {
                if source.is_some() {
                    return Err(ApiError::bad("来源服务器字段重复。"));
                }
                source = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad("来源字段无效。"))?,
                );
            }
            _ => {}
        }
    }
    let batch = imports::stage(
        &state,
        &a,
        &headers,
        &org,
        source
            .as_deref()
            .ok_or_else(|| ApiError::bad("请填写来源服务器标识。"))?
            .trim(),
        raw.as_deref()
            .ok_or_else(|| ApiError::bad("请选择文件。"))?,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({"ok":true,"batch":batch}))))
}
pub async fn review(
    State(state): State<AppState>,
    Path((org, batch)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(input): ApiJson<Value>,
) -> Result<Json<Value>> {
    let a = authenticate(&state, &headers, &Method::POST).await?;
    require_org_owner(&state, &a, &org).await?;
    let decision = input["decision"].as_str().unwrap_or("");
    if decision == "APPROVED" && input["confirmation"] != "APPROVE_EXTERNAL_INTEGRITY_DATA" {
        return Err(ApiError::bad("批准外部数据需要明确确认。"));
    }
    Ok(Json(
        json!({"ok":true,"batch":imports::review(&state,&a,&headers,&org,&batch,decision).await?}),
    ))
}
