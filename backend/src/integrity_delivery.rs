//! Delivery is checked against committed authorization, independently of queued assessment.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    integrity_decisions,
    integrity_enforcement::whitelist,
    integrity_statistics::camel_row,
};
use serde_json::Value;
pub async fn record(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &Value,
    state: &str,
) -> Result<()> {
    if row["triggerKind"] == "model_integrity" {
        let id = row["detail"]["runId"]
            .as_str()
            .ok_or_else(|| ApiError::bad("Model action identity is missing."))?;
        let result=sqlx::query("UPDATE integrity_model_runs SET action_state=$4 WHERE id=$1 AND server_id=$2 AND steam_id=$3 AND action IS NOT NULL").bind(id).bind(row["serverId"].as_str()).bind(row["steamId"].as_str()).bind(state).execute(&mut **tx).await?;
        if result.rows_affected() != 1 {
            return Err(ApiError::bad(
                "Model action disappeared before delivery finalization.",
            ));
        }
        return Ok(());
    }
    if row["triggerKind"] != "integrity" {
        return Ok(());
    }
    let action = row["detail"]["actionId"]
        .as_str()
        .ok_or_else(|| ApiError::bad("Integrity action identity is missing."))?;
    let case = row["detail"]["caseId"]
        .as_str()
        .ok_or_else(|| ApiError::bad("Integrity case identity is missing."))?;
    if row["action"] != "kick" || row["steamId"].as_str().is_none() {
        return Err(ApiError::bad("Invalid integrity delivery identity."));
    }
    let previous:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM integrity_actions a WHERE id=$1 AND case_id=$2 AND server_id=$3 AND steam_id=$4 FOR UPDATE").bind(action).bind(case).bind(row["serverId"].as_str()).bind(row["steamId"].as_str()).fetch_optional(&mut **tx).await?;
    let previous = previous.ok_or_else(|| {
        ApiError::bad("Integrity action disappeared before delivery finalization.")
    })?;
    if !previous["reverted_at"].is_null() {
        return Ok(());
    }
    sqlx::query("UPDATE integrity_actions SET delivery_state=$2,effective_at=CASE WHEN $2='delivered' THEN now() ELSE effective_at END WHERE id=$1 AND reverted_at IS NULL").bind(action).bind(state).execute(&mut **tx).await?;
    Ok(())
}
pub async fn skip(state: &AppState, row: &Value) -> Result<Option<&'static str>> {
    let kind = row["triggerKind"].as_str().unwrap_or("");
    if !["integrity", "model_integrity", "risk_kick"].contains(&kind) {
        return Ok(None);
    }
    let server = row["serverId"].as_str().unwrap_or("");
    let steam = row["steamId"].as_str().unwrap_or("");
    let mut conn = state.db.acquire().await?;
    if kind == "risk_kick" {
        return Ok(
            if row["action"] == "kick"
                && !steam.is_empty()
                && whitelist(&mut conn, server, steam).await?
            {
                Some("VIP 白名单：跳过自动风险踢人")
            } else {
                None
            },
        );
    }
    if kind == "model_integrity" {
        let run:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_model_runs r WHERE id=$1 AND server_id=$2 AND steam_id=$3").bind(row["detail"]["runId"].as_str().unwrap_or("")).bind(server).bind(steam).fetch_optional(&mut *conn).await?;
        let Some(run) = run.filter(|r| !r["action"].is_null()) else {
            return Ok(Some("模型处罚记录缺失"));
        };
        let org = run["org_id"].as_str().unwrap();
        let config = crate::model_http::config(&state.db, org).await?;
        let mode:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_rules WHERE org_id=$1 AND assessment_mode IN ('model_only','long_only'))").bind(org).fetch_one(&mut *conn).await?;
        if !config.developer_enabled
            || !config.auto_punish_enabled
            || run["config_revision"] != config.revision
            || !mode
        {
            return Ok(Some("模型自动处罚已停用或配置改变"));
        }
        if whitelist(&mut conn, server, steam).await? {
            return Ok(Some("VIP 白名单免除自动风控处罚"));
        }
        let healthy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM server_live WHERE server_id=$1 AND ok AND players_at>now()-interval '30 seconds' AND status_at>now()-interval '30 seconds')").bind(server).fetch_one(&mut *conn).await?;
        if !healthy {
            return Ok(Some("服务器观测不健康"));
        }
        if let Some(entry) = run["list_entry_id"].as_str() {
            let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM list_entries WHERE id=$1 AND removed_at IS NULL AND expires_at>now())").bind(entry).fetch_one(&mut *conn).await?;
            if !active {
                return Ok(Some("模型隔离已撤销或到期"));
            }
        }
        return Ok(None);
    }
    if row["action"] != "kick" || steam.is_empty() {
        return Ok(Some("Integrity action identity is invalid before delivery"));
    }
    let matched:Option<(Value,Value,String)>=sqlx::query_as("SELECT to_jsonb(a),to_jsonb(c),s.org_id FROM integrity_actions a JOIN integrity_cases c ON c.id=a.case_id JOIN servers s ON s.id=a.server_id WHERE a.id=$1 AND a.case_id=$2").bind(row["detail"]["actionId"].as_str().unwrap_or("")).bind(row["detail"]["caseId"].as_str().unwrap_or("")).fetch_optional(&mut *conn).await?;
    let Some((action, case, server_org)) = matched else {
        return Ok(Some("Integrity action or case changed before delivery"));
    };
    let action = camel_row(action);
    let case = camel_row(case);
    if !["RULE", "REVIEW"].contains(&action["source"].as_str().unwrap_or(""))
        || !action["revertedAt"].is_null()
        || action["serverId"] != server
        || action["steamId"] != steam
        || case["orgId"] != action["orgId"]
        || case["serverId"] != server
        || case["steamId"] != steam
        || action["orgId"] != server_org
        || (action["source"] == "RULE"
            && (!["OPEN", "AUTO_ACTION"].contains(&case["status"].as_str().unwrap_or(""))
                || !case["reviewedAt"].is_null()))
    {
        return Ok(Some("Integrity action or case changed before delivery"));
    }
    if action["source"] == "REVIEW" {
        if action["action"] != "QUARANTINE_7D"
            || action["listEntryId"].is_null()
            || case["reviewedAt"].is_null()
        {
            return Ok(Some("人工审核处罚记录不完整"));
        }
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM list_entries e JOIN lists l ON l.id=e.list_id JOIN server_lists sl ON sl.list_id=l.id WHERE e.id=$1 AND e.steam_id=$2 AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()) AND l.kind='ban' AND l.org_id=$3 AND sl.server_id=$4)").bind(action["listEntryId"].as_str()).bind(steam).bind(action["orgId"].as_str()).bind(server).fetch_one(&mut *conn).await?;
        return Ok(if active {
            None
        } else {
            Some("人工封禁已撤销或到期")
        });
    }
    if whitelist(&mut conn, server, steam).await? {
        return Ok(Some("VIP 白名单：免除自动风控处罚"));
    }
    let rules: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_rules r WHERE org_id=$1")
            .bind(action["orgId"].as_str())
            .fetch_optional(&mut *conn)
            .await?;
    let Some(rules) = rules else {
        return Ok(Some("Integrity enforcement disabled before delivery"));
    };
    if !rules["auto_suspended_at"].is_null() || rules["version"] != case["ruleVersion"] {
        return Ok(Some("Integrity enforcement disabled before delivery"));
    }
    if !["legacy", "statistical"].contains(&rules["assessment_mode"].as_str().unwrap_or("")) {
        return Ok(Some("未选择旧规则或委员会，不执行其自动处罚"));
    }
    if rules["assessment_mode"] == "statistical" {
        let model: Option<Value> =
            sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id=$1")
                .bind(action["orgId"].as_str())
                .fetch_optional(&mut *conn)
                .await?;
        if !integrity_decisions::action_version(
            &case["statistical"],
            &model.map(camel_row).unwrap_or(Value::Null),
        ) {
            return Ok(Some("Integrity enforcement disabled before delivery"));
        }
    }
    let key = match action["action"].as_str() {
        Some("KICK") => "auto_kick_enabled",
        Some("QUARANTINE_24H") => "auto_quarantine_24h_enabled",
        Some("QUARANTINE_7D") => "auto_quarantine_7d_enabled",
        _ => return Ok(Some("Integrity action type is invalid before delivery")),
    };
    Ok(if rules[key] == true {
        None
    } else {
        Some("Integrity enforcement disabled before delivery")
    })
}
