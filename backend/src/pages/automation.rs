use super::Context;
use crate::{
    auth,
    config::AppState,
    error::{ApiError, Result},
};
use chrono::Utc;
use serde_json::{Value, json};
pub async fn rows(
    state: &AppState,
    table: &str,
    id: &str,
    order: &str,
    limit: i64,
) -> Result<Vec<Value>> {
    // table/order are compile-time controller constants, never input parameters.
    Ok(sqlx::query_scalar(&format!(
        "SELECT to_jsonb(t) FROM {table} t WHERE server_id=$1 ORDER BY {order} DESC LIMIT $2"
    ))
    .bind(id)
    .bind(limit)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(crate::ai_evidence::row)
    .collect())
}
pub async fn rule(state: &AppState, table: &str, id: &str, default: Value) -> Result<Value> {
    Ok(sqlx::query_scalar::<_, Value>(&format!(
        "SELECT to_jsonb(t) FROM {table} t WHERE server_id=$1"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .map(crate::ai_evidence::row)
    .unwrap_or(default))
}
pub fn catalogue() -> Value {
    serde_json::from_str(include_str!("../../assets/code-plugins.json"))
        .expect("compiled plugin metadata")
}
pub fn causes() -> Vec<Value> {
    serde_json::from_str(include_str!("../../assets/causes.json")).expect("cause metadata")
}
pub async fn load(c: &mut Context) -> Result<Value> {
    let a = c.actor().await?;
    let id = c.param("id").to_owned();
    let source = c.input.source.clone();
    let s = auth::server_scope(
        &c.state,
        &a,
        &id,
        if source.contains("/settings/") {
            "server.view"
        } else {
            "automation.manage"
        },
    )
    .await?;
    if source == "src/routes/(app)/server/[id]/settings/+page.server.ts" {
        let mut channels = vec![];
        if s.manager {
            let v = c
                .get(&format!("/api/orgs/{}/webhooks", super::segment(&s.org_id)))
                .await?;
            channels = v["webhooks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|w| {
                    (w["statusEnabled"] == true
                        || w["events"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|v| v == "teamkills")))
                        && crate::webhooks::scope(&w["serverIds"], Some(&id))
                })
                .cloned()
                .collect();
        }
        return Ok(
            json!({"owner":s.manager,"https":c.state.config.origin.starts_with("https:"),"origin":c.state.config.origin,"channels":channels}),
        );
    }
    let status: Option<Value> =
        sqlx::query_scalar("SELECT status FROM server_live WHERE server_id=$1")
            .bind(&id)
            .fetch_optional(&c.state.db)
            .await?;
    let status = status.unwrap_or(Value::Null);
    let teams = status["scores"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["name"].as_str())
        .collect::<Vec<_>>();
    let faction = rule(
        &c.state,
        "faction_lock_rules",
        &id,
        json!({"enabled":false,"graceSeconds":120,"capacities":{}}),
    )
    .await?;
    let faction =
        json!({"rule":faction,"events":rows(&c.state,"faction_lock_events",&id,"id",50).await?});
    if source == "src/routes/(app)/server/[id]/faction-lock/+page.server.ts" {
        let mut v = faction;
        v["teams"] = json!(teams);
        return Ok(v);
    }
    if source != "src/routes/(app)/server/[id]/automation/+page.server.ts" {
        return Err(ApiError::missing());
    }
    let numeric=rule(&c.state,"numeric_limit_rules",&id,json!({"config":{"enabled":false,"kpm":null,"kd":null,"cash":null,"windowSeconds":180,"minKills":10}})).await?;
    let mut runs = rows(&c.state, "skill_balance_runs", &id, "created_at", 20).await?;
    for r in &mut runs {
        let at = crate::integrity_enforcement::date(&r["updatedAt"])
            .map(|t| t.timestamp_millis())
            .unwrap_or(0);
        if r["state"] == "executing" && Utc::now().timestamp_millis() - at > 120000 {
            r["state"] = json!("unknown");
        }
        if let Some(m) = r["moves"].as_array_mut() {
            for m in m {
                let since = crate::integrity_enforcement::date(&m["attemptedAt"])
                    .map(|t| t.timestamp_millis())
                    .unwrap_or(at);
                if m["state"] == "sending" && Utc::now().timestamp_millis() - since > 120000 {
                    m["state"] = json!("unknown");
                    m["reason"] = json!("发送过程已中断，请人工核对；不会自动重试");
                }
            }
        }
    }
    let weapon = rule(
        &c.state,
        "weapon_restriction_rules",
        &id,
        json!({"enabled":false,"causes":[],"groups":[]}),
    )
    .await?;
    let observed:Vec<Option<String>>=sqlx::query_scalar("SELECT DISTINCT cause FROM kills WHERE server_id=$1 AND ts>=now()-interval '30 days' LIMIT 2001").bind(&id).fetch_all(&c.state.db).await?;
    let known = causes();
    let mut all = std::collections::BTreeSet::new();
    all.extend(
        known
            .iter()
            .filter_map(|v| v["cause"].as_str().map(str::to_owned)),
    );
    all.extend(observed.iter().flatten().cloned());
    all.extend(
        weapon["causes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned)),
    );
    let catalogue=all.into_iter().map(|cause|json!({"label":known.iter().find(|v|v["cause"]==cause).map(|v|v["label"].clone()).unwrap_or_else(||json!(cause)),"cause":cause})).collect::<Vec<_>>();
    let feed: bool =
        sqlx::query_scalar("SELECT feed_token_hash IS NOT NULL FROM servers WHERE id=$1")
            .bind(&id)
            .fetch_one(&c.state.db)
            .await?;
    Ok(
        json!({"factionQuota":crate::faction_quota::view(&c.state,&id).await?,"factionScores":status["scores"],"groupControl":crate::group_control::view(&c.state,&id).await?,"numericLimits":{"config":numeric["config"],"events":rows(&c.state,"numeric_limit_events",&id,"created_at",50).await?},"skillBalance":{"executionAvailable":true,"executionBlock":"","rule":rule(&c.state,"skill_balance_rules",&id,json!({"enabled":false,"graceSeconds":300,"leadPoints":40})).await?,"runs":runs},"factionLock":faction,"weaponRestriction":{"rule":{"enabled":weapon["enabled"],"causes":weapon["causes"],"groups":weapon["groups"]},"events":rows(&c.state,"weapon_restriction_events",&id,"created_at",50).await?,"catalogue":catalogue,"catalogueTruncated":observed.len()>2000},"teams":teams,"triggers":crate::trigger_store::list(&c.state,&id).await?,"steam":std::env::var("STEAM_API_KEY").is_ok_and(|s|!s.is_empty()),"feed":feed}),
    )
}
