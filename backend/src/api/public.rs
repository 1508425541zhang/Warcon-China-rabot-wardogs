use crate::{
    auth,
    config::AppState,
    error::{ApiError, Result},
    http::{Peer, client_address},
    leaderboards,
};
use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, Method},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::collections::HashMap;
fn cache(value: Value, seconds: u32) -> Response {
    let mut r = Json(value).into_response();
    r.headers_mut().insert(
        "cache-control",
        HeaderValue::from_str(&format!("public, max-age={seconds}")).unwrap(),
    );
    r
}
pub fn limit(state: &AppState, peer: Peer, headers: &HeaderMap) -> Result<()> {
    crate::ratelimit::allow(
        format!(
            "public:{}",
            client_address(state, peer, headers)
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "unknown".into())
        ),
        120,
        true,
    )
}
pub async fn require(state: &AppState, id: &str, feature: &str) -> Result<Value> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(ApiError::missing());
    }
    let row:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'name',s.name,'orgId',o.id,'orgName',o.name,'discordInviteUrl',o.discord_invite_url,'publicKills',s.public_kills,'features',jsonb_build_object('status',s.public_status AND o.allow_public_status,'leaderboards',s.public_leaderboards AND o.allow_public_leaderboards)) FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1 AND o.suspended_at IS NULL").bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    if row["features"][feature] != true {
        return Err(ApiError::missing());
    }
    Ok(row)
}
async fn org_servers(state: &AppState, org: &str) -> Result<Vec<(String, String)>> {
    Ok(sqlx::query_as("SELECT s.id,s.name FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.org_id=$1 AND s.public_leaderboards AND o.allow_public_leaderboards AND o.suspended_at IS NULL ORDER BY s.sort_order,s.name").bind(org).fetch_all(&state.db).await?)
}
pub fn public_kill(mut k: Value) -> Value {
    if let Some(o) = k.as_object_mut() {
        o.retain(|key, _| {
            [
                "eventId",
                "ts",
                "eventTime",
                "killer",
                "victim",
                "cause",
                "distanceM",
                "headshot",
                "suicide",
                "teamKill",
                "tags",
            ]
            .contains(&key.as_str())
        });
    }
    for side in ["killer", "victim"] {
        if let Some(o) = k[side].as_object_mut() {
            o.retain(|key, _| ["name", "faction"].contains(&key.as_str()));
        }
    }
    k
}
pub async fn status_view(state: &AppState, ps: &Value) -> Result<Value> {
    let id = ps["id"].as_str().unwrap();
    let live: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=$1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let l = live.unwrap_or(Value::Null);
    let s = crate::live::status(&l["status"]);
    let ok = l["ok"] == true && s.is_object();
    let links = ps["features"]["leaderboards"] == true;
    let mut roster = if ok {
        l["players"].as_array().into_iter().flatten().map(|p|{let steam=p["steamId"].as_str().filter(|id|super::notes::steam_id(id).is_ok());json!({"name":crate::http::string(&p["name"],usize::MAX),"faction":p["faction"],"kills":crate::http::js_number(&p["kills"]).unwrap_or(0.),"deaths":crate::http::js_number(&p["deaths"]).unwrap_or(0.),"steamId":if links{steam}else{None}})}).collect::<Vec<_>>()
    } else {
        vec![]
    };
    roster.sort_by(|a, b| {
        b["kills"]
            .as_f64()
            .partial_cmp(&a["kills"].as_f64())
            .unwrap()
            .then_with(|| {
                a["deaths"]
                    .as_f64()
                    .partial_cmp(&b["deaths"].as_f64())
                    .unwrap()
            })
            .then_with(|| a["name"].as_str().cmp(&b["name"].as_str()))
    });
    let kills = if ps["publicKills"] == true {
        let f = super::kills::KillQuery {
            limit: Some(20),
            ..Default::default()
        };
        json!(
            super::kills::load(state, id, &f).await?["kills"]
                .as_array()
                .unwrap()
                .iter()
                .cloned()
                .map(public_kill)
                .collect::<Vec<_>>()
        )
    } else {
        Value::Null
    };
    let scores = if ok {
        s["scores"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| json!({"name":f["name"],"colorHex":f["colorHex"],"score":f["score"]}))
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    Ok(
        json!({"serverId":id,"name":ps["name"],"orgName":ps["orgName"],"ok":ok,"observedAt":l["observed_at"],"joinCode":l["game_server_id"].as_str().filter(|s|!s.is_empty()),"map":s["map"].as_str().filter(|s|!s.is_empty()),"lighting":s["lighting"].as_str().filter(|s|!s.is_empty()),"experiences":s["experiences"].as_array().cloned().unwrap_or_default(),"players":if ok{s["playerCount"].clone()}else{json!(0)},"maxPlayers":if ok{s["maxPlayers"].clone()}else{Value::Null},"reservedSlots":l["reserved_slots"],"scores":scores,"scoreCap":if ok{s["scoreCap"].clone()}else{Value::Null},"matchSeconds":if ok{s["matchSeconds"].clone()}else{Value::Null},"roster":roster,"kills":kills}),
    )
}
pub async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response> {
    limit(&state, peer, &headers)?;
    let ps = require(&state, &id, "status").await?;
    Ok(cache(
        json!({"ok":true,"server":status_view(&state,&ps).await?}),
        5,
    ))
}
pub async fn matches(
    State(state): State<AppState>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response> {
    limit(&state, peer, &headers)?;
    require(&state, &id, "leaderboards").await?;
    let page = url::form_urlencoded::parse(raw.as_deref().unwrap_or("").as_bytes())
        .find(|(k, _)| k == "page")
        .map(|(_, v)| v.into_owned());
    Ok(cache(
        super::matches::list_view(&state, &id, page.as_deref(), 20).await?,
        30,
    ))
}
pub async fn match_detail(
    State(state): State<AppState>,
    Path((id, mid)): Path<(String, String)>,
    RawQuery(raw): RawQuery,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response> {
    limit(&state, peer, &headers)?;
    require(&state, &id, "leaderboards").await?;
    if mid.is_empty() || mid.len() > 18 || !mid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApiError::missing());
    }
    let mid = mid.parse::<i64>().map_err(|_| ApiError::missing())?;
    let mut view = super::matches::load_match(&state.db, &id, mid)
        .await?
        .ok_or_else(ApiError::missing)?;
    let parsed = super::kills::parse_query(raw)?;
    let f = super::kills::KillQuery {
        match_id: Some(mid),
        before: parsed.before,
        before_time: parsed.before_time,
        limit: parsed.limit,
        ..Default::default()
    };
    let kills = super::kills::load(&state, &id, &f).await?;
    let feed = kills["kills"].as_array().unwrap();
    view["feed"] = json!(feed.iter().cloned().map(public_kill).collect::<Vec<_>>());
    view["more"] = json!(feed.len() as i64 == f.limit.unwrap_or(50));
    view["ok"] = json!(true);
    Ok(cache(view, 30))
}
pub async fn board(
    State(state): State<AppState>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response> {
    limit(&state, peer, &headers)?;
    let ps = require(&state, &id, "leaderboards").await?;
    let q = leaderboards::parse(raw.as_deref(), 20);
    let ids = if q.scope == "org" {
        org_servers(&state, ps["orgId"].as_str().unwrap())
            .await?
            .into_iter()
            .map(|s| s.0)
            .collect()
    } else {
        vec![id]
    };
    let mut v = leaderboards::board(&state, &ids, &q).await?;
    v["ok"] = json!(true);
    v["maxPage"] = json!(20);
    Ok(cache(v, 30))
}
pub async fn player(
    State(state): State<AppState>,
    Path((id, steam)): Path<(String, String)>,
    peer: Peer,
    headers: HeaderMap,
) -> Result<Response> {
    limit(&state, peer, &headers)?;
    let ps = require(&state, &id, "leaderboards").await?;
    if super::notes::steam_id(&steam).is_err() {
        return Err(ApiError::missing());
    }
    let servers = org_servers(&state, ps["orgId"].as_str().unwrap()).await?;
    let names: HashMap<_, _> = servers.into_iter().collect();
    let ids = names.keys().cloned().collect::<Vec<_>>();
    let name:Option<String>=sqlx::query_scalar("SELECT name FROM player_sessions WHERE steam_id=$1 AND server_id=ANY($2) ORDER BY last_seen DESC LIMIT 1").bind(&steam).bind(&ids).fetch_optional(&state.db).await?;
    let name = name
        .filter(|n| !n.is_empty())
        .ok_or_else(ApiError::missing)?;
    Ok(cache(
        json!({"ok":true,"player":{"steamId":steam,"name":name,"avatar":sqlx::query_scalar::<_,Option<String>>("SELECT avatar FROM steam_profiles WHERE steam_id=$1").bind(&steam).fetch_optional(&state.db).await?.flatten().unwrap_or_default()},"career":leaderboards::career(&state,&id,&ids,&names,&steam).await?,"combat":crate::player_views::combat(&state,&ids,&names,&steam,false).await?,"multiServer":ids.len()>1}),
        30,
    ))
}
pub async fn private_board(
    State(state): State<AppState>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = auth::authenticate(&state, &headers, &Method::GET).await?;
    let scope = auth::server_scope(&state, &actor, &id, "server.view").await?;
    let q = leaderboards::parse(raw.as_deref(), 100000);
    let ids = if q.scope == "org" {
        super::servers::accessible(&state, &actor, Some(&scope.org_id))
            .await?
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_owned())
            .collect()
    } else {
        vec![id]
    };
    let mut v = leaderboards::board(&state, &ids, &q).await?;
    v["ok"] = json!(true);
    Ok(Json(v))
}
