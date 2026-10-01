use crate::{
    auth,
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, ApiQuery, integer, string},
    player_views as views,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
pub async fn dossier(
    State(state): State<AppState>,
    Path((id, steam)): Path<(String, String)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::GET).await?;
    let s = auth::server_scope(&state, &a, &id, "server.view").await?;
    super::notes::steam_id(&steam)?;
    Ok(Json(
        json!({"ok":true,"dossier":views::dossier(&state,&a,&s,&steam).await?}),
    ))
}
pub async fn career(
    State(state): State<AppState>,
    Path((id, steam)): Path<(String, String)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::GET).await?;
    let s = auth::server_scope(&state, &a, &id, "server.view").await?;
    super::notes::steam_id(&steam)?;
    let (ids, names) = views::visible(&state, &a, &s.org_id).await?;
    Ok(Json(
        json!({"ok":true,"career":crate::leaderboards::career(&state,&id,&ids,&names,&steam).await?}),
    ))
}
pub async fn marks(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiQuery(q): ApiQuery<HashMap<String, String>>,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::GET).await?;
    let s = auth::server_scope(&state, &a, &id, "server.view").await?;
    let names = q
        .get("names")
        .map(String::as_str)
        .unwrap_or("")
        .split('\n')
        .collect::<Vec<_>>();
    let players = q
        .get("ids")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|id| super::notes::steam_id(id).is_ok())
        .take(200)
        .enumerate()
        .map(|(i, id)| json!({"steamId":id,"name":string(&json!(names.get(i)),100)}))
        .collect::<Vec<_>>();
    let (ids, _) = views::visible(&state, &a, &s.org_id).await?;
    let inputs =
        crate::player_risk::inputs(&state, &s.org_id, &ids, Some(&id), &players, true).await?;
    let steam: Vec<_> = players
        .iter()
        .filter_map(|p| p["steamId"].as_str().map(str::to_owned))
        .collect();
    let visits:Vec<(String,i64)>=sqlx::query_as("SELECT steam_id,count(*)FROM player_sessions WHERE server_id=$1 AND steam_id=ANY($2)GROUP BY steam_id").bind(&id).bind(&steam).fetch_all(&state.db).await?;
    crate::steam::request_refresh(&state, &steam).await?;
    let staff = s
        .caps
        .iter()
        .any(|c| ["players.notes", "players.notes.manage"].contains(&c.as_str()));
    let mut seen = HashSet::new();
    let mut out = vec![];
    for id in steam {
        if !seen.insert(id.clone()) {
            continue;
        }
        let mut input = inputs.get(&id).cloned().unwrap_or(Value::Null);
        let watched = input["watched"].is_object();
        let reason = if staff {
            input["watched"]["reason"].as_str().unwrap_or("").to_owned()
        } else {
            String::new()
        };
        if !staff && watched {
            input["watched"]["reason"] = json!("")
        }
        out.push(json!({"steamId":id,"watched":watched,"reason":reason,"firstVisit":visits.iter().find(|r|r.0==id).map(|r|r.1).unwrap_or(0)<=1,"risk":crate::player_risk::assess(&input,chrono::Utc::now().timestamp_millis())}));
    }
    Ok(Json(json!({"ok":true,"marks":out})))
}
pub async fn seen(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiQuery(mut q): ApiQuery<HashMap<String, String>>,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::GET).await?;
    let s = auth::server_scope(&state, &a, &id, "server.view").await?;
    crate::ratelimit::allow(format!("seen:{}", a.id), 60, true)?;
    q.remove("server");
    let mut out = views::seen(
        &state,
        &s.org_id,
        &[id],
        &q,
        integer(&json!(q.get("limit")), 50, 1, 100),
        integer(&json!(q.get("offset")), 0, 0, 1000000),
        false,
    )
    .await?;
    out["ok"] = json!(true);
    Ok(Json(out))
}
pub async fn org_seen(
    State(state): State<AppState>,
    Path(org): Path<String>,
    h: HeaderMap,
    ApiQuery(q): ApiQuery<HashMap<String, String>>,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::GET).await?;
    let row: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(o)FROM organizations o WHERE id=$1")
            .bind(&org)
            .fetch_optional(&state.db)
            .await?;
    let row = row.ok_or_else(ApiError::missing)?;
    if super::lists::role_for(&state, &a, &org).await?.is_none() {
        return Err(ApiError::missing());
    }
    if !row["suspended_at"].is_null() && !a.owner {
        return Err(ApiError::forbidden());
    }
    let (ids, _) = views::visible(&state, &a, &org).await?;
    let mut out = views::seen(
        &state,
        &org,
        &ids,
        &q,
        integer(&json!(q.get("limit")), 100, 1, 200),
        integer(&json!(q.get("offset")), 0, 0, 1000000),
        true,
    )
    .await?;
    out["ok"] = json!(true);
    Ok(Json(out))
}
pub async fn refresh(
    State(state): State<AppState>,
    Path((id, steam)): Path<(String, String)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::POST).await?;
    if a.key.is_some() {
        return Err(ApiError::forbidden());
    }
    let s = auth::server_scope(&state, &a, &id, "server.view").await?;
    super::notes::steam_id(&steam)?;
    crate::ratelimit::allow(format!("steam:{}", a.id), 20, true)?;
    if let Some(key) = std::env::var("STEAM_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
    {
        let client = crate::steam::SteamClient::new(key)
            .map_err(|_| ApiError::bad("Steam 客户端配置不可用。"))?;
        let profile = client.profile(&steam).await.map_err(|_| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "steam_unreachable",
                "Steam 未返回有效档案。",
            )
        })?;
        let mut tx = state.db.begin().await?;
        crate::steam::save(&mut tx, &profile)
            .await
            .map_err(|_| ApiError::bad("Steam 档案保存失败。"))?;
        tx.commit().await?;
        crate::steam::refresh_friends(&state, &client, &steam).await?;
    }
    Ok(Json(
        json!({"ok":true,"dossier":views::dossier(&state,&a,&s,&steam).await?}),
    ))
}
pub async fn profiles(
    State(state): State<AppState>,
    h: HeaderMap,
    ApiQuery(q): ApiQuery<HashMap<String, String>>,
) -> Result<Response> {
    let a = auth::authenticate(&state, &h, &Method::GET).await?;
    if super::servers::accessible(&state, &a, None)
        .await?
        .is_empty()
    {
        return Err(ApiError::forbidden());
    }
    crate::ratelimit::allow(format!("steam:{}", a.id), 60, true)?;
    let key = std::env::var("STEAM_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "steam_disabled",
                "Steam lookup is not configured.",
            )
        })?;
    let mut seen = HashSet::new();
    let ids: Vec<String> = q
        .get("ids")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| super::notes::steam_id(s).is_ok() && seen.insert(s.to_string()))
        .take(100)
        .map(str::to_owned)
        .collect();
    type Cache = HashMap<String, (Instant, Value)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let name = format!("{}:{}", crate::crypto::hash_token(&key), ids.join(","));
    let hit = cache
        .lock()
        .await
        .get(&name)
        .filter(|r| r.0.elapsed() < Duration::from_secs(3600))
        .map(|r| r.1.clone());
    let body = if let Some(hit) = hit {
        hit
    } else {
        let mut out = serde_json::Map::new();
        for id in &ids {
            out.insert(id.clone(), Value::Null);
        }
        if !ids.is_empty() {
            let client = crate::steam::SteamClient::new(key)
                .map_err(|_| ApiError::bad("Steam 客户端配置不可用。"))?;
            let r = client.summaries(&ids).await.map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "steam_unreachable",
                    "Steam 未返回有效档案。",
                )
            })?;
            for p in r["response"]["players"].as_array().into_iter().flatten() {
                if let Some(id) = p["steamid"].as_str().filter(|id| out.contains_key(*id)) {
                    out.insert(id.into(),json!({"name":p["personaname"].as_str().unwrap_or(""),"avatar":p["avatarmedium"].as_str().or(p["avatar"].as_str()).unwrap_or("")}));
                }
            }
        }
        let body = Value::Object(out);
        let mut c = cache.lock().await;
        c.retain(|_, r| r.0.elapsed() < Duration::from_secs(3600));
        if c.len() >= 500 {
            if let Some(k) = c.iter().min_by_key(|(_, r)| r.0).map(|(k, _)| k.clone()) {
                c.remove(&k);
            }
        }
        c.insert(name, (Instant::now(), body.clone()));
        body
    };
    let mut response = Json(body).into_response();
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("private, max-age=3600"),
    );
    Ok(response)
}
pub async fn purge(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiJson(input): ApiJson<Value>,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::POST).await?;
    let s = auth::server_scope(&state, &a, &id, "server.view").await?;
    if !s.manager || a.key.is_some() {
        return Err(ApiError::forbidden());
    }
    if input["name"].as_str().is_none_or(|n| n.trim() != s.name) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "name_mismatch",
            "Type the server's name to confirm the purge.",
        ));
    }
    let mut tx = state.db.begin().await?;
    let mut counts = json!({});
    for (table, key) in [
        ("kills", "kills"),
        ("match_players", "matchPlayers"),
        ("matches", "matches"),
    ] {
        let sql = format!(
            "WITH d AS(DELETE FROM {table} WHERE server_id=$1 RETURNING 1)SELECT count(*)FROM d"
        );
        counts[key] = json!(
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(&id)
                .fetch_one(&mut *tx)
                .await?
        )
    }
    crate::audit::event(
        &mut tx,
        &a,
        Some(&s.org_id),
        &h,
        "server",
        "server.stats.purge",
        &s.name,
        counts.clone(),
    )
    .await?;
    super::servers::observe(&mut tx, &id, false).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"counts":counts})))
}
