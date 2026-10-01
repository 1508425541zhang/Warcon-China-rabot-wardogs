//! Page controllers own queries, authorization, form validation and redirects. Svelte only
//! transports this contract and renders the result; no database or business runtime is required.
mod account;
mod automation;
mod charts;
mod integrity;
mod management;
use crate::{
    auth::{self, Actor},
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, Peer},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;

pub async fn invalidate_catalog(state: &AppState, id: &str) {
    management::invalidate_catalog(state, id).await;
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub source: String,
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub form: Value,
}
pub struct Context {
    pub state: AppState,
    pub headers: HeaderMap,
    pub peer: Peer,
    pub input: Input,
    service: Router,
    pub cookies: Vec<String>,
}
impl Context {
    pub fn param(&self, name: &str) -> &str {
        self.input
            .params
            .get(name)
            .map(String::as_str)
            .unwrap_or("")
    }
    pub fn query(&self, name: &str) -> String {
        url::form_urlencoded::parse(self.input.search.trim_start_matches('?').as_bytes())
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    }
    pub fn form(&self, name: &str) -> &Value {
        let v = &self.input.form[name];
        if let Some(a) = v.as_array() {
            a.first().unwrap_or(&Value::Null)
        } else {
            v
        }
    }
    pub fn text(&self, name: &str, max: usize) -> String {
        crate::http::string(self.form(name), max)
    }
    pub fn raw(&self, name: &str) -> String {
        self.form(name).as_str().unwrap_or("").to_owned()
    }
    pub fn fields(&self) -> Value {
        self.input
            .form
            .as_object()
            .map(|m| {
                Value::Object(
                    m.iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                v.as_array().and_then(|a| a.first()).unwrap_or(v).clone(),
                            )
                        })
                        .collect(),
                )
            })
            .unwrap_or(json!({}))
    }
    pub async fn actor(&self) -> Result<Actor> {
        let a = auth::authenticate(&self.state, &self.headers, &Method::GET).await?;
        if a.key.is_some() {
            return Err(ApiError::forbidden());
        }
        Ok(a)
    }
    pub async fn account(&self) -> Result<Option<Actor>> {
        match auth::authenticate_account(&self.state, &self.headers, &Method::GET).await {
            Ok(a) if a.key.is_none() => Ok(Some(a)),
            Ok(_) => Err(ApiError::forbidden()),
            Err(e) if e.status == StatusCode::UNAUTHORIZED => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub async fn api(&mut self, method: Method, path: &str, body: Value) -> Result<Value> {
        if !path.starts_with("/api/") && !path.starts_with("/metrics") {
            return Err(ApiError::bad("Invalid internal route."));
        }
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::from(body.to_string()))
            .map_err(|_| ApiError::bad("Invalid page request."))?;
        *request.headers_mut() = self.headers.clone();
        request.headers_mut().remove("content-length");
        request.headers_mut().remove("transfer-encoding");
        request.headers_mut().insert(
            "content-type",
            axum::http::HeaderValue::from_static("application/json"),
        );
        if let Some(peer) = self.peer.0 {
            request.extensions_mut().insert(ConnectInfo(peer));
        }
        let response = self
            .service
            .clone()
            .oneshot(request)
            .await
            .expect("Router is infallible");
        for cookie in response.headers().get_all("set-cookie") {
            if let Ok(v) = cookie.to_str() {
                self.cookies.push(v.into());
            }
        }
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 12 * 1024 * 1024)
            .await
            .map_err(|_| ApiError::bad("Page response too large."))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "page_response",
                "Invalid backend response.",
            )
        })?;
        if !status.is_success() {
            return Err(ApiError::new(
                status,
                "page_api",
                value["error"]["message"]
                    .as_str()
                    .unwrap_or("Request failed."),
            ));
        }
        Ok(value)
    }
    pub async fn get(&mut self, path: &str) -> Result<Value> {
        self.api(Method::GET, path, Value::Null).await
    }
}
pub fn segment(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}
pub fn go(status: u16, path: &str) -> Value {
    json!({"$redirect":path,"$status":status})
}
pub fn failure(status: u16, data: Value) -> Value {
    json!({"$failed":true,"$status":status,"$data":data})
}
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/page/load", post(load))
        .route("/api/page/action", post(action))
        .with_state(state)
}
async fn load(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(input): ApiJson<Input>,
) -> Response {
    run(state, peer, headers, input, false).await
}
async fn action(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    ApiJson(input): ApiJson<Input>,
) -> Response {
    run(state, peer, headers, input, true).await
}
async fn run(
    state: AppState,
    peer: Peer,
    headers: HeaderMap,
    mut input: Input,
    action: bool,
) -> Response {
    if headers.contains_key("authorization") {
        return ApiError::forbidden().into_response();
    }
    if input.search.len() > 8000
        || input.source.len() > 300
        || input.params.values().any(|v| v.len() > 200)
    {
        return ApiError::bad("Invalid page parameters.").into_response();
    }
    // Whitelist by exact source file, never by a user-controlled filesystem path.
    input.source = input.source.replace('\\', "/");
    let mut c = Context {
        service: crate::api::business_router(state.clone()),
        state,
        peer,
        headers,
        input,
        cookies: vec![],
    };
    if action {
        if let Err(e) = crate::identity::origin(&c.state, &c.headers) {
            return e.into_response();
        }
    }
    let result = if action {
        account::action(&mut c).await
    } else {
        dispatch_load(&mut c).await
    };
    let mut response = match result {
        Ok(v) if v.get("$redirect").is_some() => {
            axum::Json(json!({"redirect":v["$redirect"],"status":v["$status"]})).into_response()
        }
        Ok(v) if v.get("$failed").is_some() => {
            axum::Json(json!({"failed":true,"status":v["$status"],"data":v["$data"]}))
                .into_response()
        }
        Ok(v) => axum::Json(json!({"data":v})).into_response(),
        Err(e) if action => {
            axum::Json(json!({"failed":true,"status":e.status.as_u16(),"data":{"error":e.message}}))
                .into_response()
        }
        Err(e) => e.into_response(),
    };
    for cookie in c.cookies {
        if let Ok(v) = axum::http::HeaderValue::from_str(&cookie) {
            response.headers_mut().append("set-cookie", v);
        }
    }
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}
async fn dispatch_load(c: &mut Context) -> Result<Value> {
    let source = c.input.source.clone();
    if source.starts_with("src/routes/(auth)/")
        || source == "src/routes/(app)/account/+page.server.ts"
    {
        return account::load(c).await;
    }
    if [
        "src/routes/(app)/server/[id]/automation/+page.server.ts",
        "src/routes/(app)/server/[id]/faction-lock/+page.server.ts",
        "src/routes/(app)/server/[id]/settings/+page.server.ts",
    ]
    .contains(&source.as_str())
    {
        return automation::load(c).await;
    }
    if source == "src/routes/(app)/server/[id]/players/[steamId]/+page.server.ts" {
        return charts::player(c).await;
    }
    if [
        "src/routes/(app)/orgs/[id]/integrity/+page.server.ts",
        "src/routes/(app)/server/[id]/integrity/+page.server.ts",
        "src/routes/(app)/server/[id]/integrity/cases/+page.server.ts",
        "src/routes/(app)/server/[id]/integrity/ai/+page.server.ts",
    ]
    .contains(&source.as_str())
    {
        return integrity::load(c).await;
    }
    management::load(c).await
}
