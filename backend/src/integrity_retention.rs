//! Fenced, bounded retention with the original active-ban, delivery and appeal protections.
use crate::{
    audit,
    auth::Actor,
    config::AppState,
    error::{ApiError, Result},
    integrity_enforcement::date,
};
use axum::http::{HeaderMap, StatusCode};
use chrono::{Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub auto_delete_enabled: bool,
    pub page_size: i64,
    pub max_records: i64,
    pub max_age_days: i64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            auto_delete_enabled: false,
            page_size: 20,
            max_records: 2000,
            max_age_days: 90,
        }
    }
}
pub fn parse(value: &Value) -> Option<Policy> {
    let p: Policy = serde_json::from_value(value.clone()).ok()?;
    if ![10, 20, 50, 100].contains(&p.page_size)
        || !(100..=100000).contains(&p.max_records)
        || !(7..=3650).contains(&p.max_age_days)
    {
        None
    } else {
        Some(p)
    }
}
pub async fn policy(state: &AppState, server: &str) -> Result<Value> {
    let key = format!("integrityRetention:{server}");
    let rows: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM site_settings s WHERE key=ANY($1)")
            .bind(vec![
                key.clone(),
                format!("integrityRetentionLast:{server}"),
            ])
            .fetch_all(&state.db)
            .await?;
    let setting = rows.iter().find(|r| r["key"] == key);
    let last = rows.iter().find(|r| r["key"] != key).map(|r| &r["value"]);
    Ok(
        json!({"policy":setting.and_then(|r|parse(&r["value"])).unwrap_or_default(),"revision":setting.and_then(|r|date(&r["updated_at"])).map(|t|t.to_rfc3339_opts(SecondsFormat::Millis,true)).unwrap_or_default(),"lastCleanup":last.filter(|v|v["at"].is_string()).map(|v|json!({"at":v["at"],"removed":(["actions","modelRuns","short","cases"].iter().map(|k|v[*k].as_u64().unwrap_or(0)).sum::<u64>())}))}),
    )
}
pub async fn save(
    state: &AppState,
    actor: &Actor,
    headers: &HeaderMap,
    org: &str,
    server: &str,
    input: &Value,
    revision: &str,
) -> Result<Value> {
    let policy = parse(input).ok_or_else(|| {
        ApiError::bad("每页支持10／20／50／100条；保留上限100–100000条，时间7–3650天。")
    })?;
    let key = format!("integrityRetention:{server}");
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    let old: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT updated_at FROM site_settings WHERE key=$1 FOR UPDATE")
            .bind(&key)
            .fetch_optional(&mut *tx)
            .await?;
    if old
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_default()
        != revision
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "retention_revision",
            "保留设置已更新，请刷新后重试。",
        ));
    }
    let now = Utc::now();
    let updated = old
        .map(|t| (t + Duration::milliseconds(1)).max(now))
        .unwrap_or(now);
    let updated = chrono::DateTime::from_timestamp_millis(updated.timestamp_millis()).unwrap();
    sqlx::query("INSERT INTO site_settings(key,value,updated_at)VALUES($1,$2,$3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at").bind(key).bind(json!(policy)).bind(updated).execute(&mut *tx).await?;
    audit::org_event(
        &mut tx,
        actor,
        org,
        headers,
        "integrity.historyRetention",
        server,
        json!(policy),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"policy":policy,"revision":updated.to_rfc3339_opts(SecondsFormat::Millis,true)}))
}
const QUERIES: &[(u8, &str)] = &[
    (3, include_str!("../sql/retention/3.sql")),
    (4, include_str!("../sql/retention/4.sql")),
    (5, include_str!("../sql/retention/5.sql")),
    (6, include_str!("../sql/retention/6.sql")),
    (7, include_str!("../sql/retention/7.sql")),
    (8, include_str!("../sql/retention/8.sql")),
    (9, include_str!("../sql/retention/9.sql")),
    (10, include_str!("../sql/retention/10.sql")),
    (11, include_str!("../sql/retention/11.sql")),
    (12, include_str!("../sql/retention/12.sql")),
    (13, include_str!("../sql/retention/13.sql")),
    (14, include_str!("../sql/retention/14.sql")),
    (15, include_str!("../sql/retention/15.sql")),
    (16, include_str!("../sql/retention/16.sql")),
    (17, include_str!("../sql/retention/17.sql")),
    (18, include_str!("../sql/retention/18.sql")),
    (19, include_str!("../sql/retention/19.sql")),
    (20, include_str!("../sql/retention/20.sql")),
    (21, include_str!("../sql/retention/21.sql")),
    (22, include_str!("../sql/retention/22.sql")),
    (23, include_str!("../sql/retention/23.sql")),
    (24, include_str!("../sql/retention/24.sql")),
];
pub async fn prune(state: &AppState, server: &str) -> Result<Option<Value>> {
    tokio::time::timeout(
        std::time::Duration::from_secs(8),
        prune_inner(state, server),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "retention_timeout",
            "本次清理超时并已回滚，后台稍后重试。",
        )
    })?
}
async fn prune_inner(state: &AppState, server: &str) -> Result<Option<Value>> {
    let mut tx = state.worker_transaction().await?;
    let key = format!("integrityRetention:{server}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    let setting: Option<Value> =
        sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1 FOR UPDATE")
            .bind(&key)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(policy) = setting
        .as_ref()
        .and_then(parse)
        .filter(|p| p.auto_delete_enabled)
    else {
        return Ok(None);
    };
    // Bound lock time so lease renewal cannot be held behind a large historical scan.
    sqlx::query("SET LOCAL statement_timeout='8s'")
        .execute(&mut *tx)
        .await?;
    let mut result = json!({"actions":0,"modelRuns":0,"short":0,"cases":0});
    for (index, sql) in QUERIES {
        let mut query = sqlx::query(sql);
        if sql.contains("$1") {
            query = query.bind(server)
        }
        if sql.contains("$2") {
            query = query.bind(policy.max_records)
        }
        if sql.contains("$3") {
            query = query.bind(policy.max_age_days)
        }
        let count = query.execute(&mut *tx).await?.rows_affected();
        match index {
            17 => result["actions"] = json!(count),
            18 => result["modelRuns"] = json!(count),
            19 => result["short"] = json!(count),
            24 => result["cases"] = json!(count),
            _ => {}
        }
    }
    let mut stored = result.clone();
    stored["at"] = json!(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
    sqlx::query("INSERT INTO site_settings(key,value,updated_at)VALUES($1,$2,now())ON CONFLICT(key)DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at").bind(format!("integrityRetentionLast:{server}")).bind(stored).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Some(result))
}
pub async fn run(state: AppState) -> Result<()> {
    tokio::select! {_=state.runtime.stop.cancelled()=>return Ok(()),_=tokio::time::sleep(std::time::Duration::from_secs(30))=>{}}
    let mut clock = tokio::time::interval(std::time::Duration::from_secs(3600));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=state.runtime.stop.cancelled()=>break,_=clock.tick()=>{let servers:Vec<String>=sqlx::query_scalar("SELECT s.id FROM servers s JOIN site_settings p ON p.key='integrityRetention:'||s.id WHERE p.value->'autoDeleteEnabled'='true'::jsonb").fetch_all(&state.db).await?;for server in servers{if state.runtime.stop.is_cancelled(){break}if let Err(e)=prune(&state,&server).await{tracing::warn!(code=e.code,"Integrity retention pass failed")}}}}
    }
    Ok(())
}
