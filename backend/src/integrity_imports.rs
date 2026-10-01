//! Human-approved external JSON/JSONL history; never inserts into the live feed.
use crate::{
    audit,
    auth::Actor,
    config::AppState,
    error::{ApiError, Result},
    integrity_weapons,
};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet};
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalKill {
    pub event_id: String,
    pub event_at: DateTime<Utc>,
    pub instance_id: String,
    pub match_id: String,
    pub event_time: f64,
    pub map: String,
    pub killer_steam_id: String,
    pub victim_steam_id: String,
    pub killer_faction: String,
    pub victim_faction: String,
    pub cause: String,
    pub distance_m: Option<f64>,
    pub headshot: bool,
    pub penetration: bool,
    pub player_count: Option<i32>,
}
pub fn valid_id(s: &str, max: usize, extras: &[u8]) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || extras.contains(&c))
}
pub fn parse(raw: &str, now: DateTime<Utc>) -> Result<Vec<ExternalKill>> {
    if raw.trim().is_empty() || raw.len() > MAX_BYTES || raw.contains('\u{fffd}') {
        return Err(ApiError::bad("文件为空、超过 8 MiB，或不是有效 UTF-8。"));
    }
    let parsed: Vec<Value> = match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(a)) => a,
        Ok(v) => v["events"].as_array().cloned().unwrap_or_else(|| vec![v]),
        Err(_) => raw
            .lines()
            .filter(|s| !s.trim().is_empty())
            .enumerate()
            .map(|(i, s)| {
                serde_json::from_str(s)
                    .map_err(|_| ApiError::bad(format!("第 {} 行不是合法 JSON。", i + 1)))
            })
            .collect::<Result<_>>()?,
    };
    if parsed.is_empty() || parsed.len() > 10000 {
        return Err(ApiError::bad("每个文件需要 1～10000 条记录。"));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (i, v) in parsed.into_iter().enumerate() {
        let fail = |field: &str| ApiError::bad(format!("第 {} 条的 {field} 无效。", i + 1));
        let row: ExternalKill = serde_json::from_value(v.clone()).map_err(|_| fail("字段格式"))?;
        for (name, s) in [
            ("eventId", &row.event_id),
            ("instanceId", &row.instance_id),
            ("matchId", &row.match_id),
            ("map", &row.map),
            ("killerFaction", &row.killer_faction),
            ("victimFaction", &row.victim_faction),
        ] {
            if !valid_id(s, 80, b"._:-") {
                return Err(fail(name));
            }
        }
        if !seen.insert(row.event_id.clone()) {
            return Err(fail("eventId 重复"));
        }
        if !v["eventAt"].as_str().is_some_and(|s| s.ends_with('Z'))
            || row.event_at < now - Duration::days(30)
            || row.event_at > now - Duration::minutes(10)
        {
            return Err(ApiError::bad(format!(
                "第 {} 条 eventAt 必须是最近30天、至少10分钟前的 UTC 时间。",
                i + 1
            )));
        }
        if !row.event_time.is_finite() || !(0. ..=86400.).contains(&row.event_time) {
            return Err(fail("eventTime 本局秒数"));
        }
        for (name, s) in [
            ("killerSteamId", &row.killer_steam_id),
            ("victimSteamId", &row.victim_steam_id),
        ] {
            if s.len() != 17 || !s.bytes().all(|c| c.is_ascii_digit()) {
                return Err(fail(name));
            }
        }
        if row.killer_steam_id == row.victim_steam_id || row.killer_faction == row.victim_faction {
            return Err(fail("自杀或同阵营事件"));
        }
        if !row.cause.strip_prefix("Id.Item.").is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        }) {
            return Err(fail("cause"));
        }
        if row
            .distance_m
            .is_some_and(|n| !n.is_finite() || !(0. ..=5000.).contains(&n))
        {
            return Err(fail("distanceM 必须为0～5000米或null"));
        }
        if row.player_count.is_some_and(|n| !(1..=200).contains(&n)) {
            return Err(fail("playerCount"));
        }
        out.push(row)
    }
    Ok(out)
}
pub async fn list(state: &AppState, org: &str) -> Result<Vec<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(b) FROM integrity_import_batches b WHERE org_id=$1 ORDER BY staged_at DESC LIMIT 50").bind(org).fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(crate::ai_evidence::row).collect())
}
pub async fn stage(
    state: &AppState,
    actor: &Actor,
    headers: &HeaderMap,
    org: &str,
    source: &str,
    raw: &str,
) -> Result<Value> {
    if source.len() < 3 || !valid_id(source, 80, b"._-") {
        return Err(ApiError::bad(
            "来源服务器标识只能包含3～80位字母、数字、点、下划线和连字符。",
        ));
    }
    let rows = parse(raw, Utc::now())?;
    let overrides = integrity_weapons::overrides(&state.db, org).await?;
    for (i, row) in rows.iter().enumerate() {
        if integrity_weapons::classify(
            &json!({"cause":row.cause,"tags":[],"suicide":false}),
            &overrides,
        ) != "INFANTRY"
        {
            return Err(ApiError::bad(format!(
                "第 {} 条 cause 不是已确认的步兵枪械；请先在武器映射中审核。",
                i + 1
            )));
        }
    }
    let hash = crate::crypto::hash_token(raw);
    let id = uuid::Uuid::new_v4().to_string();
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("integrity-import:{org}"))
        .execute(&mut *tx)
        .await?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM integrity_import_batches WHERE org_id=$1 AND file_sha256=$2 LIMIT 1",
    )
    .bind(org)
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(id) = existing {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "import_duplicate",
            format!("文件已上传，批次 {id}。"),
        ));
    }
    let event_ids: Vec<_> = rows.iter().map(|r| r.event_id.clone()).collect();
    let duplicate:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_import_kills WHERE org_id=$1 AND source_server=$2 AND event_id=ANY($3))").bind(org).bind(source).bind(event_ids).fetch_one(&mut *tx).await?;
    if duplicate {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "import_event_duplicate",
            "此来源已有相同 eventId，整个批次未保存。",
        ));
    }
    let first = rows.iter().map(|r| r.event_at).min().unwrap();
    let last = rows.iter().map(|r| r.event_at).max().unwrap();
    sqlx::query("INSERT INTO integrity_import_batches(id,org_id,source_server,file_sha256,status,row_count,first_event_at,last_event_at,staged_by)VALUES($1,$2,$3,$4,'STAGED',$5,$6,$7,$8)").bind(&id).bind(org).bind(source).bind(&hash).bind(rows.len() as i32).bind(first).bind(last).bind(&actor.id).execute(&mut *tx).await?;
    for chunk in rows.chunks(500) {
        let data = json!(chunk);
        sqlx::query("INSERT INTO integrity_import_kills(batch_id,org_id,source_server,event_id,event_at,instance_id,match_id,event_time,map,killer_steam_id,victim_steam_id,killer_faction,victim_faction,cause,distance_m,headshot,penetration,player_count)SELECT $1,$2,$3,p.\"eventId\",p.\"eventAt\",p.\"instanceId\",p.\"matchId\",p.\"eventTime\",p.map,p.\"killerSteamId\",p.\"victimSteamId\",p.\"killerFaction\",p.\"victimFaction\",p.cause,p.\"distanceM\",p.headshot,p.penetration,p.\"playerCount\" FROM jsonb_to_recordset($4) p(\"eventId\" text,\"eventAt\" timestamptz,\"instanceId\" text,\"matchId\" text,\"eventTime\" real,map text,\"killerSteamId\" text,\"victimSteamId\" text,\"killerFaction\" text,\"victimFaction\" text,cause text,\"distanceM\" real,headshot boolean,penetration boolean,\"playerCount\" int)").bind(&id).bind(org).bind(source).bind(data).execute(&mut *tx).await?;
    }
    audit::org_event(
        &mut tx,
        actor,
        org,
        headers,
        "integrity.import.stage",
        "",
        json!({"batchId":id,"sourceServer":source,"sha256":hash,"rows":rows.len()}),
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"id":id,"sourceServer":source,"status":"STAGED","rowCount":rows.len(),"firstEventAt":first.to_rfc3339_opts(SecondsFormat::Millis,true),"lastEventAt":last.to_rfc3339_opts(SecondsFormat::Millis,true),"weapons":rows.iter().map(|r|r.cause.clone()).collect::<BTreeSet<_>>(),"maps":rows.iter().map(|r|r.map.clone()).collect::<BTreeSet<_>>(),"populationKnown":rows.iter().filter(|r|r.player_count.is_some()).count()}),
    )
}
pub async fn review(
    state: &AppState,
    actor: &Actor,
    headers: &HeaderMap,
    org: &str,
    id: &str,
    decision: &str,
) -> Result<Value> {
    if !["APPROVED", "REJECTED"].contains(&decision) {
        return Err(ApiError::bad("decision 必须是 APPROVED 或 REJECTED。"));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("integrity-import:{org}"))
        .execute(&mut *tx)
        .await?;
    let status: String = sqlx::query_scalar(
        "SELECT status FROM integrity_import_batches WHERE id=$1 AND org_id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::missing)?;
    if status == decision || status == "REJECTED" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "import_reviewed",
            "该批次已审核，不能重复批准。",
        ));
    }
    let batch:Value=sqlx::query_scalar("UPDATE integrity_import_batches SET status=$3,reviewed_at=now(),reviewed_by=$4 WHERE id=$1 AND org_id=$2 RETURNING to_jsonb(integrity_import_batches)").bind(id).bind(org).bind(decision).bind(&actor.id).fetch_one(&mut *tx).await?;
    // Revocation and invalidation commit together so an old external baseline cannot leak.
    if decision == "REJECTED" {
        sqlx::query("DELETE FROM integrity_baselines WHERE org_id=$1 AND source='external'")
            .bind(org)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE integrity_model_state SET baseline_status='STALE',active_baseline_generation=NULL,updated_at=now() WHERE org_id=$1").bind(org).execute(&mut *tx).await?;
    }
    audit::org_event(
        &mut tx,
        actor,
        org,
        headers,
        if decision == "APPROVED" {
            "integrity.import.approved"
        } else {
            "integrity.import.rejected"
        },
        id,
        json!({"batchId":id,"sourceServer":batch["source_server"],"rows":batch["row_count"]}),
    )
    .await?;
    sqlx::query("SELECT pg_notify('warcon_wake',$1)")
        .bind(json!({"baselines":org}).to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut batch = crate::ai_evidence::row(batch);
    if decision == "APPROVED" {
        batch["baselineRebuild"] = match crate::integrity_baseline_db::refresh(state, org).await {
            Ok(n) if n > 0 => json!({"status":"ready","cohorts":n}),
            _ => {
                json!({"status":"pending","message":"批准已保存；样本尚不足或重建暂未完成。统计模式后台会继续重建。"})
            }
        };
    }
    Ok(batch)
}
