//! Official QQ gateway, native TLS, bounded frames, reconnect and resumable sessions.
use crate::{
    config::AppState,
    qq_config::{self, Configuration},
    qq_transport,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};
fn fingerprint(c: &Configuration) -> String {
    crate::crypto::hash_token(&format!(
        "{}:{}:{}",
        c.stored.provider, c.stored.self_id, c.secret
    ))
}
pub fn valid_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        u.scheme() == "wss"
            && ["api.bot.qq.com", "api.sgroup.qq.com"].contains(&u.host_str().unwrap_or(""))
            && u.port_or_known_default() == Some(443)
            && u.username().is_empty()
            && u.password().is_none()
            && u.fragment().is_none()
    })
}
#[derive(Default)]
struct Session {
    sequence: Option<i64>,
    id: String,
    owner: String,
    attempt: u32,
}
async fn connect(state: &AppState, c: &Configuration, session: &mut Session) -> anyhow::Result<()> {
    let value = qq_transport::official(c, "/gateway", None).await?;
    let url = value["url"]
        .as_str()
        .filter(|s| valid_url(s))
        .ok_or_else(|| anyhow::anyhow!("Invalid official gateway address"))?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(65536))
        .max_frame_size(Some(65536));
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(20),
        tokio_tungstenite::connect_async_with_config(url, Some(config), false),
    )
    .await??;
    let (mut sink, mut stream) = socket.split();
    let mut interval: Option<tokio::time::Interval> = None;
    let mut acknowledged = true;
    let mut settings = tokio::time::interval(Duration::from_secs(1));
    settings.tick().await;
    let first = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        tokio::select! {
            _=state.runtime.stop.cancelled()=>break,
            _=tokio::time::sleep_until(first),if interval.is_none()=>anyhow::bail!("Gateway hello timed out"),
            _=settings.tick()=>{
                state.runtime.check().await?;let fresh=qq_config::load(state).await?;
                if !fresh.active()||fresh.stored.provider!="official"||fingerprint(&fresh)!=session.owner{session.id.clear();session.sequence=None;break}
            },
            _=async{match interval.as_mut(){Some(t)=>t.tick().await,None=>std::future::pending().await}},if interval.is_some()=>{
                anyhow::ensure!(acknowledged,"Gateway heartbeat not acknowledged");acknowledged=false;
                sink.send(Message::Text(json!({"op":1,"d":session.sequence}).to_string().into())).await?;
            },
            message=stream.next()=>{
                let Some(message)=message else{break};let message=message?;
                let raw=match message{Message::Text(v)=>v.to_string(),Message::Ping(v)=>{sink.send(Message::Pong(v)).await?;continue},Message::Pong(_)=>continue,Message::Close(_)=>break,_=>anyhow::bail!("Unsupported official frame")};
                let v:Value=serde_json::from_str(&raw)?;if let Some(n)=v["s"].as_i64(){session.sequence=Some(n)}
                match v["op"].as_i64(){
                    Some(10)=>{
                        anyhow::ensure!(interval.is_none(),"Duplicate gateway hello");let ms=v["d"]["heartbeat_interval"].as_u64().filter(|n|(1000..=120000).contains(n)).ok_or_else(||anyhow::anyhow!("Invalid heartbeat interval"))?;
                        let mut clock=tokio::time::interval(Duration::from_millis(ms));clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);clock.tick().await;interval=Some(clock);
                        let token=qq_transport::token(c).await?;
                        let auth=if !session.id.is_empty(){json!({"op":6,"d":{"token":format!("QQBot {token}"),"session_id":session.id,"seq":session.sequence}})}else{json!({"op":2,"d":{"token":format!("QQBot {token}"),"intents":1<<25,"shard":[0,1],"properties":{"$os":"linux","$browser":"warcon","$device":"warcon"}}})};
                        sink.send(Message::Text(auth.to_string().into())).await?;
                    },Some(11)=>acknowledged=true,
                    Some(7)=>break,Some(9)=>{if v["d"]!=true{session.id.clear();session.sequence=None;}break},
                    Some(0)=>{
                        if v["t"]=="READY"{session.id=v["d"]["session_id"].as_str().filter(|s|s.len()<=512).ok_or_else(||anyhow::anyhow!("Invalid gateway session"))?.into();session.attempt=0;tracing::info!("Official QQ gateway ready");}
                        if let Some(m)=crate::qq_protocol::official(&v,&c.stored.self_id,chrono::Utc::now().timestamp_millis()){
                            let fresh=qq_config::load(state).await?;if fingerprint(&fresh)==session.owner{crate::qq_protocol::accept(state,&fresh,&m).await?}
                        }
                    },_=>{}
                }
            }
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), sink.close()).await;
    Ok(())
}
pub async fn run(state: AppState) -> anyhow::Result<()> {
    let mut session = Session::default();
    loop {
        state.runtime.check().await?;
        let c = qq_config::load(&state).await?;
        if !c.active() || c.stored.provider != "official" {
            session = Session::default();
            tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(1))=>{}}
            continue;
        }
        let owner = fingerprint(&c);
        if session.owner != owner {
            session = Session {
                owner,
                ..Default::default()
            }
        }
        tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),result=connect(&state,&c,&mut session)=>{if result.is_err(){tracing::warn!("Official QQ gateway disconnected; reconnecting");}}}
        session.attempt = (session.attempt + 1).min(7);
        let delay = Duration::from_secs((1u64 << session.attempt.saturating_sub(1)).min(60));
        let until = tokio::time::Instant::now() + delay;
        loop {
            tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),_=tokio::time::sleep_until(until)=>break,_=tokio::time::sleep(Duration::from_secs(1))=>{let fresh=qq_config::load(&state).await?;if !fresh.active()||fingerprint(&fresh)!=session.owner{break}}}
        }
    }
}
