//! Per-process events and worker ownership. Events are hints; the database is authoritative.
use crate::{config::AppState, error::ApiError, leadership::Leadership};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub struct Runtime {
    pub leader: OnceLock<Arc<Leadership>>,
    pub stop: CancellationToken,
    pub events: broadcast::Sender<Value>,
    interest: Mutex<HashMap<String, Instant>>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            leader: OnceLock::new(),
            stop: CancellationToken::new(),
            events: broadcast::channel(4096).0,
            interest: Mutex::new(HashMap::new()),
        }
    }
}
pub fn lost() -> ApiError {
    ApiError::new(
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "worker_ownership_lost",
        "Worker no longer owns the database.",
    )
}
impl Runtime {
    pub fn interest(&self, ids: &[String], lease_ms: u64) {
        let now = Instant::now();
        let mut map = self.interest.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, until| *until > now);
        for id in ids {
            map.insert(id.clone(), now + Duration::from_millis(lease_ms));
        }
    }
    pub fn watched(&self, id: &str) -> bool {
        self.interest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .is_some_and(|until| *until > Instant::now())
    }
    pub fn emit(&self, event: Value) {
        let _ = self.events.send(event);
    }
    pub async fn check(&self) -> crate::error::Result<()> {
        if self.stop.is_cancelled() {
            return Err(lost());
        }
        if let Some(leader) = self.leader.get() {
            if !leader.valid().await? {
                return Err(lost());
            }
        }
        Ok(())
    }
}
/// A single listener per API process fans notifications out to bounded browser queues.
pub async fn listen(state: AppState) -> anyhow::Result<()> {
    let mut listener = loop {
        if state.runtime.stop.is_cancelled() {
            return Ok(());
        }
        let connection = async {
            let mut listener = sqlx::postgres::PgListener::connect_with(&state.db).await?;
            listener.listen("warcon_events").await?;
            Ok::<_, sqlx::Error>(listener)
        }
        .await;
        match connection {
            Ok(listener) => break listener,
            Err(_) => {
                tracing::warn!("Realtime database listener unavailable; reconnecting");
                state.runtime.emit(json!({"type":"resync"}));
                tokio::select! { _=state.runtime.stop.cancelled()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(1))=>{} }
            }
        }
    };
    loop {
        tokio::select! {
            _=state.runtime.stop.cancelled()=>break,
            note=listener.recv()=>{
                let note=match note { Ok(note)=>note, Err(_)=>{
                    state.runtime.emit(json!({"type":"resync"}));
                    tokio::select! { _=state.runtime.stop.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(1))=>{} }
                    continue;
                }};
                let Ok(mut event)=serde_json::from_str::<Value>(note.payload()) else { continue };
                match event["type"].as_str() {
                    Some("live")=>if let Some(id)=event["serverId"].as_str(){
                        if let Ok(rows)=crate::live::read(&state,&[id.to_owned()]).await {
                            if let Some(live)=rows.get(id){ event=json!({"type":"live","live":live}); }else{continue}
                        }else{continue}
                    },
                    Some("kills")=>{
                        let Some(id)=event["serverId"].as_str() else {continue};
                        let ids=event["eventIds"].as_array().map(|a|a.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default();
                        let Ok(rows)=crate::live::kills(&state,id,&ids,event["ts"].as_str()).await else{continue};
                        event=json!({"type":"kills","serverId":id,"kills":rows});
                    },
                    Some("outbox")=>{ if event["serverId"].as_str().is_none(){continue} },
                    _=>continue,
                }
                state.runtime.emit(event);
            }
        }
    }
    Ok(())
}
