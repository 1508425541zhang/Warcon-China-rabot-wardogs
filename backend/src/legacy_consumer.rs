//! Independent ordered legacy Feed lane. Integrity failures cannot block these rules.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    feed_jobs::{self, Consumer, Job},
    trigger_policy as p,
    trigger_store::{self, Intent},
};
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
#[derive(Clone, Default)]
struct Memory {
    rates: HashMap<String, HashMap<String, p::Track>>,
    seen: HashMap<String, i64>,
}
#[derive(Default)]
pub struct Engine {
    servers: Mutex<HashMap<String, Arc<Mutex<Memory>>>>,
}
pub async fn batch(state: &AppState, job: &Job) -> Result<Vec<Value>> {
    let ids = job
        .event_ids
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 128 && v.iter().all(Value::is_string))
        .ok_or_else(|| ApiError::bad("Invalid legacy job identity."))?;
    let ids: Vec<_> = ids.iter().map(|v| v.as_str().unwrap().to_owned()).collect();
    if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(ApiError::bad("Duplicate legacy source events."));
    }
    let mut tx = state.worker_transaction().await?;
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM feed_processing_jobs WHERE id=$1 AND attempts=$2 AND lease_until=$3 AND state='processing' AND lease_until>now() AND consumer='legacy')").bind(job.id).bind(job.attempts).bind(job.lease_until).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(ApiError::bad("Legacy feed lease lost."));
    }
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND ts=$2 AND event_id=ANY($3)",
    )
    .bind(&job.server_id)
    .bind(job.kill_ts)
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    if rows.len() != ids.len() {
        return Err(ApiError::bad("Legacy feed source events missing."));
    }
    tx.commit().await?;
    let mut indexed: HashMap<String, Value> = rows
        .into_iter()
        .map(crate::api::kills::view)
        .map(|v| (v["eventId"].as_str().unwrap().into(), v))
        .collect();
    Ok(ids.iter().map(|id| indexed.remove(id).unwrap()).collect())
}
pub fn counts(k: &Value) -> bool {
    k["killer"]["steamId"].is_string()
        && k["suicide"] != true
        && k["cause"].as_str().is_some_and(|c| {
            !c.is_empty()
                && !c.to_ascii_lowercase().starts_with("vehicle.")
                && !c
                    .to_ascii_lowercase()
                    .starts_with("id.vehicle.weaponextension.")
                && !c.to_ascii_lowercase().starts_with("id.buildable.")
        })
}
impl Engine {
    pub async fn process(
        &self,
        state: &AppState,
        server: &str,
        kills: &[Value],
        allow: bool,
    ) -> Result<()> {
        if kills.is_empty() {
            return Ok(());
        }
        state
            .runtime
            .emit(json!({"type":"kills","serverId":server,"kills":kills}));
        if !allow {
            return Ok(());
        }
        let lock = self
            .servers
            .lock()
            .await
            .entry(server.into())
            .or_default()
            .clone();
        let mut memory = lock.lock().await;
        let mut next = memory.clone();
        let now = Utc::now();
        let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(t) FROM triggers t WHERE server_id=$1 AND enabled AND kind IN('kill_rate','team_kill')").bind(server).fetch_all(&state.db).await?;
        let enabled: HashSet<_> = rows
            .iter()
            .filter_map(|r| r["id"].as_str())
            .map(str::to_owned)
            .collect();
        next.rates.retain(|id, _| enabled.contains(id));
        let received = crate::integrity_enforcement::date(&kills[0]["ts"])
            .unwrap_or(now)
            .timestamp_millis();
        let latest = kills
            .iter()
            .filter_map(|k| k["eventTime"].as_f64())
            .fold(f64::NEG_INFINITY, f64::max);
        let mut counted: Vec<_> = kills
            .iter()
            .filter(|k| counts(k))
            .map(|k| {
                (
                    k,
                    received
                        - ((latest - k["eventTime"].as_f64().unwrap_or(latest)).max(0.) * 1000.)
                            as i64,
                )
            })
            .collect();
        counted.sort_by_key(|(_, at)| *at);
        let mut tx = state.worker_transaction().await?;
        let server_row:Option<(String,String)>=sqlx::query_as("SELECT s.org_id,s.name FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1 AND o.suspended_at IS NULL").bind(server).fetch_optional(&mut *tx).await?;
        let Some((org, name)) = server_row else {
            return Ok(());
        };
        for row in rows.iter().filter(|r| r["kind"] == "kill_rate") {
            let id = row["id"].as_str().unwrap();
            let cfg = &row["config"];
            let tracks = next.rates.entry(id.into()).or_default();
            let mut intents = vec![];
            for (k, at) in &counted {
                let steam = k["killer"]["steamId"].as_str().unwrap();
                let key = format!("{id}:{}:{}", k["instanceId"], k["eventId"]);
                if next.seen.contains_key(&key) {
                    continue;
                }
                next.seen.insert(key, *at);
                if let Some(verdict) = p::rate_step(
                    cfg,
                    tracks.entry(steam.into()).or_default(),
                    *at,
                    k["headshot"] == true,
                ) {
                    intents.push(Intent {
                        action: "kill_rate_flag".into(),
                        params: json!({}),
                        target: steam.into(),
                        detail: json!({"name":k["killer"]["name"],"verdict":verdict}),
                        steam: Some(steam.into()),
                        key: format!("{id}:{steam}:{}", k["eventId"].as_str().unwrap_or("")),
                        ok: format!("Flagged {steam}: {verdict}"),
                    })
                }
            }
            let from = now.timestamp_millis() - cfg["windowMinutes"].as_i64().unwrap_or(5) * 60000;
            let cooled =
                now.timestamp_millis() - cfg["cooldownMinutes"].as_i64().unwrap_or(30) * 60000;
            tracks.retain(|_, t| {
                t.at.last().is_some_and(|at| *at > from) || t.flagged.is_some_and(|at| at > cooled)
            });
            trigger_store::enqueue(&mut tx, server, row, &intents, None, now).await?;
        }
        let by_killer: HashMap<_, _> = kills
            .iter()
            .filter(|k| k["teamKill"] == true)
            .filter_map(|k| k["killer"]["steamId"].as_str().map(|s| (s, k)))
            .collect();
        for (steam, k) in &by_killer {
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM kills WHERE server_id=$1 AND killer_steam_id=$2 AND team_kill AND ts>=COALESCE((SELECT joined_at FROM player_sessions WHERE server_id=$1 AND steam_id=$2 AND left_at IS NULL ORDER BY id DESC LIMIT 1),now()-interval '1 hour')").bind(server).bind(steam).fetch_one(&mut *tx).await?;
            let vars = json!({"name":k["killer"]["name"],"victim":k["victim"]["name"],"count":count,"server":name,"map":k["map"]});
            for row in rows.iter().filter(|r| r["kind"] == "team_kill") {
                let cfg = &row["config"];
                if let Some(stage) = p::team_stage(cfg, count) {
                    let kick = stage == "kick";
                    let text = p::render(
                        if kick {
                            cfg["kickReason"].as_str()
                        } else {
                            cfg["warnMessage"].as_str()
                        }
                        .unwrap_or(""),
                        &vars,
                    );
                    let i = Intent {
                        action: if kick { "kick" } else { "whisper" }.into(),
                        params: if kick {
                            json!({"steamId":steam,"reason":text})
                        } else {
                            json!({"steamId":steam,"message":text})
                        },
                        target: (*steam).into(),
                        detail: json!({"name":k["killer"]["name"],"victim":k["victim"]["name"],"count":count,"eventId":k["eventId"]}),
                        steam: Some((*steam).into()),
                        key: format!(
                            "{}:{steam}:{}",
                            row["id"].as_str().unwrap(),
                            k["eventId"].as_str().unwrap_or("")
                        ),
                        ok: format!("{stage} {steam} ({count})"),
                    };
                    trigger_store::enqueue(&mut tx, server, row, &[i], None, now).await?;
                }
            }
        }
        for k in kills.iter().filter(|k| k["teamKill"] == true) {
            let key = format!("teamkill:{server}:{}:{}", k["instanceId"], k["eventId"]);
            let embed = json!({"title":"队杀事件","color":15158332,"description":format!("{} 击杀同阵营玩家 {}",k["killer"]["name"].as_str().unwrap_or(""),k["victim"]["name"].as_str().unwrap_or("")),"fields":[{"name":"服务器","value":name},{"name":"来源","value":k["cause"].as_str().unwrap_or("—")}],"timestamp":k["ts"]});
            crate::webhooks::input(&mut tx, &key, &org, server, "teamkills", &embed).await?;
        }
        next.seen
            .retain(|_, at| now.timestamp_millis() - *at < 86400000);
        tx.commit().await?;
        *memory = next;
        if let Err(error) = crate::skill_balance::deaths(state, server, kills).await {
            state.runtime.check().await?;
            tracing::warn!(%server,%error,"skill balance death pass failed");
        }
        Ok(())
    }
}
pub async fn next(state: &AppState, engine: &Engine) -> Result<bool> {
    let leader = state
        .runtime
        .leader
        .get()
        .ok_or_else(crate::runtime::lost)?;
    let Some(job) = feed_jobs::claim(leader, Consumer::Legacy, None)
        .await
        .map_err(|_| ApiError::bad("Legacy claim failed."))?
    else {
        return Ok(false);
    };
    let result = async {
        let kills = batch(state, &job).await?;
        let safe = feed_jobs::backlog_safe(leader, Consumer::Legacy)
            .await
            .map_err(|_| ApiError::bad("Legacy feed health failed."))?;
        engine
            .process(
                state,
                &job.server_id,
                &kills,
                safe && (Utc::now() - job.created_at).num_milliseconds() <= 300000,
            )
            .await
    }
    .await;
    if result
        .as_ref()
        .is_err_and(|e| e.code == "worker_ownership_lost")
    {
        return Err(crate::runtime::lost());
    }
    feed_jobs::finish(leader, &job, result.err().map(|_| "legacy_consumer_failed"))
        .await
        .map_err(|_| ApiError::bad("Legacy completion failed."))?;
    Ok(true)
}
pub async fn run(state: AppState) -> Result<()> {
    let engine = Arc::new(Engine::default());
    let mut workers = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let state = state.clone();
        let engine = engine.clone();
        workers.spawn(async move{let mut tick=tokio::time::interval(Duration::from_secs(1));tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);loop{tokio::select!{_=state.runtime.stop.cancelled()=>break,_=tick.tick()=>{state.runtime.check().await?;for _ in 0..20{match next(&state,&engine).await{Ok(true)=>{},Ok(false)=>break,Err(e)if e.code=="worker_ownership_lost"=>return Err(e),Err(_)=>{tracing::warn!("Legacy feed pass failed");break}}}}}}Ok::<_,ApiError>(())});
    }
    if let Some(result) = workers.join_next().await {
        state.runtime.stop.cancel();
        while workers.join_next().await.is_some() {}
        return result.map_err(|_| crate::runtime::lost())?;
    }
    Ok(())
}
