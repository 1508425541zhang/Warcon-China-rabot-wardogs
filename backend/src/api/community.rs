use crate::{
    auth,
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, ApiQuery},
    qq_community as c, qq_config,
    webhooks::text,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method},
};
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize, Default)]
pub struct Query {
    pub view: Option<String>,
    pub page: Option<String>,
    #[serde(rename = "steamId")]
    pub steam_id: Option<String>,
    pub target: Option<String>,
}
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiQuery(q): ApiQuery<Query>,
) -> Result<Json<Value>> {
    let view = q.view.as_deref().unwrap_or("player");
    if ![
        "player", "vote", "economy", "status", "players", "maps", "vip", "battle",
    ]
    .contains(&view)
    {
        return Err(ApiError::bad("Unknown query view."));
    }
    let a = auth::authenticate(&state, &h, &Method::GET).await?;
    auth::server_scope(
        &state,
        &a,
        &id,
        if ["player", "status", "players", "maps", "vip", "battle"].contains(&view) {
            "server.view"
        } else {
            "automation.manage"
        },
    )
    .await?;
    let mut v = if ["status", "players", "maps", "battle"].contains(&view) {
        let raw = q.page.as_deref().unwrap_or("1");
        let page = raw
            .parse::<usize>()
            .ok()
            .filter(|n| *n >= 1 && *n <= 999 && !raw.starts_with('0'))
            .ok_or_else(|| ApiError::bad("page must be 1–999."))?;
        c::query(&state, &id, view, page, Some(&h)).await?
    } else if view == "vote" {
        json!({"text":c::vote_status(&state,&id).await?})
    } else if view == "player" {
        c::player(&state, &id, q.target.as_deref().unwrap_or("")).await?
    } else {
        let steam = q.steam_id.as_deref().unwrap_or("");
        crate::api::notes::steam_id(steam)?;
        if view == "vip" {
            let mut conn = state.db.acquire().await?;
            let v = qq_config::vips(&mut conn).await?;
            let vip = v["entries"].as_array().and_then(|vs| {
                vs.iter()
                    .find(|v| v["serverId"] == id && v["steamId"] == steam && v["enabled"] == true)
            });
            json!({"steamId":steam,"vip":vip.is_some(),"reserve":vip.is_some_and(|v|v["reserve"]==true),"allowOverkill":vip.is_some_and(|v|v["allowOverkill"]==true),"whitelist":vip.is_some_and(|v|v["whitelist"]==true)})
        } else {
            let ledger:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(l) FROM(SELECT id,delta,reason,created_at FROM qq_ledger WHERE server_id=$1 AND steam_id=$2 ORDER BY created_at DESC LIMIT 100)l").bind(&id).bind(steam).fetch_all(&state.db).await?;
            let orders:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(o) FROM(SELECT id,kind,cost,state,outcome,created_at FROM qq_orders WHERE server_id=$1 AND(steam_id=$2 OR kind='map') ORDER BY created_at DESC LIMIT 30)o").bind(&id).bind(steam).fetch_all(&state.db).await?;
            let deliveries:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(d) FROM qq_deliveries d JOIN qq_orders o ON o.id=d.order_id WHERE o.server_id=$1 AND o.steam_id=$2 ORDER BY o.created_at DESC LIMIT 500").bind(&id).bind(steam).fetch_all(&state.db).await?;
            json!({"wallet":crate::qq_economy::wallet(&state,&id,steam).await?,"ledger":ledger,"orders":orders,"deliveries":deliveries})
        }
    };
    v["ok"] = json!(true);
    Ok(Json(v))
}
pub async fn post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiJson(v): ApiJson<Value>,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::POST).await?;
    let server = auth::server_scope(&state, &a, &id, "automation.manage").await?;
    let config = qq_config::load(&state).await?;
    let p = config.policy(&id).ok_or_else(ApiError::missing)?;
    let action = text(&v["action"]);
    let result = match action {
        "reconcile" => {
            let refund = v["refund"]
                .as_bool()
                .ok_or_else(|| ApiError::bad("orderId and refund are required."))?;
            let order = v["orderId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 1024)
                .ok_or_else(|| ApiError::bad("orderId and refund are required."))?;
            let mut tx = state.db.begin().await?;
            crate::qq_economy::reconcile(&mut tx, &id, order, refund, &a.id).await?;
            crate::audit::event(
                &mut tx,
                &a,
                Some(&server.org_id),
                &h,
                "server",
                "qq.reconcile",
                order,
                json!({"reconciled":true,"refund":refund}),
            )
            .await?;
            tx.commit().await?;
            return Ok(Json(json!({"ok":true,"result":{"reconciled":true}})));
        }
        "openVote" => {
            auth::server_scope(&state, &a, &id, "match.control").await?;
            json!({"voteId":c::open_vote(&state,p,Some(&h)).await?})
        }
        "vote" => {
            auth::server_scope(&state, &a, &id, "match.control").await?;
            let steam = text(&v["steamId"]);
            crate::api::notes::steam_id(steam)?;
            let choice = v["choice"].as_str().map(str::to_owned).unwrap_or_else(|| {
                v["choice"]
                    .as_u64()
                    .map(|n| n.to_string())
                    .unwrap_or_default()
            });
            json!({"message":c::cast_vote(&state,p,steam,&choice).await?})
        }
        "friendly" | "reserve" => {
            auth::server_scope(
                &state,
                &a,
                &id,
                if action == "friendly" {
                    "chat.send"
                } else {
                    "slots.manage"
                },
            )
            .await?;
            let request = text(&v["requestId"]);
            if !(16..=80).contains(&request.len())
                || !request
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                return Err(ApiError::bad(
                    "requestId must contain 16–80 letters, digits, _ or -.",
                ));
            }
            let steam = text(&v["steamId"]);
            crate::api::notes::steam_id(steam)?;
            json!({"orderId":c::purchase(&state,p,steam,&format!("api:{id}:{steam}:{request}"),action,text(&v["message"]),Some(&h)).await?})
        }
        _ => return Err(ApiError::bad("Unknown community action.")),
    };
    let mut tx = state.db.begin().await?;
    crate::audit::event(
        &mut tx,
        &a,
        Some(&server.org_id),
        &h,
        "server",
        &format!("qq.{action}"),
        v["orderId"]
            .as_str()
            .or(v["steamId"].as_str())
            .unwrap_or(&id),
        result.clone(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"result":result})))
}
