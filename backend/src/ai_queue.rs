//! Automatic triage with durable claims and human-review precedence. No ban calls.
use crate::{
    ai_protocol as protocol,
    config::AppState,
    error::{ApiError, Result},
    integrity_ai,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
pub async fn discover(state: &AppState) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE integrity_ai_jobs j SET state='pending',attempts=0,next_at=now(),result=NULL,last_error=NULL,updated_at=now() FROM integrity_cases c JOIN integrity_ai_settings s ON s.org_id=c.org_id JOIN organizations o ON o.id=c.org_id WHERE j.case_id=c.id AND j.state='done' AND(j.result->>'promptVersion' IS DISTINCT FROM $1 OR(s.auto_close_enabled AND j.result->>'disposition'='ADVISORY')) AND o.suspended_at IS NULL AND EXISTS(SELECT 1 FROM integrity_rules r WHERE r.org_id=c.org_id AND r.assessment_mode IN('statistical','statistical_shadow')) AND s.auto_enabled AND c.status='OPEN' AND c.reviewed_at IS NULL").bind(protocol::PROMPT_VERSION).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO integrity_ai_jobs(case_id) SELECT c.id FROM integrity_cases c JOIN integrity_ai_settings s ON s.org_id=c.org_id JOIN organizations o ON o.id=c.org_id WHERE o.suspended_at IS NULL AND EXISTS(SELECT 1 FROM integrity_rules r WHERE r.org_id=c.org_id AND r.assessment_mode IN('statistical','statistical_shadow')) AND s.auto_enabled AND s.model<>'' AND c.reviewed_at IS NULL AND c.status='OPEN' AND NOT EXISTS(SELECT 1 FROM integrity_ai_jobs j WHERE j.case_id=c.id) ORDER BY(c.trigger='AI_SINGLE_SIGNAL'),c.created_at,c.id LIMIT 100 ON CONFLICT DO NOTHING").execute(&mut *tx).await?;
    sqlx::query("UPDATE integrity_ai_jobs j SET state='skipped',last_error='案件已人工审核或关闭',updated_at=now(),lease_until=NULL FROM integrity_cases c WHERE j.case_id=c.id AND j.state='pending' AND(c.reviewed_at IS NOT NULL OR c.status<>'OPEN')").execute(&mut *tx).await?;
    sqlx::query("UPDATE integrity_ai_jobs SET state='error',last_error='多次执行中断；停止自动重试',updated_at=now(),lease_until=NULL WHERE state='running' AND lease_until<now() AND attempts>=3").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn claim(state: &AppState, token: &str) -> Result<Option<Value>> {
    let mut tx = state.worker_transaction().await?;
    let job:Option<Value>=sqlx::query_scalar("UPDATE integrity_ai_jobs j SET state='running',attempts=attempts+1,claim_token=$1,lease_until=now()+interval '3 minutes',updated_at=now() WHERE j.case_id=(SELECT q.case_id FROM integrity_ai_jobs q JOIN integrity_cases c ON c.id=q.case_id JOIN integrity_ai_settings s ON s.org_id=c.org_id JOIN organizations o ON o.id=c.org_id WHERE o.suspended_at IS NULL AND EXISTS(SELECT 1 FROM integrity_rules r WHERE r.org_id=c.org_id AND r.assessment_mode IN('statistical','statistical_shadow')) AND s.auto_enabled AND s.model<>'' AND c.reviewed_at IS NULL AND c.status='OPEN' AND(s.last_request_at IS NULL OR s.last_request_at<now()-interval '65 seconds') AND(s.budget_day<>to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD') OR s.daily_requests<s.daily_limit) AND q.attempts<3 AND((q.state='pending' AND q.next_at<=now()) OR(q.state='running' AND q.lease_until<now())) ORDER BY(c.trigger='AI_SINGLE_SIGNAL'),q.next_at,c.created_at,q.case_id LIMIT 1 FOR UPDATE OF q SKIP LOCKED) RETURNING to_jsonb(j)").bind(token).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    Ok(job)
}
pub async fn finish(
    tx: &mut Transaction<'_, Postgres>,
    case: &str,
    token: &str,
    input: &Value,
    version: DateTime<Utc>,
) -> Result<()> {
    let parsed = protocol::review(input)?;
    let Some(c): Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(c) FROM integrity_cases c WHERE id=$1 FOR UPDATE")
            .bind(case)
            .fetch_optional(&mut **tx)
            .await?
    else {
        return Ok(());
    };
    let config: Option<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM integrity_ai_settings s WHERE org_id=$1 FOR SHARE",
    )
    .bind(c["org_id"].as_str())
    .fetch_optional(&mut **tx)
    .await?;
    let org: Option<bool> =
        sqlx::query_scalar("SELECT suspended_at IS NULL FROM organizations WHERE id=$1 FOR SHARE")
            .bind(c["org_id"].as_str())
            .fetch_optional(&mut **tx)
            .await?;
    let job:Option<bool>=sqlx::query_scalar("SELECT state='running' AND claim_token=$2 AND lease_until>now() FROM integrity_ai_jobs WHERE case_id=$1 FOR UPDATE").bind(case).bind(token).fetch_optional(&mut **tx).await?;
    if job != Some(true) {
        return Ok(());
    }
    if org != Some(true)
        || config.as_ref().is_none_or(|s| {
            s["auto_enabled"] != true
                || crate::integrity_enforcement::date(&s["updated_at"]) != Some(version)
        })
    {
        sqlx::query("UPDATE integrity_ai_jobs SET state='pending',lease_until=NULL,attempts=0,next_at=now(),last_error='配置已变更，等待按最新配置重新审核',updated_at=now() WHERE case_id=$1").bind(case).execute(&mut **tx).await?;
        return Ok(());
    }
    let config = config.unwrap();
    let mut disposition = "ADVISORY";
    if c["status"] != "OPEN" || !c["reviewed_at"].is_null() {
        disposition = "SKIPPED_REVIEWED"
    } else if config["auto_close_enabled"] == true {
        disposition = if protocol::numeric_checks(&c["snapshot"])["kpm180"]["matches"] == false {
            if parsed.suspicion_percent.is_some_and(|n| n >= 65) {
                "ADMIN_REVIEW"
            } else {
                "AI_ARCHIVED_UNRESOLVED"
            }
        } else {
            protocol::triage(&parsed, config["delete_low_risk"] == true)
        };
        let protected:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_actions WHERE case_id=$1 UNION ALL SELECT 1 FROM integrity_labels WHERE case_id=$1 UNION ALL SELECT 1 FROM integrity_reports WHERE case_id=$1 UNION ALL SELECT 1 FROM integrity_action_eligibility WHERE case_id=$1)").bind(case).fetch_one(&mut **tx).await?;
        if protected {
            disposition = "ADMIN_REVIEW"
        }
        if ["AI_ARCHIVED", "AI_CLEARED", "AI_ARCHIVED_UNRESOLVED"].contains(&disposition) {
            sqlx::query("UPDATE integrity_cases SET status=$2,reviewed_at=now(),reviewed_by=NULL WHERE id=$1").bind(case).bind(if disposition=="AI_CLEARED"{"AI_CLEARED"}else{"AI_ARCHIVED"}).execute(&mut **tx).await?;
        }
        if disposition == "AI_CLEARED" {
            sqlx::query("UPDATE integrity_cases SET snapshot=jsonb_build_object('roundId',snapshot->'roundId','eventIds',snapshot->'eventIds'),statistical=CASE WHEN statistical IS NULL THEN NULL ELSE jsonb_build_object('level',statistical->'level') END,risk_breakdown='[]'::jsonb WHERE id=$1").bind(case).execute(&mut **tx).await?;
            sqlx::query("UPDATE integrity_case_events SET event=jsonb_build_object('killerSteamId',event->'killerSteamId','ts',event->'ts') WHERE case_id=$1").bind(case).execute(&mut **tx).await?;
            sqlx::query("DELETE FROM integrity_ai_reviews WHERE case_id=$1")
                .bind(case)
                .execute(&mut **tx)
                .await?;
        }
    }
    let mut result = serde_json::to_value(&parsed).unwrap();
    result["promptVersion"] = json!(protocol::PROMPT_VERSION);
    result["disposition"] = json!(disposition);
    result["model"] = input["model"]
        .as_str()
        .map(|s| json!(s))
        .unwrap_or(config["model"].clone());
    if disposition == "AI_CLEARED" {
        result["reasons"] = json!([]);
        result["alternatives"] = json!([]);
        result["summary"] = json!(crate::feed::truncate(&parsed.summary, 300))
    }
    sqlx::query("UPDATE integrity_ai_jobs SET state='done',result=$2,last_error=NULL,lease_until=NULL,updated_at=now() WHERE case_id=$1").bind(case).bind(result).execute(&mut **tx).await?;
    Ok(())
}
pub async fn process_next(state: &AppState) -> Result<bool> {
    let token = uuid::Uuid::new_v4().to_string();
    let Some(job) = claim(state, &token).await? else {
        return Ok(false);
    };
    let case = job["case_id"].as_str().ok_or_else(ApiError::missing)?;
    let work = async {
        let c: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM integrity_cases c WHERE id=$1")
            .bind(case)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(ApiError::missing)?;
        let org = c["org_id"].as_str().ok_or_else(ApiError::missing)?;
        let server = c["server_id"].as_str().ok_or_else(ApiError::missing)?;
        let bundle = crate::ai_evidence::bundle(state, org, server, case).await?;
        let response = integrity_ai::call(state, org, "review", Some(&bundle), true).await?;
        let mut tx = state.worker_transaction().await?;
        finish(
            &mut tx,
            case,
            &token,
            &response.response["result"],
            response.settings_version,
        )
        .await?;
        tx.commit().await?;
        Ok::<(), ApiError>(())
    }
    .await;
    if let Err(error) = work {
        let deferred =
            [StatusCode::CONFLICT, StatusCode::TOO_MANY_REQUESTS].contains(&error.status);
        let attempts = job["attempts"].as_i64().unwrap_or(3);
        let exhausted = attempts >= 3
            || [
                StatusCode::BAD_REQUEST,
                StatusCode::NOT_FOUND,
                StatusCode::PAYLOAD_TOO_LARGE,
                StatusCode::GONE,
            ]
            .contains(&error.status);
        let mut tx = state.worker_transaction().await?;
        sqlx::query("UPDATE integrity_ai_jobs SET state=$3,attempts=$4,next_at=now()+make_interval(secs=>$5),lease_until=NULL,last_error=$6,updated_at=now() WHERE case_id=$1 AND claim_token=$2 AND state='running'").bind(case).bind(token).bind(if deferred||!exhausted{"pending"}else{"error"}).bind(if deferred{attempts-1}else{attempts} as i32).bind(if deferred{70.}else{300.*attempts as f64}).bind(error.message).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(true)
}
use axum::http::StatusCode;
pub async fn views(state: &AppState, ids: &[String]) -> Result<Vec<Value>> {
    let rows: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(j) FROM integrity_ai_jobs j WHERE case_id=ANY($1)")
            .bind(ids)
            .fetch_all(&state.db)
            .await?;
    Ok(rows.into_iter().map(|r|json!({"caseId":r["case_id"],"state":r["state"],"attempts":r["attempts"],"lastError":r["last_error"],"updatedAt":crate::ai_evidence::row(r.clone())["updatedAt"],"result":protocol::review(&r["result"]).ok(),"disposition":r["result"]["disposition"].as_str(),"promptVersion":protocol::PROMPT_VERSION})).collect())
}
pub async fn run(state: AppState) -> Result<()> {
    let mut clock = tokio::time::interval(std::time::Duration::from_secs(10));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=state.runtime.stop.cancelled()=>break,_=clock.tick()=>{if let Err(e)=discover(&state).await{tracing::warn!(code=e.code,"AI discovery failed");continue}if let Err(e)=process_next(&state).await{tracing::warn!(code=e.code,"AI triage pass failed")}}}
    }
    Ok(())
}
