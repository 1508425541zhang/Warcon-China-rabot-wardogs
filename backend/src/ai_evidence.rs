//! Lossless JSON evidence, resolved using frozen identities rather than map names.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    integrity_cases::row_view,
    integrity_enforcement::date,
};
use chrono::{Duration, SecondsFormat};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub fn compact_table(records: &[Value]) -> Value {
    let columns: BTreeSet<String> = records
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|r| r.keys().cloned())
        .collect();
    let mut dict: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &columns {
        if records.iter().any(|r| r[c].is_string())
            && records.iter().all(|r| r[c].is_null() || r[c].is_string())
        {
            let mut values = Vec::new();
            for r in records {
                if let Some(s) = r[c].as_str() {
                    if !values.iter().any(|v| v == s) {
                        values.push(s.to_owned())
                    }
                }
            }
            dict.insert(c.clone(), values);
        }
    }
    let rows: Vec<Vec<Value>> = records
        .iter()
        .map(|r| {
            columns
                .iter()
                .map(|c| match r.get(c) {
                    None => json!({"$missing":true}),
                    Some(Value::String(s)) if dict.contains_key(c) => {
                        json!(dict[c].iter().position(|v| v == s).unwrap())
                    }
                    Some(v) => v.clone(),
                })
                .collect()
        })
        .collect();
    json!({"columns":columns,"dictionaries":dict,"rows":rows,"count":records.len()})
}
pub fn row(v: Value) -> Value {
    let mut v = row_view(v);
    if let Some(m) = v.as_object_mut() {
        for (key, value) in m {
            if key.ends_with("At")
                || [
                    "ts",
                    "joinedAt",
                    "lastSeen",
                    "lastRequestAt",
                    "leaseUntil",
                    "expiresAt",
                ]
                .contains(&key.as_str())
            {
                if let Some(d) = date(value) {
                    *value = json!(d.to_rfc3339_opts(SecondsFormat::Millis, true))
                }
            }
        }
    }
    v
}
pub async fn bundle(state: &AppState, org: &str, server: &str, id: &str) -> Result<Value> {
    let mut tx = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let c: Value = sqlx::query_scalar(
        "SELECT to_jsonb(c) FROM integrity_cases c WHERE id=$1 AND org_id=$2 AND server_id=$3",
    )
    .bind(id)
    .bind(org)
    .bind(server)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::missing)?;
    let c = row(c);
    if c["status"] == "AI_CLEARED" {
        return Err(ApiError::new(
            axum::http::StatusCode::GONE,
            "ai_cleared",
            "低风险案件已清理，仅保留审核回执，不再发送不完整证据。",
        ));
    }
    let steam = c["steamId"].as_str().ok_or_else(ApiError::missing)?;
    let saved:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('instanceId',instance_id,'eventId',event_id,'event',event) FROM integrity_case_events WHERE case_id=$1 ORDER BY instance_id,event_id").bind(id).fetch_all(&mut *tx).await?;
    let reviews:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('label',label,'reason',reason,'createdAt',created_at) FROM integrity_labels WHERE case_id=$1 AND org_id=$2 ORDER BY id").bind(id).bind(org).fetch_all(&mut *tx).await?;
    let actions: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(a) FROM integrity_actions a WHERE case_id=$1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let history:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'createdAt',created_at,'riskScore',risk_score,'status',status) FROM integrity_cases WHERE org_id=$1 AND server_id=$2 AND steam_id=$3 ORDER BY created_at DESC LIMIT 21").bind(org).bind(server).bind(steam).fetch_all(&mut *tx).await?;
    let player:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',steam_id,'name',persona,'fetchedAt',fetched_at) FROM steam_profiles WHERE steam_id=$1").bind(steam).fetch_optional(&mut *tx).await?;
    let instance = c["snapshot"]["instanceId"].as_str();
    let clock = c["snapshot"]["clockTo"].as_f64().filter(|n| n.is_finite());
    let ids: Vec<String> = saved
        .iter()
        .filter(|r| instance.is_none_or(|i| r["instanceId"] == i))
        .filter_map(|r| r["eventId"].as_str().map(str::to_owned))
        .collect();
    let anchors: Vec<i64> = if let Some(instance) = instance {
        sqlx::query_scalar("SELECT DISTINCT match_row FROM kills WHERE server_id=$1 AND instance_id=$2 AND event_id=ANY($3) AND match_row IS NOT NULL").bind(server).bind(instance).bind(&ids).fetch_all(&mut *tx).await?
    } else {
        vec![]
    };
    let round: Option<Value> = if anchors.len() == 1 {
        sqlx::query_scalar("SELECT to_jsonb(m) FROM matches m WHERE id=$1 AND server_id=$2")
            .bind(anchors[0])
            .bind(server)
            .fetch_optional(&mut *tx)
            .await?
    } else {
        None
    };
    let round = round.map(row);
    let round_id = round.as_ref().and_then(|r| r["id"].as_i64());
    let case_at = date(&c["createdAt"]).ok_or_else(|| ApiError::bad("案件时间无效。"))?;
    let fallback = case_at - Duration::hours(24);
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND (CASE WHEN $2::bigint IS NOT NULL AND $3::text IS NOT NULL THEN match_row=$2 AND instance_id=$3 ELSE ts>=$4 AND ts<=$5 END) AND (killer_steam_id=$6 OR victim_steam_id=$6 OR ($2::bigint IS NOT NULL AND $7::double precision IS NOT NULL AND event_time>=greatest(0,$7-180) AND event_time<=$7)) ORDER BY ts,event_id").bind(server).bind(round_id).bind(instance).bind(fallback).bind(case_at).bind(steam).bind(clock).fetch_all(&mut *tx).await?;
    let rows: Vec<Value> = rows.into_iter().map(row).collect();
    let from = round
        .as_ref()
        .and_then(|r| date(&r["startedAt"]))
        .unwrap_or(fallback);
    let to = round
        .as_ref()
        .and_then(|r| date(&r["endedAt"]))
        .unwrap_or(case_at);
    let reports:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_reports r WHERE org_id=$1 AND server_id=$2 AND target_steam_id=$3 AND (case_id=$4 OR(created_at>=$5 AND created_at<=$6))").bind(org).bind(server).bind(steam).bind(id).bind(from).bind(to).fetch_all(&mut *tx).await?;
    let sessions:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s) FROM player_sessions s WHERE server_id=$1 AND steam_id=$2 AND last_seen>=$3 AND joined_at<=$4").bind(server).bind(steam).bind(from).bind(to).fetch_all(&mut *tx).await?;
    let progress:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(p) FROM player_progress_samples p WHERE server_id=$1 AND match_id=$2 ORDER BY observed_at").bind(server).bind(round_id).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let mut cash = Vec::new();
    for p in progress.into_iter().map(row) {
        if let Some(players) = p["players"].as_array() {
            for player in players {
                if player["steamId"] == steam {
                    if let Some(mut v) = player.as_object().cloned() {
                        v.entry("observedAt").or_insert(p["observedAt"].clone());
                        v.entry("matchId").or_insert(p["matchId"].clone());
                        cash.push(Value::Object(v))
                    }
                }
            }
        }
    }
    let identities: Vec<Value> = saved
        .iter()
        .map(|r| json!({"instanceId":r["instanceId"],"eventId":r["eventId"]}))
        .collect();
    let frozen: Vec<Value> = saved
        .iter()
        .map(|r| {
            if r["event"].is_object() {
                r["event"].clone()
            } else {
                json!({"value":r["event"]})
            }
        })
        .collect();
    let iso = |d: chrono::DateTime<chrono::Utc>| d.to_rfc3339_opts(SecondsFormat::Millis, true);
    let scope = if let Some(round) = round {
        json!({"kind":"resolved_round","round":round,"instanceId":instance,"caseAt":iso(case_at),"contextClockFrom":clock.map(|n|(n-180.).max(0.)),"contextClockTo":clock,"includes":"all recorded kills AND deaths of the subject in this round, including post-case events already received; plus every server kill in the case 180-second window"})
    } else {
        json!({"kind":"unresolved_round","from":iso(fallback),"to":iso(case_at),"includes":"all recorded kills AND deaths involving the subject in preceding 24 hours; round cannot be safely resolved, do not combine rounds"})
    };
    let evidence = json!({"encoding":"column-table-v1: rows[i][j] uses columns[j]; non-null values in dictionary columns are zero-based dictionary indices; {$missing:true} means absent, not null. Decode before calculating. All stored columns and rows preserved.","scope":scope,"frozenEvents":{"identities":compact_table(&identities),"data":compact_table(&frozen),"join":"identities.rows[i] corresponds to data.rows[i]"},"reports":compact_table(&reports.into_iter().map(row).collect::<Vec<_>>()),"sessions":{"scope":"session totals may span multiple rounds; not a substitute for round KPM","data":compact_table(&sessions.into_iter().map(row).collect::<Vec<_>>())},"cashObservations":compact_table(&cash),"combatEvents":compact_table(&rows),"counts":{"records":rows.len(),"subjectKills":rows.iter().filter(|r|r["killerSteamId"]==steam&&r["suicide"]!=true).count(),"subjectDeaths":rows.iter().filter(|r|r["victimSteamId"]==steam).count(),"subjectSuicides":rows.iter().filter(|r|r["victimSteamId"]==steam&&r["suicide"]==true).count(),"postCaseRecords":rows.iter().filter(|r|date(&r["ts"]).is_some_and(|d|d>case_at)).count()},"coverage":{"databaseRowsComplete":true,"gameFeedComplete":"unknown","duplicates":"records are preserved; deduplicate only on serverId+instanceId+eventId; differing versions must be flagged","latestReceivedAt":rows.last().map(|r|r["ts"].clone()),"nonFatalDamage":{"available":false,"events":null,"reason":"Current verified feed stores killed events only; no damage amounts, shots fired, hits or non-fatal injury events are collected. Kill distance is not damage. Do not infer accuracy or total damage."}}});
    Ok(
        json!({"schemaVersion":2,"player":player.map(row).unwrap_or(json!({"steamId":steam,"name":null})),"scope":"案件冻结资料＋玩家整局击杀死亡＋案件180秒全服交战背景；无法定位轮次时为案件前24小时玩家记录。逐条完整、无抽样；数据库记录完整不代表游戏回传无缺失。历史索引仍最多20条。","reviewPurpose":if c["trigger"]=="AI_SINGLE_SIGNAL"{"低信号AI预筛：至少一位专家可疑或极可能作弊，但未达到委员会正式建案门槛。请核对异常是否有数据支撑、是否有正常解释；生成预筛记录不代表确认违规，不得因为存在案件编号就提高可疑度。"}else{"正式案件初审：核对原始证据、数值与正常解释，提供管理员参考。"},"numericChecks":crate::ai_protocol::numeric_checks(&c["snapshot"]),"coverage":{"savedEvents":saved.len(),"expectedEventIds":c["snapshot"]["eventIds"]},"case":c,"evidence":evidence,"reviews":reviews.into_iter().map(row).collect::<Vec<_>>(),"actions":actions.into_iter().map(row).collect::<Vec<_>>(),"recentCases":history.iter().take(20).cloned().map(row).collect::<Vec<_>>(),"historyTruncated":history.len()>20}),
    )
}
