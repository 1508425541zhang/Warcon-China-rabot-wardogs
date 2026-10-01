use crate::{
    auth::{self, Actor},
    config::AppState,
    error::{ApiError, Result},
};
use axum::{
    Json,
    body::to_bytes,
    extract::{FromRequest, Path, Request, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
pub struct PluginJson(Value);
impl<S: Send + Sync> FromRequest<S> for PluginJson {
    type Rejection = ApiError;
    async fn from_request(r: Request, _: &S) -> Result<Self> {
        if !r
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|s| s.contains("application/json"))
        {
            return Err(ApiError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "请使用 application/json。",
            ));
        }
        let bytes = to_bytes(r.into_body(), 65536).await.map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "body_size",
                "插件配置不得超过 64 KB。",
            )
        })?;
        Ok(Self(
            serde_json::from_slice(&bytes).map_err(|_| ApiError::bad("JSON 格式不正确。"))?,
        ))
    }
}
fn invalid() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_plugin",
        "插件字段格式不正确，请按插件 API 文档填写。",
    )
}
fn strict(v: &Value, keys: &[&str]) -> Result<()> {
    if !v
        .as_object()
        .is_some_and(|o| o.keys().all(|k| keys.contains(&k.as_str())))
    {
        Err(invalid())
    } else {
        Ok(())
    }
}
fn text(v: &Value, min: usize, max: usize) -> bool {
    v.as_str().is_some_and(|s| {
        let n = s.encode_utf16().count();
        n >= min && n <= max
    })
}
fn identifier(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        (2..=48).contains(&s.len())
            && s.as_bytes()[0].is_ascii_lowercase()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}
fn integer(v: &Value, min: i64, max: i64) -> bool {
    v.as_f64()
        .is_some_and(|n| n.fract() == 0. && n >= min as f64 && n <= max as f64)
}
fn default(v: &mut Value, k: &str, d: Value) {
    if v.get(k).is_none() {
        v[k] = d
    }
}
pub fn manifest(mut v: Value) -> Result<Value> {
    strict(
        &v,
        &[
            "apiVersion",
            "id",
            "name",
            "description",
            "version",
            "renderer",
            "style",
            "widgets",
        ],
    )?;
    if v["apiVersion"].as_f64() != Some(1.) || !identifier(&v["id"]) {
        return Err(invalid());
    }
    let n = v["name"].as_str().ok_or_else(invalid)?.trim().to_owned();
    v["name"] = json!(n);
    if !text(&v["name"], 1, 80) {
        return Err(invalid());
    }
    default(&mut v, "description", json!(""));
    default(&mut v, "version", json!("1.0.0"));
    default(&mut v, "renderer", json!("cards"));
    default(&mut v, "style", json!({}));
    default(&mut v, "widgets", json!([]));
    if !text(&v["description"], 0, 500)
        || !identifier(&v["renderer"])
        || !v["version"].as_str().is_some_and(|s| {
            let p = s.split('.').collect::<Vec<_>>();
            p.len() == 3
                && p.iter()
                    .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        })
    {
        return Err(invalid());
    }
    strict(&v["style"], &["accent", "columns", "density"])?;
    default(&mut v["style"], "accent", json!("#69d6e3"));
    default(&mut v["style"], "columns", json!(2));
    default(&mut v["style"], "density", json!("comfortable"));
    if !v["style"]["accent"].as_str().is_some_and(|s| {
        s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
    }) || !integer(&v["style"]["columns"], 1, 3)
        || !v["style"]["density"]
            .as_str()
            .is_some_and(|s| ["comfortable", "compact"].contains(&s))
    {
        return Err(invalid());
    }
    let cards = v["renderer"] == "cards";
    let widgets = v["widgets"].as_array_mut().ok_or_else(invalid)?;
    if widgets.len() > 12 || (cards && widgets.is_empty()) {
        return Err(invalid());
    }
    for w in widgets {
        let t = w["type"].as_str().unwrap_or("").to_owned();
        let keys = match t.as_str() {
            "metric" => vec!["type", "title", "metric"],
            "text" => vec!["type", "title", "text"],
            "players" => vec!["type", "title", "columns", "limit", "sortBy"],
            _ => return Err(invalid()),
        };
        strict(w, &keys)?;
        if !text(&w["title"], 1, 80) {
            return Err(invalid());
        }
        match t.as_str() {
            "metric" => {
                if !w["metric"].as_str().is_some_and(|s| {
                    ["online", "kills", "deaths", "averagePing", "cash"].contains(&s)
                }) {
                    return Err(invalid());
                }
            }
            "text" => {
                if !text(&w["text"], 0, 2000) {
                    return Err(invalid());
                }
            }
            _ => {
                default(w, "limit", json!(10));
                default(w, "sortBy", json!("kills"));
                if !integer(&w["limit"], 1, 50)
                    || !w["sortBy"]
                        .as_str()
                        .is_some_and(|s| ["kills", "deaths", "cash", "ping"].contains(&s))
                    || !w["columns"].as_array().is_some_and(|a| {
                        (1..=7).contains(&a.len())
                            && a.iter().all(|v| {
                                v.as_str().is_some_and(|s| {
                                    [
                                        "name", "steamId", "faction", "kills", "deaths", "cash",
                                        "ping",
                                    ]
                                    .contains(&s)
                                })
                            })
                    })
                {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(v)
}
async fn user(state: &AppState, headers: &HeaderMap, method: &Method) -> Result<Actor> {
    let actor = auth::authenticate(state, headers, method).await?;
    if actor.key.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "api_key_forbidden",
            "个人插件设置需要账号会话。",
        ));
    }
    Ok(actor)
}
pub async fn list_for(state: &AppState, id: &str) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('manifest',manifest,'serverId',server_id,'enabled',enabled,'updatedAt',updated_at) FROM personal_plugins WHERE user_id=$1 ORDER BY updated_at DESC").bind(id).fetch_all(&state.db).await?)
}
pub async fn get_for(state: &AppState, user: &str, id: &str) -> Result<Value> {
    sqlx::query_scalar("SELECT jsonb_build_object('manifest',manifest,'serverId',server_id,'enabled',enabled,'updatedAt',updated_at) FROM personal_plugins WHERE user_id=$1 AND plugin_id=$2").bind(user).bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)
}
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let u = user(&state, &headers, &Method::GET).await?;
    Ok(Json(
        json!({"ok":true,"plugins":list_for(&state,&u.id).await?}),
    ))
}
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let u = user(&state, &headers, &Method::GET).await?;
    Ok(Json(
        json!({"ok":true,"plugin":get_for(&state,&u.id,&id).await?}),
    ))
}
async fn save(
    state: &AppState,
    headers: &HeaderMap,
    mut body: Value,
    id: Option<&str>,
) -> Result<Value> {
    strict(&body, &["manifest", "serverId", "enabled"])?;
    let m = manifest(body["manifest"].clone())?;
    default(&mut body, "serverId", Value::Null);
    default(&mut body, "enabled", json!(true));
    if !body["enabled"].is_boolean()
        || (!body["serverId"].is_null() && !text(&body["serverId"], 1, 64))
    {
        return Err(invalid());
    }
    let compiled: Value = serde_json::from_str(include_str!(
        "../../../src/lib/plugins/extensions/round-summary/manifest.json"
    ))
    .unwrap();
    if m["renderer"] != "cards" && m["renderer"] != compiled["renderer"] {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unknown_renderer",
            "此代码插件尚未随构建安装。",
        ));
    }
    let plugin = m["id"].as_str().unwrap();
    if id.is_some_and(|id| id != plugin) {
        return Err(ApiError::bad("编辑时不能修改插件标识。"));
    }
    let method = if id.is_some() {
        Method::PUT
    } else {
        Method::POST
    };
    user(state, headers, &method).await?;
    let mut tx = state.db.begin().await?;
    super::users::account_lock(&mut tx).await?;
    let actor = user(state, headers, &method).await?;
    if let Some(server) = body["serverId"].as_str() {
        auth::server_scope(state, &actor, server, "server.view").await?;
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("personal-plugins:{}", actor.id))
        .execute(&mut *tx)
        .await?;
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT plugin_id FROM personal_plugins WHERE user_id=$1")
            .bind(&actor.id)
            .fetch_all(&mut *tx)
            .await?;
    let exists = rows.iter().any(|s| s == plugin);
    if id.is_some() && !exists {
        return Err(ApiError::missing());
    }
    if id.is_none() && exists {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "plugin_exists",
            "同名标识已存在。",
        ));
    }
    if !exists && rows.len() >= 20 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "plugin_limit",
            "每个账号最多保存 20 个个人插件。",
        ));
    }
    let saved:Value=sqlx::query_scalar("INSERT INTO personal_plugins(user_id,plugin_id,manifest,server_id,enabled,updated_at) VALUES($1,$2,$3,$4,$5,now()) ON CONFLICT(user_id,plugin_id) DO UPDATE SET manifest=excluded.manifest,server_id=excluded.server_id,enabled=excluded.enabled,updated_at=now() RETURNING jsonb_build_object('manifest',manifest,'serverId',server_id,'enabled',enabled,'updatedAt',updated_at)").bind(&actor.id).bind(plugin).bind(&m).bind(body["serverId"].as_str()).bind(body["enabled"].as_bool()).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(saved)
}
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    PluginJson(body): PluginJson,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(json!({"ok":true,"plugin":save(&state,&headers,body,None).await?})),
    ))
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    PluginJson(body): PluginJson,
) -> Result<Json<Value>> {
    Ok(Json(
        json!({"ok":true,"plugin":save(&state,&headers,body,Some(&id)).await?}),
    ))
}
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let u = user(&state, &headers, &Method::DELETE).await?;
    if sqlx::query("DELETE FROM personal_plugins WHERE user_id=$1 AND plugin_id=$2")
        .bind(&u.id)
        .bind(&id)
        .execute(&state.db)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::missing());
    }
    Ok(Json(json!({"ok":true})))
}
pub async fn snapshot(state: &AppState, actor: &Actor, id: &str) -> Result<Value> {
    let scope = auth::server_scope(state, actor, id, "server.view").await?;
    let live: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=$1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let live = live.unwrap_or(Value::Null);
    let raw = live["players"].as_array().cloned().unwrap_or_default();
    let players=raw.iter().take(256).filter(|p|p.is_object()).map(|p|json!({"name":crate::http::string(&p["name"],usize::MAX),"steamId":crate::http::string(&p["steamId"],usize::MAX),"faction":p["faction"].as_str(),"kills":p["kills"].as_f64(),"deaths":p["deaths"].as_f64(),"cash":p["cash"].as_f64(),"ping":p["ping"].as_f64()})).collect::<Vec<_>>();
    let has_players = !live["players_at"].is_null() && live["players"].is_array();
    let sum = |key: &str| {
        if has_players && raw.len() <= 256 && players.iter().all(|p| p[key].is_number()) {
            Some(
                players
                    .iter()
                    .map(|p| p[key].as_f64().unwrap())
                    .sum::<f64>(),
            )
        } else {
            None
        }
    };
    let pings = players
        .iter()
        .filter_map(|p| p["ping"].as_f64())
        .collect::<Vec<_>>();
    let fresh = live["players_at"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|t| (chrono::Utc::now() - t.to_utc()).num_seconds() <= 90);
    Ok(
        json!({"apiVersion":1,"server":{"id":id,"name":scope.name,"map":live["status"]["map"].as_str()},"statusAt":live["status_at"],"playersAt":live["players_at"],"stale":!fresh,"metrics":{"online":has_players.then_some(raw.len()),"kills":sum("kills"),"deaths":sum("deaths"),"cash":sum("cash"),"averagePing":if has_players&&raw.len()<=256&&!pings.is_empty(){Some((pings.iter().sum::<f64>()/pings.len() as f64+0.5).floor())}else{None}},"players":players,"playersTruncated":raw.len()>256}),
    )
}
pub async fn snapshot_api(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let actor = auth::authenticate(&state, &h, &Method::GET).await?;
    Ok(Json(
        json!({"ok":true,"apiVersion":1,"snapshot":snapshot(&state,&actor,&id).await?}),
    ))
}
pub async fn data(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let u = user(&state, &headers, &Method::GET).await?;
    let p = get_for(&state, &u.id, &id).await?;
    if p["enabled"] != true {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "plugin_disabled",
            "插件已停用。",
        ));
    }
    let snapshot = if let Some(id) = p["serverId"].as_str() {
        snapshot(&state, &u, id).await?
    } else {
        Value::Null
    };
    Ok(Json(json!({"ok":true,"apiVersion":1,"snapshot":snapshot})))
}
