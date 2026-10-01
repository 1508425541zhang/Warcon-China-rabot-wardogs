use crate::{
    auth,
    config::AppState,
    error::{ApiError, Result},
    http::ApiQuery,
    live,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, Method},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    time::Duration,
};
async fn allowed(
    state: &AppState,
    headers: &HeaderMap,
    asked: &[String],
) -> Result<(Vec<String>, HashSet<String>)> {
    let actor = auth::authenticate(state, headers, &Method::GET).await?;
    let rows = super::servers::accessible(state, &actor, None).await?;
    let all = rows
        .iter()
        .filter_map(|r| r["id"].as_str().map(str::to_owned))
        .collect::<HashSet<_>>();
    let ids = if asked.is_empty() {
        all.iter().cloned().collect::<Vec<_>>()
    } else {
        asked
            .iter()
            .filter(|id| all.contains(*id))
            .cloned()
            .collect()
    };
    let mut automation = HashSet::new();
    for id in &ids {
        if auth::server_scope(state, &actor, id, "automation.manage")
            .await
            .is_ok()
        {
            automation.insert(id.clone());
        }
    }
    Ok((ids, automation))
}
fn asked(q: &HashMap<String, String>) -> Result<Vec<String>> {
    let raw = q.get("ids").map(String::as_str).unwrap_or("");
    if raw.len() > 128000 {
        return Err(ApiError::bad("Too many servers."));
    }
    let ids = raw
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if ids.len() > 2000 || ids.iter().any(|s| s.len() > 64) {
        return Err(ApiError::bad("Too many servers."));
    }
    Ok(ids)
}
pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiQuery(q): ApiQuery<HashMap<String, String>>,
) -> Result<Json<Value>> {
    let (ids, _) = allowed(&state, &headers, &asked(&q)?).await?;
    Ok(Json(
        json!({"ok":true,"live":live::read(&state,&ids).await?}),
    ))
}
pub async fn interest(state: &AppState, ids: &[String]) -> Result<()> {
    for part in ids.chunks(60) {
        sqlx::query("SELECT pg_notify('warcon_observe',$1)")
            .bind(json!({"interest":part}).to_string())
            .execute(&state.db)
            .await?;
    }
    Ok(())
}
pub async fn summary(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::authenticate(&state, &h, &Method::GET).await?;
    let s = auth::server_scope(&state, &a, &id, "server.view").await?;
    let role = super::servers::accessible(&state, &a, Some(&s.org_id))
        .await?
        .into_iter()
        .find(|v| v["id"] == id)
        .map(|v| v["roleName"].clone())
        .unwrap_or(Value::Null);
    let mut rows = live::read(&state, std::slice::from_ref(&id)).await?;
    if !rows.contains_key(&id) {
        interest(&state, std::slice::from_ref(&id)).await?;
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            rows = live::read(&state, std::slice::from_ref(&id)).await?;
            if rows.contains_key(&id) {
                break;
            }
        }
    }
    let Some(l) = rows.get(&id) else {
        return Ok(Json(
            json!({"ok":false,"role":role,"live":null,"error":{"message":"Not observed yet."}}),
        ));
    };
    let mut out = json!({"ok":l["ok"],"role":role,"caps":s.caps,"live":l,"status":l["status"]});
    if l["ok"] != true {
        out["error"] = json!({"message":l["error"]})
    }
    Ok(Json(out))
}
pub async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiQuery(q): ApiQuery<HashMap<String, String>>,
) -> Result<Response> {
    let (ids, mut automation) = allowed(&state, &headers, &asked(&q)?).await?;
    if ids.is_empty() {
        return Err(ApiError::bad("No visible servers were selected."));
    }
    if state.runtime.stop.is_cancelled() {
        return Err(crate::runtime::lost());
    }
    let mut subscribed = state.runtime.events.subscribe();
    let initial = live::read(&state, &ids).await?;
    interest(&state, &ids).await?;
    // Slow clients disconnect and resnapshot on reconnect instead of growing a buffer.
    let (send, recv) = tokio::sync::mpsc::channel::<Value>(32);
    let stop = state.runtime.stop.clone();
    tokio::spawn(async move {
        let selected = ids.iter().cloned().collect::<HashSet<_>>();
        for snapshot in initial.into_values() {
            if send
                .send(json!({"type":"live","live":snapshot}))
                .await
                .is_err()
            {
                return;
            }
        }
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tick.tick().await;
        let deadline = tokio::time::sleep(Duration::from_secs(300));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _=stop.cancelled()=>break,
                _=send.closed()=>break,
                _=&mut deadline=>break,
                _=tick.tick()=>{
                    // Revoke subscriptions when a session, key, role or organisation changes.
                    let Ok((current,next))=allowed(&state,&headers,&ids).await else{break};
                    if current.iter().cloned().collect::<HashSet<_>>()!=selected{break}
                    automation=next;
                    if interest(&state,&ids).await.is_err(){break}
                },
                event=subscribed.recv()=>{
                    let Ok(event)=event else{break};
                    if event["type"]=="resync"{
                        let Ok(rows)=live::read(&state,&ids).await else{break};
                        for live in rows.into_values(){if send.try_send(json!({"type":"live","live":live})).is_err(){return}}
                        continue;
                    }
                    let kind=event["type"].as_str().unwrap_or("");
                    let id=if kind=="live"{event["live"]["serverId"].as_str()}else{event["serverId"].as_str()};
                    let Some(id)=id.filter(|id|selected.contains(*id)) else{continue};
                    if kind=="outbox"&&!automation.contains(id){continue}
                    if !["live","kills","outbox"].contains(&kind){continue}
                    if send.try_send(event).is_err(){break}
                }
            }
        }
    });
    let stream = futures_util::stream::unfold(recv, |mut recv| async move {
        recv.recv().await.map(|v| {
            let kind = v["type"].as_str().unwrap_or("message");
            let data = if kind == "live" {
                v["live"].clone()
            } else {
                v.clone()
            };
            (
                Ok::<_, Infallible>(Event::default().event(kind).data(data.to_string())),
                recv,
            )
        })
    });
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("ping"),
        )
        .into_response();
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("no-cache, no-transform"),
    );
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}
