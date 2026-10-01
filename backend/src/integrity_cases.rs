//! Immutable evidence and append-only human review; punishment and review save atomically.
use crate::{
    api::lists,
    audit,
    auth::Actor,
    error::{ApiError, Result},
    integrity_decisions,
    integrity_statistics::camel_row,
    integrity_windows::Finding,
};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
pub const LABELS: &[&str] = &[
    "FALSE_POSITIVE",
    "CONFIRMED_ABUSE",
    "INSUFFICIENT_EVIDENCE",
    "DATA_ERROR",
];
fn iso(value: &Value) -> Option<String> {
    value
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Millis, true))
}
pub fn row_view(row: Value) -> Value {
    let mut row = camel_row(row);
    for key in [
        "ts",
        "createdAt",
        "reviewedAt",
        "effectiveAt",
        "expiresAt",
        "revertedAt",
    ] {
        if let Some(value) = iso(&row[key]) {
            row[key] = json!(value);
        }
    }
    row
}
pub struct Freeze<'a> {
    pub org: &'a str,
    pub server: &'a str,
    pub steam: &'a str,
    pub finding: &'a Finding,
    pub score: &'a Value,
    pub signals: &'a Value,
    pub steam_known: bool,
    pub version: i32,
    pub rules: &'a Value,
    pub created: DateTime<Utc>,
    pub score_id: Option<i64>,
    pub statistical: Option<&'a Value>,
    pub trigger: Option<&'a str>,
}
pub async fn freeze(tx: &mut Transaction<'_, Postgres>, input: &Freeze<'_>) -> Result<String> {
    let event_ids: Vec<_> = input.finding.event_ids.iter().take(200).cloned().collect();
    let first: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT min(ts) FROM kills WHERE server_id=$1 AND instance_id=$2 AND event_id=ANY($3)",
    )
    .bind(input.server)
    .bind(&input.finding.instance_id)
    .bind(&event_ids)
    .fetch_one(&mut **tx)
    .await?;
    let first = first.unwrap_or(input.created).min(input.created);
    let clock_from = (input.finding.clock_to - 180.).min(
        input
            .statistical
            .and_then(|s| s.pointer("/sustainedKpm/windows/0/from"))
            .and_then(Value::as_f64)
            .unwrap_or(input.finding.clock_to - 180.),
    );
    let events:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND instance_id=$2 AND map=$3 AND event_time>=$4 AND event_time<=$5 AND ts>=$6 AND ts<=$7 ORDER BY ts,event_time LIMIT 1001").bind(input.server).bind(&input.finding.instance_id).bind(&input.finding.map).bind(clock_from).bind(input.finding.clock_to).bind(first-chrono::Duration::seconds(240)).bind(input.created).fetch_all(&mut **tx).await?;
    let found: Vec<_> = events
        .iter()
        .filter_map(|row| {
            row["event_id"]
                .as_str()
                .filter(|id| event_ids.iter().any(|e| e == id))
                .map(str::to_owned)
        })
        .collect();
    let confidence = if input.finding.event_ids.len() > event_ids.len() || events.len() > 1000 {
        "C"
    } else {
        integrity_decisions::confidence(&input.finding.event_ids, &found)
    };
    let id = format!(
        "CASE-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..16].to_ascii_uppercase()
    );
    let f = input.finding;
    let s = input.signals;
    let snapshot = json!({"instanceId":f.instance_id,"roundId":f.round_id,"map":f.map,"clockFrom":f.clock_from,"clockTo":f.clock_to,"infantryKills":f.infantry_kills,"kpm180":f.kpm180,"uniqueVictims":f.unique_victims,"headshots":f.headshots,"headshotPct":f.headshot_pct,"penetrations":f.penetrations,"penetrationPct":f.penetration_pct,"burstPoints":f.burst_points,"maxKills15s":f.max_kills15s,"medianKillInterval":f.median_kill_interval,"behaviorReasons":f.reasons,"steamBansKnown":input.steam_known,"vacBans":s["vacBans"],"gameBans":s["gameBans"],"daysSinceLastBan":s["daysSinceLastBan"],"repeatHighRiskWindow":s["repeatHighRiskWindow"],"uniqueReporters":s["uniqueReporters"],"ruleVersion":input.version,"eventIds":f.event_ids,"rules":input.rules});
    sqlx::query("INSERT INTO integrity_cases(id,org_id,server_id,steam_id,created_at,confidence,trigger,rule_version,risk_score,risk_breakdown,score_id,statistical,snapshot) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)").bind(&id).bind(input.org).bind(input.server).bind(input.steam).bind(input.created).bind(confidence).bind(input.trigger.unwrap_or("ABNORMAL_INFANTRY_WINDOW")).bind(input.version).bind(crate::integrity_score::num(input.score,"score")).bind(&input.score["breakdown"]).bind(input.score_id).bind(input.statistical).bind(snapshot).execute(&mut **tx).await?;
    let server_name: Option<String> = sqlx::query_scalar("SELECT name FROM servers WHERE id=$1")
        .bind(input.server)
        .fetch_optional(&mut **tx)
        .await?;
    let alert = json!({"caseId":id,"serverId":input.server,"serverName":server_name.unwrap_or_default(),"steamId":input.steam,"map":f.map,"score":input.score["score"],"level":input.score["level"],"statisticalLevel":input.statistical.and_then(|v|v["level"].as_str()),"breakdown":input.score["breakdown"],"infantryKills":f.infantry_kills,"kpm180":f.kpm180,"uniqueVictims":f.unique_victims,"uniqueReporters":s["uniqueReporters"],"createdAt":input.created});
    crate::webhooks::input(
        tx,
        &format!("case:{id}"),
        input.org,
        input.server,
        "integrity",
        &alert,
    )
    .await?;
    for row in events.into_iter().take(1000) {
        let row = camel_row(row);
        let mut event = json!({});
        for key in [
            "eventId",
            "ts",
            "eventTime",
            "map",
            "killerSteamId",
            "killerName",
            "killerFaction",
            "victimSteamId",
            "victimName",
            "victimFaction",
            "cause",
            "distanceM",
            "distanceInvalid",
            "rawDistanceCm",
            "headshot",
            "teamKill",
            "suicide",
            "tags",
        ] {
            event[key] = if key == "ts" {
                json!(iso(&row[key]))
            } else {
                row[key].clone()
            };
        }
        sqlx::query("INSERT INTO integrity_case_events(case_id,instance_id,event_id,event) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(&id).bind(&row["instanceId"].as_str().unwrap_or("")).bind(row["eventId"].as_str().unwrap_or("")).bind(event).execute(&mut **tx).await?;
    }
    Ok(id)
}
pub async fn review_penalty(
    tx: &mut Transaction<'_, Postgres>,
    case: &Value,
    actor: &Actor,
    review_reason: &str,
) -> Result<Value> {
    let case_id = case["id"].as_str().unwrap_or("");
    let id = format!("review-7d:{case_id}");
    let prior: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(a) FROM integrity_actions a WHERE id=$1")
            .bind(&id)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(prior) = prior {
        let mut prior = row_view(prior);
        prior["reused"] = json!(true);
        return Ok(prior);
    }
    let org = case["org_id"].as_str().unwrap_or("");
    let server = case["server_id"].as_str().unwrap_or("");
    let steam = case["steam_id"].as_str().unwrap_or("");
    let valid: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM servers WHERE id=$1 AND org_id=$2)")
            .bind(server)
            .bind(org)
            .fetch_one(&mut **tx)
            .await?;
    if !valid || steam.len() != 17 || !steam.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "case_identity",
            "案件服务器或玩家身份无效，审核和封禁均未保存。",
        ));
    }
    let list = lists::ensure(tx, org, Some(server), "ban").await?;
    let now = Utc::now();
    let requested = now + chrono::Duration::days(7);
    let reason = crate::feed::truncate(
        &format!("人工确认违规，封禁7天。案件 {case_id}：{review_reason}"),
        200,
    );
    let existing:Option<(String,Option<DateTime<Utc>>)>=sqlx::query_as("SELECT id,expires_at FROM list_entries WHERE list_id=$1 AND steam_id=$2 AND removed_at IS NULL FOR UPDATE").bind(&list).bind(steam).fetch_optional(&mut **tx).await?;
    let (entry_id, expires) = if let Some((entry_id, old)) = existing {
        let expires = if old.is_none_or(|t| t > requested) {
            old
        } else {
            sqlx::query("UPDATE list_entries SET expires_at=$2,reason=$3,added_by=$4,added_by_name=$5 WHERE id=$1").bind(&entry_id).bind(requested).bind(&reason).bind(&actor.id).bind(&actor.name).execute(&mut **tx).await?;
            Some(requested)
        };
        (entry_id, expires)
    } else {
        let entry_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO list_entries(id,list_id,steam_id,reason,expires_at,added_by,added_by_name) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(&entry_id).bind(&list).bind(steam).bind(&reason).bind(requested).bind(&actor.id).bind(&actor.name).execute(&mut **tx).await?;
        (entry_id, Some(requested))
    };
    sqlx::query("UPDATE lists SET updated_at=$2 WHERE id=$1")
        .bind(&list)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    let action:Value=sqlx::query_scalar("INSERT INTO integrity_actions(id,case_id,org_id,server_id,steam_id,action,source,list_entry_id,created_at,effective_at,expires_at,delivery_state) VALUES($1,$2,$3,$4,$5,'QUARANTINE_7D','REVIEW',$6,$7,$7,$8,'pending') RETURNING to_jsonb(integrity_actions)").bind(&id).bind(case_id).bind(org).bind(server).bind(steam).bind(&entry_id).bind(now).bind(expires).fetch_one(&mut **tx).await?;
    let params = json!({"steamId":steam,"reason":if expires.is_none_or(|t|t>requested){"人工确认违规，已有更长期封禁继续生效。"}else{&reason}});
    sqlx::query("INSERT INTO outbox(server_id,trigger_name,trigger_kind,action,params,target,steam_id,ok_message,detail,dedupe_key) VALUES($1,'人工案件确认违规','integrity','kick',$2,$3,$3,$4,$5,$6)").bind(server).bind(params).bind(steam).bind(format!("案件 {case_id}：人工确认违规并封禁7天")).bind(json!({"caseId":case_id,"actionId":id,"source":"REVIEW","reviewerId":actor.id,"reviewerName":actor.name,"reviewReason":review_reason})).bind(format!("integrity:{id}:kick")).execute(&mut **tx).await?;
    let mut action = row_view(action);
    action["reused"] = json!(false);
    Ok(action)
}
pub async fn label(
    state: &crate::config::AppState,
    headers: &HeaderMap,
    actor: &Actor,
    org: &str,
    case_id: &str,
    label: &Value,
    reason: &Value,
) -> Result<Value> {
    let label = label
        .as_str()
        .filter(|s| LABELS.contains(s))
        .ok_or_else(|| ApiError::bad("无效的案件标签。"))?;
    let reason = reason
        .as_str()
        .filter(|s| s.trim().encode_utf16().count() >= 5 && s.encode_utf16().count() <= 2000)
        .ok_or_else(|| ApiError::bad("请填写 5～2000 字的审核理由。"))?
        .trim();
    let mut tx = state.db.begin().await?;
    let case: Value = sqlx::query_scalar(
        "SELECT to_jsonb(c) FROM integrity_cases c WHERE id=$1 AND org_id=$2 FOR UPDATE",
    )
    .bind(case_id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::missing)?;
    if case["status"] == "AI_CLEARED" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "evidence_cleared",
            "案件证据已清理，不能据此执行人工处罚。",
        ));
    }
    let version = case["statistical"]["modelVersion"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("legacy-rules-{}", case["rule_version"]));
    let latest:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(l) FROM integrity_labels l WHERE case_id=$1 AND org_id=$2 ORDER BY created_at DESC,id DESC LIMIT 1").bind(case_id).bind(org).fetch_optional(&mut *tx).await?;
    let saved = if let Some(latest) = latest
        .filter(|l| l["label"] == label && l["reason"] == reason && l["reviewer_id"] == actor.id)
    {
        latest
    } else {
        sqlx::query_scalar::<_,Value>("INSERT INTO integrity_labels(case_id,org_id,label,reason,reviewer_id,model_version) VALUES($1,$2,$3,$4,$5,$6) RETURNING to_jsonb(integrity_labels)").bind(case_id).bind(org).bind(label).bind(reason).bind(&actor.id).bind(version).fetch_one(&mut *tx).await?
    };
    let at = DateTime::parse_from_rfc3339(saved["created_at"].as_str().unwrap())
        .map_err(|_| ApiError::bad("无效审核时间。"))?
        .to_utc();
    sqlx::query(
        "UPDATE integrity_cases SET status='REVIEWED',reviewed_by=$2,reviewed_at=$3 WHERE id=$1",
    )
    .bind(case_id)
    .bind(&actor.id)
    .bind(at)
    .execute(&mut *tx)
    .await?;
    let penalty = if label == "CONFIRMED_ABUSE" {
        Some(review_penalty(&mut tx, &case, actor, reason).await?)
    } else {
        None
    };
    audit::org_event(&mut tx,actor,org,headers,"integrity.case.label","",json!({"caseId":case_id,"label":label,"labelId":saved["id"],"actionId":penalty.as_ref().map(|p|&p["id"]),"expiresAt":penalty.as_ref().map(|p|&p["expiresAt"]),"source":penalty.as_ref().map(|_|"REVIEW")})).await?;
    tx.commit().await?;
    let mut saved = row_view(saved);
    saved["penalty"] = json!(penalty);
    Ok(saved)
}
