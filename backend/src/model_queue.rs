//! Native long-window queue; HTTP inference never holds a feed lane or worker lease row.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    integrity_enforcement::{date, whitelist},
    model_http,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
pub fn decision(score: f64) -> Option<&'static str> {
    if !score.is_finite() || score < 0. {
        return None;
    }
    let c = model_http::calibration();
    if score >= c["p99"].as_f64()? {
        Some("QUARANTINE_24H")
    } else if score >= c["p98"].as_f64()? {
        Some("KICK")
    } else {
        None
    }
}
pub async fn schedule(
    state: &AppState,
    org: &str,
    server: &str,
    steam: &str,
    match_id: i64,
    at: DateTime<Utc>,
) -> Result<()> {
    let c = model_http::config(&state.db, org).await?;
    if !c.developer_enabled {
        return Ok(());
    }
    crate::api::notes::steam_id(steam)?;
    let mut tx = state.worker_transaction().await?;
    sqlx::query("INSERT INTO integrity_model_runs(id,org_id,server_id,steam_id,match_id,slot,config_revision,threshold,created_at) SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9 WHERE EXISTS(SELECT 1 FROM servers s JOIN organizations o ON o.id=s.org_id JOIN integrity_rules r ON r.org_id=s.org_id WHERE s.id=$3 AND s.org_id=$2 AND o.suspended_at IS NULL AND r.assessment_mode IN ('model_only','long_only')) AND EXISTS(SELECT 1 FROM matches WHERE id=$5 AND server_id=$3 AND started_at<=$9-interval '30 minutes') AND NOT EXISTS(SELECT 1 FROM integrity_model_runs WHERE server_id=$3 AND steam_id=$4 AND match_id=$5 AND config_revision=$7 AND created_at>$9-($10::int*interval '1 second')) ON CONFLICT DO NOTHING").bind(uuid::Uuid::new_v4().to_string()).bind(org).bind(server).bind(steam).bind(match_id).bind(at.timestamp().div_euclid(c.interval_seconds as i64)).bind(c.revision).bind(model_http::calibration()["p98"].as_f64()).bind(at).bind(c.interval_seconds).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn schedule_online(state: &AppState) -> Result<()> {
    let rows:Vec<(String,String,Value,i64)>=sqlx::query_as("SELECT s.org_id,s.id,l.players,m.id FROM servers s JOIN organizations o ON o.id=s.org_id JOIN integrity_rules r ON r.org_id=s.org_id JOIN site_settings c ON c.key='integrityModel:'||s.org_id JOIN server_live l ON l.server_id=s.id JOIN LATERAL(SELECT id FROM matches WHERE server_id=s.id AND ended_at IS NULL AND started_at<=now()-interval '30 minutes' ORDER BY started_at DESC LIMIT 1)m ON true WHERE o.suspended_at IS NULL AND l.ok AND l.players_at>now()-interval '30 seconds' AND l.status_at>now()-interval '30 seconds' AND r.assessment_mode IN ('model_only','long_only') AND c.value->>'developerEnabled'='true'").fetch_all(&state.db).await?;
    for (org, server, players, match_id) in rows {
        for p in players.as_array().into_iter().flatten() {
            if let Some(steam) = p["steamId"].as_str() {
                schedule(state, &org, &server, steam, match_id, Utc::now()).await?;
            }
        }
    }
    Ok(())
}
pub async fn sources(
    state: &AppState,
    server: &str,
    steam: &str,
    match_id: i64,
    at: DateTime<Utc>,
) -> Result<Value> {
    // A coherent read snapshot preserves the original observation cutoff and row ordering.
    let mut tx = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ,READ ONLY")
        .execute(&mut *tx)
        .await?;
    let matches:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'started_at',started_at,'ended_at',$3::timestamptz,'map',map) FROM matches WHERE id=$1 AND server_id=$2 AND started_at<$3").bind(match_id).bind(server).bind(at).fetch_all(&mut *tx).await?;
    if matches.is_empty() {
        return Err(ApiError::missing());
    }
    let since = at - Duration::seconds(1900);
    let progress:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(v) FROM(SELECT server_id,match_id,observed_at,jsonb_array_length(players) AS roster_size,(SELECT jsonb_agg(jsonb_build_object('steamId','player','cash',p->'cash','kills',p->'kills','deaths',p->'deaths')) FROM jsonb_array_elements(players)p WHERE p->>'steamId'=$3)AS players FROM player_progress_samples WHERE server_id=$1 AND match_id=$2 AND observed_at>=$4 AND observed_at<$5 AND EXISTS(SELECT 1 FROM jsonb_array_elements(players)p WHERE p->>'steamId'=$3) ORDER BY observed_at LIMIT 25001)v").bind(server).bind(match_id).bind(steam).bind(since).bind(at).fetch_all(&mut *tx).await?;
    let pattern = format!("%:match:{match_id}");
    let history:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(v) FROM(SELECT server_id,round_id,observed_at,'player' AS steam_id,kpm_180,headshot_rate,max_kills_15s FROM integrity_player_metric_history WHERE server_id=$1 AND steam_id=$2 AND round_id LIKE $3 AND observed_at>=$4 AND observed_at<$5 ORDER BY observed_at,id LIMIT 25001)v").bind(server).bind(steam).bind(&pattern).bind(since).bind(at).fetch_all(&mut *tx).await?;
    let windows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(v) FROM(SELECT server_id,round_id,observed_at,'player' AS steam_id,infantry_kills,kpm_180,unique_victims,headshots,penetrations,burst_points,max_kills_15s,median_kill_interval FROM integrity_windows WHERE server_id=$1 AND steam_id=$2 AND round_id LIKE $3 AND observed_at>=$4 AND observed_at<$5 ORDER BY observed_at,id LIMIT 25001)v").bind(server).bind(steam).bind(pattern).bind(since).bind(at).fetch_all(&mut *tx).await?;
    if [progress.len(), history.len(), windows.len()]
        .into_iter()
        .any(|n| n > 25000)
    {
        return Err(ApiError::new(
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            "model_input_limit",
            "模型输入超限，未截断数据。",
        ));
    }
    tx.commit().await?;
    Ok(
        json!({"matches":matches,"player_progress_samples":progress,"integrity_player_metric_history":history,"integrity_windows":windows}),
    )
}
async fn mark_skip(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
    reason: &str,
) -> Result<Option<&'static str>> {
    sqlx::query(
        "UPDATE integrity_model_runs SET action_state='skipped',action_reason=$2 WHERE id=$1",
    )
    .bind(id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(None)
}
pub async fn enforce(state: &AppState, id: &str) -> Result<Option<&'static str>> {
    let candidate:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_model_runs r WHERE id=$1 AND state='READY' AND action_state IS NULL").bind(id).fetch_optional(&state.db).await?;
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let org = candidate["org_id"].as_str().unwrap();
    let mut tx = state.worker_transaction().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("integrityModel:{org}"))
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('qq-vip-settings',0))")
        .execute(&mut *tx)
        .await?;
    let run: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_model_runs r WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(run) = run.filter(|r| r["state"] == "READY" && r["action_state"].is_null()) else {
        return Ok(None);
    };
    let server = run["server_id"].as_str().unwrap();
    let steam = run["steam_id"].as_str().unwrap();
    let stored: Option<Value> = sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1")
        .bind(format!("integrityModel:{org}"))
        .fetch_optional(&mut *tx)
        .await?;
    let c = model_http::parse_config(stored)?;
    let rules: Option<String> = sqlx::query_scalar(
        "SELECT assessment_mode FROM integrity_rules WHERE org_id=$1 FOR UPDATE",
    )
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let result=async{
        if !c.developer_enabled||!c.auto_punish_enabled||run["config_revision"]!=c.revision||!rules.as_deref().is_some_and(crate::integrity_rules::long_enabled){return mark_skip(&mut tx,id,"模型模式、自动处罚或配置版本已改变").await}
        let score=model_http::verify_result(&run["result"],id)?;
        if score.is_none()||score!=run["score"].as_f64(){return mark_skip(&mut tx,id,"模型结果无效").await}
        let Some(action)=decision(score.unwrap())else{return mark_skip(&mut tx,id,"低于 P98").await};
        let live:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l JOIN servers s ON s.id=l.server_id JOIN organizations o ON o.id=s.org_id WHERE l.server_id=$1 AND s.org_id=$2 AND o.suspended_at IS NULL AND l.ok AND l.players_at>now()-interval '30 seconds' AND l.status_at>now()-interval '30 seconds'").bind(server).bind(org).fetch_optional(&mut *tx).await?;
        let Some(live)=live.filter(|l|l["players"].as_array().is_some_and(|p|p.iter().any(|p|p["steamId"]==steam))&&date(&run["created_at"]).is_some_and(|d|(Utc::now()-d).num_milliseconds()<=300000))else{return mark_skip(&mut tx,id,"玩家离线、观测不健康或评分已过期").await};
        let current_match:Option<i64>=sqlx::query_scalar("SELECT id FROM matches WHERE server_id=$1 AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1").bind(server).fetch_optional(&mut *tx).await?;
        if current_match!=run["match_id"].as_i64(){return mark_skip(&mut tx,id,"对局已改变").await}
        if whitelist(&mut tx,server,steam).await?{return mark_skip(&mut tx,id,"VIP 白名单免除自动风控处罚").await}
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM integrity_model_runs WHERE org_id=$1 AND punished_at>now()-interval '1 hour'").bind(org).fetch_one(&mut *tx).await?;if count>=c.max_actions_per_hour as i64{return mark_skip(&mut tx,id,"达到每小时自动处罚上限").await}
        let cooldown:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_model_runs WHERE server_id=$1 AND steam_id=$2 AND punished_at>now()-($3::int*interval '1 second') AND (action='QUARANTINE_24H' OR $4='KICK'))").bind(server).bind(steam).bind(c.cooldown_seconds).bind(action).fetch_one(&mut *tx).await?;if cooldown{return mark_skip(&mut tx,id,"玩家处罚冷却中").await}
        let banned:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM list_entries e JOIN server_lists sl ON sl.list_id=e.list_id JOIN lists l ON l.id=e.list_id WHERE sl.server_id=$1 AND l.kind='ban' AND e.steam_id=$2 AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()))").bind(server).bind(steam).fetch_one(&mut *tx).await?;if banned{return mark_skip(&mut tx,id,"已有有效封禁，保留原记录").await}
        let expires=if action=="QUARANTINE_24H"{Some(Utc::now()+Duration::days(1))}else{None};let entry=expires.map(|_|uuid::Uuid::new_v4().to_string());
        let reason=format!("模型 A测：异常分数达到 {}。记录 {id}，可联系管理员复核。",if expires.is_some(){"P99，临时隔离24小时"}else{"P98，自动踢出"});
        if let Some(entry)=&entry{let list=crate::api::lists::ensure(&mut tx,org,Some(server),"ban").await?;sqlx::query("INSERT INTO list_entries(id,list_id,steam_id,reason,expires_at,added_by_name) VALUES($1,$2,$3,$4,$5,'Model A-test rule')").bind(entry).bind(&list).bind(steam).bind(&reason).bind(expires).execute(&mut *tx).await?;sqlx::query("UPDATE lists SET updated_at=now() WHERE id=$1").bind(list).execute(&mut *tx).await?;}
        let name=live["players"].as_array().unwrap().iter().find(|p|p["steamId"]==steam).map(|p|p["name"].clone()).unwrap_or(Value::Null);
        sqlx::query("INSERT INTO outbox(server_id,trigger_name,trigger_kind,action,params,target,steam_id,ok_message,detail,dedupe_key) VALUES($1,'模型 A测 P98/P99','model_integrity','kick',$2,$3,$3,$4,$5,$6)").bind(server).bind(json!({"steamId":steam,"reason":reason})).bind(steam).bind(format!("模型 {action}: {steam}")).bind(json!({"runId":id,"playerName":name,"modelCalibration":model_http::calibration()})).bind(format!("model:{id}:kick")).execute(&mut *tx).await?;
        sqlx::query("UPDATE integrity_model_runs SET action=$2,action_state='pending',action_reason=$3,punished_at=now(),expires_at=$4,list_entry_id=$5 WHERE id=$1").bind(id).bind(action).bind(reason).bind(expires).bind(entry).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO audit_log(actor_name,org_id,server_id,category,action,target,outcome,message,detail) VALUES('模型 A测',$1,$2,'trigger',$3,$4,'ok','模型自动处罚已入队',$5)").bind(org).bind(server).bind(format!("model.{action}")).bind(steam).bind(json!({"runId":id,"decision":action,"score":score,"p98":model_http::calibration()["p98"],"p99":model_http::calibration()["p99"]})).execute(&mut *tx).await?;
        Ok(Some(action))
    }.await?;
    tx.commit().await?;
    Ok(result)
}
pub async fn process_next(state: &AppState) -> Result<bool> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE integrity_model_runs j SET state='superseded',finished_at=now(),result=jsonb_build_object('reason','模型配置已更新或长时序评估已停用，未执行评分。') WHERE j.state='pending' AND NOT EXISTS(SELECT 1 FROM site_settings s JOIN integrity_rules r ON r.org_id=j.org_id WHERE s.key='integrityModel:'||j.org_id AND s.value->>'developerEnabled'='true' AND s.value->>'revision'=j.config_revision AND r.assessment_mode IN ('model_only','long_only'))").execute(&mut *tx).await?;
    let recover:Vec<String>=sqlx::query_scalar("SELECT id FROM integrity_model_runs WHERE state='READY' AND action_state IS NULL ORDER BY created_at LIMIT 5").fetch_all(&mut *tx).await?;
    sqlx::query("UPDATE integrity_model_runs SET state='ERROR',finished_at=now() WHERE state='running' AND created_at<now()-interval '5 minutes'").execute(&mut *tx).await?;
    let job:Option<Value>=sqlx::query_scalar("UPDATE integrity_model_runs SET state='running' WHERE id=(SELECT id FROM integrity_model_runs WHERE state='pending' ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING to_jsonb(integrity_model_runs)").fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    for id in recover {
        enforce(state, &id).await?;
    }
    let Some(job) = job else { return Ok(false) };
    let id = job["id"].as_str().unwrap();
    let org = job["org_id"].as_str().unwrap();
    let result=async{
        let config=model_http::config(&state.db,org).await?;
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_rules WHERE org_id=$1 AND assessment_mode IN ('model_only','long_only'))").bind(org).fetch_one(&state.db).await?;
        if !config.developer_enabled||config.revision!=job["config_revision"]||!active{return supersede(state,id).await}
        let sources=sources(state,job["server_id"].as_str().unwrap(),job["steam_id"].as_str().unwrap(),job["match_id"].as_i64().unwrap(),date(&job["created_at"]).ok_or_else(||ApiError::bad("Invalid observation time."))?).await?;
        let output=model_http::call(state,&config,Some(&json!({"schema":model_http::manifest()["feature_schema"],"requestId":id,"sources":sources}))).await?;
        let score=model_http::verify_result(&output,id)?;
        let mut tx=state.worker_transaction().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))").bind(format!("integrityModel:{org}")).execute(&mut *tx).await?;
        let stored:Option<Value>=sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1").bind(format!("integrityModel:{org}")).fetch_optional(&mut *tx).await?;let current=model_http::parse_config(stored)?;
        let mode:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_rules WHERE org_id=$1 AND assessment_mode IN ('model_only','long_only'))").bind(org).fetch_one(&mut *tx).await?;
        let stale=!current.developer_enabled||current.revision!=config.revision||!mode;
        let mut detail=output;if stale{detail["reason"]=json!("评分期间模型配置或评估模式已变更，结果不用于处罚。");}
        let changed=sqlx::query("UPDATE integrity_model_runs SET state=$2,score=$3,result=$4,finished_at=now() WHERE id=$1 AND state='running' AND config_revision=$5").bind(id).bind(if stale{"superseded"}else if score.is_some(){"READY"}else{"INSUFFICIENT_DATA"}).bind(score).bind(detail).bind(config.revision).execute(&mut *tx).await?;tx.commit().await?;
        if changed.rows_affected()==1&&!stale&&score.is_some(){enforce(state,id).await?;}Ok(())
    }.await;
    if result.is_err() {
        state.runtime.check().await?;
        let mut tx = state.worker_transaction().await?;
        sqlx::query("UPDATE integrity_model_runs SET state='ERROR',result='{\"message\":\"模型请求或处置失败，请检查处罚状态；未回退专家。\"}'::jsonb,finished_at=now() WHERE id=$1 AND punished_at IS NULL AND state IN ('running','READY')").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(true)
}
async fn supersede(state: &AppState, id: &str) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE integrity_model_runs SET state='superseded',finished_at=now(),result=jsonb_build_object('reason','模型配置已更新或长时序评估已停用，未执行评分。') WHERE id=$1 AND state='running'").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn run(state: AppState) -> anyhow::Result<()> {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),_=tick.tick()=>{state.runtime.check().await?;if schedule_online(&state).await.is_err(){tracing::warn!("Long-window model scheduling failed");}for _ in 0..20{state.runtime.check().await?;match process_next(&state).await{Ok(true)=>{},Ok(false)=>break,Err(_)=>{tracing::warn!("Long-window model queue pass failed");break}}}}}
    }
}
