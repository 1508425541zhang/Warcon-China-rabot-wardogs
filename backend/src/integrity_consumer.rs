//! Ordered Integrity feed lane with persisted source identity and reconciled roster brackets.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    feed_jobs::{self, Consumer, Job},
    integrity_enforcement::date,
    integrity_pipeline::Pipeline,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
pub async fn batch(state: &AppState, job: &Job) -> Result<Vec<Value>> {
    let ids = job
        .event_ids
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 128 && a.iter().all(Value::is_string))
        .ok_or_else(|| ApiError::bad("Invalid persisted feed job identity."))?;
    let ids: Vec<_> = ids.iter().map(|v| v.as_str().unwrap().to_owned()).collect();
    if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(ApiError::bad("Duplicate persisted feed job identity."));
    }
    let mut tx = state.worker_transaction().await?;
    let valid:Option<i64>=sqlx::query_scalar("SELECT id FROM feed_processing_jobs WHERE id=$1 AND attempts=$2 AND lease_until=$3 AND lease_until>clock_timestamp() AND state='processing' FOR UPDATE").bind(job.id).bind(job.attempts).bind(job.lease_until).fetch_optional(&mut *tx).await?;
    if valid.is_none() {
        return Err(ApiError::bad("Feed job lease lost."));
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
        return Err(ApiError::bad("Feed job source events are missing."));
    }
    let live: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=$1")
            .bind(&job.server_id)
            .fetch_optional(&mut *tx)
            .await?;
    let live = live.unwrap_or(Value::Null);
    let after = date(&live["players_at"]);
    let observed =
        after.is_some_and(|at| at > job.kill_ts && (at - job.kill_ts).num_milliseconds() <= 60000);
    let mut by_id = HashMap::new();
    for row in rows {
        let id = row["event_id"].as_str().unwrap().to_owned();
        let reconciled = if row["faction_bracketed"] == true {
            row
        } else {
            let teams = crate::feed::roster(
                &live["players"],
                after,
                &live["status"],
                date(&live["status_at"]),
                row["map"].as_str().unwrap_or(""),
            );
            let same = observed
                && date(&row["faction_observed_at"]).is_some_and(|at| {
                    at <= job.kill_ts && (job.kill_ts - at).num_milliseconds() <= 10000
                })
                && row["killer_faction"].as_str().is_some_and(|f| {
                    !f.is_empty()
                        && teams.as_ref().is_some_and(|t| {
                            t.get(row["killer_steam_id"].as_str().unwrap_or(""))
                                .is_some_and(|v| v == f)
                        })
                })
                && row["victim_faction"].as_str().is_some_and(|f| {
                    !f.is_empty()
                        && teams.as_ref().is_some_and(|t| {
                            t.get(row["victim_steam_id"].as_str().unwrap_or(""))
                                .is_some_and(|v| v == f)
                        })
                });
            sqlx::query_scalar("UPDATE kills SET faction_bracketed=true,killer_faction=CASE WHEN $4 THEN killer_faction ELSE NULL END,victim_faction=CASE WHEN $4 THEN victim_faction ELSE NULL END,team_kill=CASE WHEN $4 THEN team_kill ELSE false END WHERE server_id=$1 AND ts=$2 AND event_id=$3 RETURNING to_jsonb(kills)").bind(&job.server_id).bind(job.kill_ts).bind(&id).bind(same).fetch_one(&mut *tx).await?
        };
        by_id.insert(id, crate::api::kills::view(reconciled));
    }
    tx.commit().await?;
    Ok(ids.iter().map(|id| by_id.remove(id).unwrap()).collect())
}
pub async fn next(state: &AppState, pipeline: &Pipeline) -> Result<bool> {
    let Some(leader) = state.runtime.leader.get() else {
        return Err(crate::runtime::lost());
    };
    let job = feed_jobs::claim(leader, Consumer::Integrity, None)
        .await
        .map_err(|_| ApiError::bad("Feed claim failed."))?;
    let Some(job) = job else { return Ok(false) };
    let result = async {
        let kills = batch(state, &job).await?;
        let safe = feed_jobs::backlog_safe(leader, Consumer::Integrity)
            .await
            .map_err(|_| ApiError::bad("Feed health check failed."))?;
        let allow = safe && (chrono::Utc::now() - job.created_at).num_milliseconds() <= 300000;
        pipeline
            .process(state, &job.server_id, &kills, allow)
            .await?;
        Ok::<_, ApiError>(())
    }
    .await;
    if result
        .as_ref()
        .is_err_and(|e| e.code == "worker_ownership_lost")
    {
        return Err(crate::runtime::lost());
    }
    feed_jobs::finish(
        leader,
        &job,
        result.err().map(|_| "integrity_consumer_failed"),
    )
    .await
    .map_err(|_| ApiError::bad("Feed job completion failed."))?;
    Ok(true)
}
pub async fn run(state: AppState) -> Result<()> {
    let pipeline = Arc::new(Pipeline::default());
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let state = state.clone();
        let pipeline = pipeline.clone();
        tasks.spawn(async move{let mut tick=tokio::time::interval(Duration::from_secs(1));tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);loop{tokio::select!{_=state.runtime.stop.cancelled()=>break,_=tick.tick()=>{state.runtime.check().await?;for _ in 0..20{match next(&state,&pipeline).await{Ok(true)=>{},Ok(false)=>break,Err(e)if e.code=="worker_ownership_lost"=>return Err(e),Err(_)=>{tracing::warn!("Integrity feed pass failed");break}}}}}}Ok::<_,ApiError>(())});
    }
    if let Some(result) = tasks.join_next().await {
        state.runtime.stop.cancel();
        while tasks.join_next().await.is_some() {}
        return result.map_err(|_| crate::runtime::lost())?;
    }
    Ok(())
}
