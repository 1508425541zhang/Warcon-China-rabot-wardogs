//! A frozen case, current switches and current live health must agree before an action is queued.
use crate::{
    config::AppState, integrity_context, integrity_decisions as decisions, integrity_rules,
    integrity_statistics::camel_row, integrity_windows::Finding,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
pub fn date(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.to_utc())
}
pub async fn whitelist(
    db: &mut sqlx::PgConnection,
    server: &str,
    steam: &str,
) -> crate::error::Result<bool> {
    Ok(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM site_settings s,LATERAL jsonb_array_elements(s.value->'entries') v WHERE s.key='qqVip' AND v->>'serverId'=$1 AND v->>'steamId'=$2 AND v->'enabled'='true'::jsonb AND v->'whitelist'='true'::jsonb)").bind(server).bind(steam).fetch_one(db).await?)
}
pub struct Candidate<'a> {
    pub org: &'a str,
    pub server: &'a str,
    pub steam: &'a str,
    pub case: &'a str,
    pub finding: &'a Finding,
    pub score: &'a Value,
}
pub async fn enforce(
    state: &AppState,
    input: &Candidate<'_>,
) -> crate::error::Result<&'static str> {
    if input.steam.len() != 17 || !input.steam.bytes().all(|c| c.is_ascii_digit()) {
        return Ok("OBSERVE");
    }
    let mut tx = state.worker_transaction().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('qq-vip-settings',0))")
        .execute(&mut *tx)
        .await?;
    if whitelist(&mut tx, input.server, input.steam).await? {
        return Ok("OBSERVE");
    }
    let Some(row) = integrity_rules::lock(&mut tx, input.org).await? else {
        return Ok("OBSERVE");
    };
    let rules = integrity_rules::from_row(Some(&row))?;
    if !["legacy", "statistical"].contains(&rules.assessment_mode.as_str())
        || !rules.enforcement["autoSuspendedAt"].is_null()
    {
        return Ok("OBSERVE");
    }
    let case: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(c) FROM integrity_cases c WHERE id=$1 FOR UPDATE")
            .bind(input.case)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(case) = case.map(camel_row) else {
        return Ok("OBSERVE");
    };
    let saved: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_scores s WHERE id=$1")
            .bind(case["scoreId"].as_i64())
            .fetch_optional(&mut *tx)
            .await?;
    let Some(saved) = saved.map(camel_row) else {
        return Ok("OBSERVE");
    };
    if case["status"] != "OPEN"
        || case["orgId"] != input.org
        || case["serverId"] != input.server
        || case["steamId"] != input.steam
        || case["ruleVersion"] != rules.version
        || saved["ruleVersion"] != rules.version
        || saved["steamId"] != input.steam
        || saved["serverId"] != input.server
        || saved["orgId"] != input.org
        || saved["source"] != "window"
        || saved["windowId"].as_i64() != input.finding.window_id
        || saved["score"].as_f64() != case["riskScore"].as_f64()
    {
        return Ok("OBSERVE");
    }
    let statistical = &saved["statistical"];
    if rules.assessment_mode == "statistical" {
        let model: Option<Value> =
            sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id=$1")
                .bind(input.org)
                .fetch_optional(&mut *tx)
                .await?;
        let model = model.map(camel_row);
        if !decisions::action_version(statistical, &model.unwrap_or(Value::Null))
            || statistical != &case["statistical"]
        {
            return Ok("OBSERVE");
        }
    } else if saved["currentBehaviorAnomaly"] != true
        || saved["score"].as_f64() != input.score["score"].as_f64()
        || saved["level"] != input.score["level"]
    {
        return Ok("OBSERVE");
    }
    let live:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l JOIN servers s ON s.id=l.server_id JOIN organizations o ON o.id=s.org_id WHERE l.server_id=$1 AND s.org_id=$2 AND o.suspended_at IS NULL").bind(input.server).bind(input.org).fetch_optional(&mut *tx).await?;
    let Some(live) = live.map(camel_row) else {
        return Ok("OBSERVE");
    };
    let now = Utc::now();
    let roster = live["players"].as_array().cloned().unwrap_or_default();
    let fresh = |field: &str| {
        date(&live[field]).is_some_and(|d| {
            (now - d).num_milliseconds() >= 0 && (now - d).num_milliseconds() < 300000
        })
    };
    let healthy = live["ok"] == true && fresh("feedAt") && fresh("playersAt");
    let prior:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(w) FROM integrity_windows w WHERE org_id=$1 AND steam_id=$2 AND id<>$3 AND observed_at>=now()-interval '24 hours'").bind(input.org).bind(input.steam).bind(input.finding.window_id.unwrap_or(-1)).fetch_all(&mut *tx).await?;
    let previous:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(a) || jsonb_build_object('snapshot',c.snapshot) FROM integrity_actions a JOIN integrity_cases c ON c.id=a.case_id WHERE a.org_id=$1 AND a.steam_id=$2 AND a.created_at>=now()-interval '7 days' ORDER BY a.created_at DESC").bind(input.org).bind(input.steam).fetch_all(&mut *tx).await?;
    let previous: Vec<Value> = previous.into_iter().map(camel_row).collect();
    let mut finding = serde_json::to_value(input.finding).unwrap();
    if rules.assessment_mode == "statistical" {
        if let Some(frozen) = case["snapshot"].as_object() {
            for (key, value) in frozen {
                finding[key] = value.clone()
            }
        }
        finding["windowId"] = json!(input.finding.window_id);
    }
    let current = json!({"eventIds":finding["eventIds"],"roundId":finding["roundId"],"clockFrom":finding["clockFrom"],"observedAt":now});
    let independent:Vec<Value>=previous.iter().filter(|a|{let s=&a["snapshot"];integrity_context::independent_episode(&json!({"eventIds":s["eventIds"],"roundId":s["roundId"],"clockTo":s["clockTo"],"observedAt":a["createdAt"]}),&current,60.)}).cloned().collect();
    let decision_input = json!({"score":if rules.assessment_mode=="statistical"{json!({"score":saved["score"],"level":saved["level"],"breakdown":saved["breakdown"],"currentBehaviorAnomaly":saved["currentBehaviorAnomaly"]})}else{input.score.clone()},"finding":finding,"assessment":statistical,"confidence":case["confidence"],"feedHealthy":healthy,"playerOnline":fresh("playersAt")&&roster.iter().any(|p|p["steamId"]==input.steam),"onlinePlayers":roster.len(),"identityReliable":finding["eventIds"].as_array().is_some_and(|e|!e.is_empty()) && finding["steamId"]==input.steam,"priorIndependentWindow":prior.into_iter().map(camel_row).any(|r|integrity_context::independent_episode(&r,&current,60.)),"previousActions":decisions::effective_actions(&independent,now),"rules":rules.config,"settings":rules.enforcement});
    let decision = decisions::decide(&decision_input, rules.assessment_mode == "statistical");
    if decision == "OBSERVE"
        || previous
            .iter()
            .any(|a| date(&a["createdAt"]).is_some_and(|d| (now - d).num_milliseconds() < 900000))
    {
        return Ok("OBSERVE");
    }
    let (org_count,server_count):(i64,i64)=sqlx::query_as("SELECT count(*),count(*) FILTER(WHERE server_id=$2) FROM integrity_actions WHERE org_id=$1 AND source='RULE' AND created_at>=now()-interval '1 hour'").bind(input.org).bind(input.server).fetch_one(&mut *tx).await?;
    if let Some(cap) = decisions::cap(
        org_count as f64,
        server_count as f64,
        roster.len() as f64,
        &rules.enforcement,
    ) {
        sqlx::query("UPDATE integrity_rules SET auto_suspended_at=now() WHERE org_id=$1")
            .bind(input.org)
            .execute(&mut *tx)
            .await?;
        audit(
            &mut tx,
            input,
            "integrity.enforcement.circuit_breaker",
            json!({"cap":cap}),
        )
        .await?;
        tx.commit().await?;
        return Ok("OBSERVE");
    }
    let bans:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e) FROM list_entries e JOIN lists l ON l.id=e.list_id JOIN server_lists sl ON sl.list_id=l.id WHERE sl.server_id=$1 AND l.kind='ban' AND e.steam_id=$2 AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now())").bind(input.server).bind(input.steam).fetch_all(&mut *tx).await?;
    let auto: Vec<&str> = previous
        .iter()
        .filter(|a| {
            a["source"] == "RULE" && a["revertedAt"].is_null() && !a["effectiveAt"].is_null()
        })
        .filter_map(|a| a["listEntryId"].as_str())
        .collect();
    if bans
        .iter()
        .any(|b| !auto.contains(&b["id"].as_str().unwrap_or("")))
    {
        return Ok("OBSERVE");
    }
    let hours = if decision == "QUARANTINE_7D" { 168 } else { 24 };
    let expires = if decision == "KICK" {
        None
    } else {
        Some(now + chrono::Duration::hours(hours))
    };
    let reason = if decision == "KICK" {
        format!(
            "社区风控：暂时移出服务器。案件号：{}。如有异议请联系服务器管理员复核。 / Community Integrity: temporary removal. Case {}. Contact an administrator to appeal.",
            input.case, input.case
        )
    } else {
        format!(
            "社区风控：临时隔离 {hours} 小时。案件号：{}。如有异议请联系服务器管理员复核。 / Community Integrity: temporary {hours}h restriction. Case {}. Contact an administrator to appeal.",
            input.case, input.case
        )
    };
    let entry = if let Some(expires) = expires {
        let list = crate::api::lists::ensure(&mut tx, input.org, Some(input.server), "ban").await?;
        let id = if let Some(old) = bans.first() {
            if date(&old["expires_at"]).is_none_or(|d| d >= expires) {
                return Ok("OBSERVE");
            }
            let id = old["id"].as_str().unwrap().to_owned();
            sqlx::query("UPDATE list_entries SET expires_at=$2,reason=$3 WHERE id=$1 AND removed_at IS NULL").bind(&id).bind(expires).bind(&reason).execute(&mut *tx).await?;
            id
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO list_entries(id,list_id,steam_id,reason,expires_at,added_by_name) VALUES($1,$2,$3,$4,$5,'Community Integrity rule')").bind(&id).bind(&list).bind(input.steam).bind(&reason).bind(expires).execute(&mut *tx).await?;
            id
        };
        sqlx::query("UPDATE lists SET updated_at=now() WHERE id=$1")
            .bind(list)
            .execute(&mut *tx)
            .await?;
        Some(id)
    } else {
        None
    };
    let action = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO integrity_actions(id,case_id,org_id,server_id,steam_id,action,source,list_entry_id,effective_at,expires_at,delivery_state) VALUES($1,$2,$3,$4,$5,$6,'RULE',$7,$8,$9,'pending')").bind(&action).bind(input.case).bind(input.org).bind(input.server).bind(input.steam).bind(decision).bind(entry).bind(if decision=="KICK"{None}else{Some(now)}).bind(expires).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO outbox(server_id,trigger_name,trigger_kind,action,params,target,steam_id,ok_message,detail,dedupe_key) VALUES($1,'Community Integrity','integrity','kick',$2,$3,$3,$4,$5,$6)").bind(input.server).bind(json!({"steamId":input.steam,"reason":reason})).bind(input.steam).bind(format!("Community Integrity {decision}: {}",input.steam)).bind(json!({"caseId":input.case,"actionId":action,"source":"RULE"})).bind(format!("integrity:{action}:kick")).execute(&mut *tx).await?;
    if decision == "KICK" && rules.assessment_mode == "statistical" {
        sqlx::query(
            "UPDATE integrity_cases SET status='AUTO_ACTION' WHERE id=$1 AND status='OPEN'",
        )
        .bind(input.case)
        .execute(&mut *tx)
        .await?;
    }
    audit(
        &mut tx,
        input,
        "integrity.enforcement.action",
        json!({"decision":decision}),
    )
    .await?;
    tx.commit().await?;
    Ok(decision)
}
async fn audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &Candidate<'_>,
    action: &str,
    detail: Value,
) -> crate::error::Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_name,org_id,server_id,category,action,target,outcome,detail) VALUES('Community Integrity',$1,$2,'system',$3,$4,'ok',$5)").bind(input.org).bind(input.server).bind(action).bind(input.steam).bind(json!({"caseId":input.case,"detail":detail})).execute(&mut **tx).await?;
    Ok(())
}
