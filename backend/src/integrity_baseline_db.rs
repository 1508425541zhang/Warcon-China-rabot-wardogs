//! Historical replay uses a read snapshot; only the short publication transaction holds worker fencing.
use crate::{
    committee::{FEATURE_VERSION, MODEL_VERSION},
    config::AppState,
    integrity_baseline::{self, Replay, ReplayRow},
};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;
const PAGE_SQL: &str = r#"
SELECT to_jsonb(events) FROM (
 SELECT 'local' AS source,k.server_id,k.event_id,k.ts AS at,k.instance_id,k.match_id,k.match_row,k.event_time,k.map,k.killer_steam_id,k.victim_steam_id,k.killer_faction,k.victim_faction,k.cause,k.distance_m,k.headshot,(k.tags ? 'Penetration') AS penetration,pop.player_count
 FROM kills k JOIN servers s ON s.id=k.server_id
 JOIN LATERAL (SELECT player_count FROM samples sm WHERE sm.server_id=k.server_id AND sm.ok AND sm.ts<=k.ts AND sm.ts>k.ts-interval '2 minutes' ORDER BY sm.ts DESC LIMIT 1) pop ON true
 WHERE s.org_id=$1 AND k.ts>=$2 AND k.ts<$3 AND k.match_row IS NOT NULL
 AND k.killer_steam_id IS NOT NULL AND k.killer_faction IS NOT NULL AND k.victim_faction IS NOT NULL AND k.faction_bracketed AND k.faction_observed_at IS NOT NULL AND NOT k.team_kill AND NOT k.suicide
 AND EXISTS(SELECT 1 FROM feed_processing_jobs j WHERE j.server_id=k.server_id AND j.consumer='integrity' AND j.kill_ts=k.ts AND j.event_ids ? k.event_id AND j.state='done' AND j.attempts=1 AND (j.done_at<=j.created_at+interval '5 minutes' OR EXISTS(SELECT 1 FROM feed_processing_jobs legacy WHERE legacy.server_id=j.server_id AND legacy.kill_ts=j.kill_ts AND legacy.event_ids ? k.event_id AND legacy.consumer='legacy' AND legacy.state='done' AND legacy.attempts=1 AND legacy.done_at<=legacy.created_at+interval '5 minutes')))
 AND NOT EXISTS(SELECT 1 FROM integrity_case_events ce JOIN integrity_cases c ON c.id=ce.case_id WHERE c.org_id=$1 AND c.steam_id=k.killer_steam_id AND c.server_id=k.server_id AND ce.instance_id=k.instance_id AND ce.event_id=k.event_id)
 UNION ALL
 SELECT 'external' AS source,'external:' || i.source_server AS server_id,i.event_id,i.event_at AS at,i.instance_id,i.match_id,NULL::bigint AS match_row,i.event_time,i.map,i.killer_steam_id,i.victim_steam_id,i.killer_faction,i.victim_faction,i.cause,i.distance_m,i.headshot,i.penetration,i.player_count
 FROM integrity_import_kills i JOIN integrity_import_batches b ON b.id=i.batch_id WHERE i.org_id=$1 AND b.status='APPROVED' AND i.event_at>=$2 AND i.event_at<$3
 ORDER BY source,server_id,at,event_time,event_id,match_id,victim_steam_id LIMIT 10000 OFFSET $4
) events"#;
pub fn refresh<'a>(
    state: &'a AppState,
    org: &'a str,
) -> futures_util::future::BoxFuture<'a, anyhow::Result<usize>> {
    Box::pin(async move {
        let started = std::time::Instant::now();
        let result = replay(state, org, started).await;
        if result.is_err() && state.runtime.check().await.is_ok() {
            let mut tx = state.worker_transaction().await?;
            sqlx::query("INSERT INTO integrity_model_state(org_id,baseline_status,last_failure_at,last_duration_ms) VALUES($1,'STALE',now(),$2) ON CONFLICT(org_id) DO UPDATE SET baseline_status=CASE WHEN integrity_model_state.baseline_status='READY' AND integrity_model_state.active_baseline_generation IS NOT NULL AND integrity_model_state.last_refresh_at>=now()-interval '48 hours' THEN 'READY' ELSE 'STALE' END,last_failure_at=now(),last_duration_ms=excluded.last_duration_ms")
            .bind(org).bind(started.elapsed().as_millis().min(i32::MAX as u128) as i32).execute(&mut *tx).await?;
            tx.commit().await?;
        }
        result
    })
}
async fn replay(state: &AppState, org: &str, started: std::time::Instant) -> anyhow::Result<usize> {
    state.runtime.check().await?;
    let mut read = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *read)
        .await?;
    let locked: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("integrity-baseline:{org}"))
            .fetch_one(&mut *read)
            .await?;
    if !locked {
        return Ok(0);
    }
    let model: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id=$1")
            .bind(org)
            .fetch_optional(&mut *read)
            .await?;
    let model =
        model.unwrap_or_else(|| json!({"weapon_map_version":1,"active_baseline_generation":null}));
    let cases: i64 = sqlx::query_scalar("SELECT count(*) FROM integrity_cases WHERE org_id=$1")
        .bind(org)
        .fetch_one(&mut *read)
        .await?;
    let overrides: HashMap<String, String> =
        sqlx::query("SELECT cause,category FROM integrity_weapon_map WHERE org_id=$1")
            .bind(org)
            .fetch_all(&mut *read)
            .await?
            .into_iter()
            .map(|row| (row.get("cause"), row.get("category")))
            .collect();
    let now = Utc::now();
    let mut replay = Replay::new(overrides);
    let mut offset = 0i64;
    loop {
        state.runtime.check().await?;
        let page: Vec<Value> = sqlx::query(PAGE_SQL)
            .bind(org)
            .bind(now - Duration::days(30))
            .bind(now - Duration::minutes(10))
            .bind(offset)
            .fetch_all(&mut *read)
            .await?
            .into_iter()
            .map(|row| row.get(0))
            .collect();
        let count = page.len();
        offset += count as i64;
        // Page computation yields cooperatively and never holds the worker ownership row.
        for (i, row) in page.into_iter().enumerate() {
            replay.add(&serde_json::from_value::<ReplayRow>(row)?);
            if i % 256 == 255 {
                tokio::task::yield_now().await;
                state.runtime.check().await?;
            }
        }
        if count < 10000 {
            break;
        }
    }
    let samples = replay.finish();
    let histories = integrity_baseline::histories(&samples, org);
    let generation = uuid::Uuid::new_v4().to_string();
    let mut rows = Vec::new();
    for group in integrity_baseline::cohorts(&samples)
        .into_iter()
        .filter(|g| g.samples.len() >= 30)
    {
        let summary = integrity_baseline::summarize(&group);
        let mut row = json!({"id":uuid::Uuid::new_v4().to_string(),"org_id":org,"server_id":group.server_id,"source":group.source,"metric":group.metric,"level":group.level,"map":group.map,"population_bucket":group.bucket,"weapon_category":group.weapon,"window_days":30,"calculated_at":now,"model_version":MODEL_VERSION,"feature_version":FEATURE_VERSION,"weapon_map_version":model["weapon_map_version"],"generation":generation});
        for (camel, snake) in [
            ("sampleCount", "sample_count"),
            ("uniquePlayers", "unique_players"),
            ("uniquePlayerDays", "unique_player_days"),
            ("effectiveSampleSize", "effective_sample_size"),
            ("median", "median"),
            ("mad", "mad"),
            ("p90", "p90"),
            ("p95", "p95"),
            ("p99", "p99"),
            ("p995", "p995"),
            ("p999", "p999"),
            ("p9995", "p9995"),
            ("cdf", "cdf"),
            ("histogram", "histogram"),
        ] {
            row[snake] = summary[camel].clone();
        }
        rows.push(row);
    }
    anyhow::ensure!(!rows.is_empty(), "No eligible clean baseline cohorts.");
    let row_count = rows.len();
    // Keep the snapshot's advisory lock until publication completes. The snapshot does
    // not lock worker_ownership, so five-second leadership renewal continues normally.
    let published=tokio::time::timeout(std::time::Duration::from_secs(8),async {
        let mut tx=state.worker_transaction().await?;
        sqlx::query("SET LOCAL statement_timeout='6s'").execute(&mut *tx).await?;
        sqlx::query("INSERT INTO integrity_model_state(org_id) VALUES($1) ON CONFLICT DO NOTHING").bind(org).execute(&mut *tx).await?;
        let current:Value=sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id=$1 FOR UPDATE").bind(org).fetch_one(&mut *tx).await?;
        let current_cases:i64=sqlx::query_scalar("SELECT count(*) FROM integrity_cases WHERE org_id=$1").bind(org).fetch_one(&mut *tx).await?;
        if current["weapon_map_version"]!=model["weapon_map_version"] || current["active_baseline_generation"]!=model["active_baseline_generation"] || current_cases!=cases {return Ok::<_,anyhow::Error>(0)}
        sqlx::query("INSERT INTO integrity_player_metric_history(org_id,steam_id,server_id,round_id,event_id,observed_at,kpm_180,headshot_rate,max_kills_15s,feature_version,model_version) SELECT org_id,steam_id,server_id,round_id,event_id,observed_at,kpm_180,headshot_rate,max_kills_15s,feature_version,model_version FROM jsonb_populate_recordset(NULL::integrity_player_metric_history,$1) ON CONFLICT DO NOTHING").bind(json!(histories)).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM integrity_baselines WHERE org_id=$1").bind(org).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO integrity_baselines SELECT * FROM jsonb_populate_recordset(NULL::integrity_baselines,$1)").bind(json!(rows)).execute(&mut *tx).await?;
        sqlx::query("UPDATE integrity_model_state SET active_baseline_generation=$2,baseline_status='READY',last_refresh_at=$3,last_duration_ms=$4,updated_at=$3 WHERE org_id=$1").bind(org).bind(&generation).bind(now).bind(started.elapsed().as_millis().min(i32::MAX as u128) as i32).execute(&mut *tx).await?;
        if let Some(leader)=state.runtime.leader.get(){let valid:bool=sqlx::query_scalar("SELECT token=$1 AND lease_until>clock_timestamp() FROM worker_ownership WHERE id=1").bind(&leader.token).fetch_one(&mut *tx).await?;anyhow::ensure!(valid,"Worker lease lost during baseline publication");}
        tx.commit().await?;Ok(row_count)
    }).await??;
    read.commit().await?;
    Ok(published)
}
pub async fn run(state: AppState) -> anyhow::Result<()> {
    tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),_=tokio::time::sleep(std::time::Duration::from_secs(10))=>{}}
    loop {
        state.runtime.check().await?;
        let orgs:Vec<String>=sqlx::query("SELECT r.org_id FROM integrity_rules r JOIN organizations o ON o.id=r.org_id WHERE r.assessment_mode IN ('statistical_shadow','statistical') AND o.suspended_at IS NULL").fetch_all(&state.db).await?.into_iter().map(|row|row.get(0)).collect();
        for org in orgs {
            if let Err(_) = refresh(&state, &org).await {
                tracing::warn!(org_id=%org,"Integrity baseline refresh failed; previous healthy generation retained where possible");
            }
            state.runtime.check().await?;
        }
        tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),_=tokio::time::sleep(std::time::Duration::from_secs(300))=>{}}
    }
}
