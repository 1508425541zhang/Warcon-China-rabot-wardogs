use super::{Context, charts, segment};
use crate::{
    auth, committee,
    config::AppState,
    error::{ApiError, Result},
    integrity_rules as rules,
};
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
const RETAINED: &str = "(coalesce(c.statistical->>'level' IN ('WATCH','KICK_CANDIDATE'),false) OR EXISTS(SELECT 1 FROM integrity_actions a WHERE a.case_id=c.id AND a.action IN ('KICK','QUARANTINE_24H','QUARANTINE_7D')))";
const CASE_FIELDS: &str = "jsonb_build_object('name',p.persona,'id',c.id,'steamId',c.steam_id,'createdAt',c.created_at,'status',c.status,'confidence',c.confidence,'riskScore',c.risk_score,'riskBreakdown',c.risk_breakdown,'statistical',c.statistical,'snapshot',c.snapshot,'trigger',c.trigger)";
const WARNING: f64 = 0.6848768837889491;
const KICK: f64 = 0.6939192028934524;
fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.)
}
pub fn empty_comparison() -> Value {
    json!({"total":0,"since":null,"normalNormal":0,"normalAbnormal":0,"abnormalNormal":0,"abnormalAbnormal":0})
}
async fn comparison(state: &AppState, org: &str, threshold: f64, enabled: bool) -> Result<Value> {
    if !enabled {
        return Ok(empty_comparison());
    }
    Ok(sqlx::query_scalar("WITH latest AS(SELECT DISTINCT ON(window_id) score,scored_at,statistical FROM integrity_scores WHERE org_id=$1 AND source='window' AND statistical IS NOT NULL AND scored_at>=now()-interval '30 days' ORDER BY window_id,id DESC) SELECT jsonb_build_object('since',min(scored_at),'total',count(*),'normalNormal',count(*) FILTER(WHERE score<$2 AND statistical->>'level'='NORMAL'),'normalAbnormal',count(*) FILTER(WHERE score<$2 AND statistical->>'level' IN ('WATCH','CASE','KICK_CANDIDATE')),'abnormalNormal',count(*) FILTER(WHERE score>=$2 AND statistical->>'level'='NORMAL'),'abnormalAbnormal',count(*) FILTER(WHERE score>=$2 AND statistical->>'level' IN ('WATCH','CASE','KICK_CANDIDATE'))) FROM latest WHERE statistical->>'status'='READY'").bind(org).bind(threshold).fetch_one(&state.db).await?)
}
pub fn shadow(rows: &[Value], labels: &[Value], truncated: bool) -> Value {
    const DECISIONS: [&str; 5] = [
        "NORMAL",
        "WATCH",
        "CASE",
        "KICK_CANDIDATE",
        "ESCALATION_CANDIDATE",
    ];
    const VERDICTS: [&str; 4] = ["NORMAL", "SUSPICIOUS", "CHEAT_LIKELY", "UNKNOWN"];
    let mut counts: Value =
        Value::Object(DECISIONS.iter().map(|d| ((*d).into(), json!(0))).collect());
    let mut models = json!({});
    let mut unknown = json!({});
    let mut seen = HashSet::new();
    let mut assessed = 0;
    let mut disagreement = 0;
    for row in rows {
        if row["statistical"]["modelVersion"] != committee::MODEL_VERSION {
            continue;
        }
        let Some(window) = row["windowId"].as_i64() else {
            continue;
        };
        if !seen.insert(window) {
            continue;
        }
        let c = &row["statistical"]["committee"];
        let Some(decision) = c["decision"].as_str().filter(|d| DECISIONS.contains(d)) else {
            continue;
        };
        if c["votingVersion"] != committee::VOTING_VERSION || !c["verdicts"].is_array() {
            continue;
        }
        counts[decision] = json!(counts[decision].as_i64().unwrap_or(0) + 1);
        assessed += 1;
        let mut active = HashSet::new();
        for v in c["verdicts"].as_array().unwrap() {
            let (Some(id), Some(d)) = (
                v["modelId"].as_str(),
                v["decision"].as_str().filter(|d| VERDICTS.contains(d)),
            ) else {
                continue;
            };
            if models.get(id).is_none() {
                models[id] =
                    Value::Object(VERDICTS.iter().map(|d| ((*d).into(), json!(0))).collect());
            }
            models[id][d] = json!(models[id][d].as_i64().unwrap_or(0) + 1);
            if d == "UNKNOWN" {
                let reasons: HashSet<&str> = v["reasons"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let reasons = if reasons.is_empty() {
                    HashSet::from(["UNKNOWN_REASON"])
                } else {
                    reasons
                };
                if unknown.get(id).is_none() {
                    unknown[id] = json!({});
                }
                for r in reasons {
                    unknown[id][r] = json!(unknown[id][r].as_i64().unwrap_or(0) + 1);
                }
            } else {
                active.insert(d);
            }
        }
        if active.len() > 1 {
            disagreement += 1;
        }
    }
    let mut latest = HashMap::new();
    for r in labels {
        if r["statistical"]["modelVersion"] == committee::MODEL_VERSION
            && r["statistical"]["committee"]["votingVersion"] == committee::VOTING_VERSION
        {
            if let (Some(id), Some(label)) = (r["caseId"].as_str(), r["label"].as_str()) {
                latest.entry(id).or_insert(label);
            }
        }
    }
    let mut abuse = 0;
    let mut false_positive = 0;
    for r in labels {
        if r["statistical"]["committee"]["decision"] != "KICK_CANDIDATE" {
            continue;
        }
        if let Some(label) = r["caseId"].as_str().and_then(|id| latest.remove(id)) {
            abuse += i32::from(label == "CONFIRMED_ABUSE");
            false_positive += i32::from(label == "FALSE_POSITIVE");
        }
    }
    json!({"modelVersion":committee::MODEL_VERSION,"votingVersion":committee::VOTING_VERSION,"counts":counts,"models":models,"unknownReasons":unknown,"assessed":assessed,"disagreement":disagreement,"disagreementRate":if assessed>0{Some(disagreement as f64/assessed as f64)}else{None},"confirmedAbuse":abuse,"falsePositive":false_positive,"truncated":truncated})
}
async fn shadow_view(state: &AppState, id: &str, enabled: bool) -> Result<Value> {
    if !enabled {
        return Ok(shadow(&[], &[], false));
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('windowId',window_id,'statistical',statistical) FROM integrity_scores WHERE server_id=$1 AND source='window' AND statistical->>'modelVersion'=$2 AND statistical->'committee'->>'votingVersion'=$3 AND scored_at>=now()-interval '30 days' ORDER BY scored_at DESC,id DESC LIMIT 5001").bind(id).bind(committee::MODEL_VERSION).bind(committee::VOTING_VERSION).fetch_all(&state.db).await?;
    let labels:Vec<Value>=sqlx::query_scalar(&format!("SELECT jsonb_build_object('caseId',l.case_id,'label',l.label,'reason',l.reason,'statistical',c.statistical,'createdAt',l.created_at) FROM integrity_labels l JOIN integrity_cases c ON c.id=l.case_id WHERE c.server_id=$1 AND {RETAINED} AND c.statistical->>'modelVersion'=$2 AND c.statistical->'committee'->>'votingVersion'=$3 AND l.created_at>=now()-interval '30 days' ORDER BY l.created_at DESC LIMIT 5001")).bind(id).bind(committee::MODEL_VERSION).bind(committee::VOTING_VERSION).fetch_all(&state.db).await?;
    Ok(shadow(
        &rows[..rows.len().min(5000)],
        &labels[..labels.len().min(5000)],
        rows.len() > 5000 || labels.len() > 5000,
    ))
}
fn empty_distributions() -> Value {
    json!({"status":"DISABLED","updatedAt":null,"lastFailureAt":null,"refreshMinutes":5,"dataBefore":null,"metrics":[]})
}
async fn distributions(state: &AppState, org: &str, id: &str, enabled: bool) -> Result<Value> {
    if !enabled {
        return Ok(empty_distributions());
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let state: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_model_state s WHERE org_id=$1")
            .bind(org)
            .fetch_optional(&mut *tx)
            .await?;
    let state = state.unwrap_or(Value::Null);
    let rows: Vec<Value> = if state["active_baseline_generation"].is_string() {
        sqlx::query_scalar("SELECT to_jsonb(b) FROM integrity_baselines b WHERE org_id=$1 AND generation=$2 AND model_version=$3 AND feature_version=$4 AND weapon_map_version=$5 AND level=3 AND (server_id=$6 OR server_id IS NULL) ORDER BY metric,weapon_category,source").bind(org).bind(state["active_baseline_generation"].as_str().unwrap()).bind(committee::MODEL_VERSION).bind(committee::FEATURE_VERSION).bind(state["weapon_map_version"].as_i64().unwrap_or(1)as i32).bind(id).fetch_all(&mut *tx).await?
    } else {
        vec![]
    };
    tx.commit().await?;
    let updated = crate::integrity_enforcement::date(&state["last_refresh_at"]);
    let stale = updated.is_none_or(|t| Utc::now() - t > chrono::Duration::hours(48));
    let metrics=rows.into_iter().filter(|r|r["metric"].as_str().is_some_and(|m|crate::integrity_statistics::METRICS.contains(&m))).map(|r|{let r=crate::ai_evidence::row(r);json!({"code":r["metric"],"serverId":r["serverId"],"source":r["source"],"value":null,"percentile":null,"extremenessPercentile":null,"robustZ":null,"tail":if r["metric"]=="medianKillInterval"{"lower"}else{"upper"},"median":r["median"],"mad":r["mad"],"p95":r["p95"],"p99":r["p99"],"p999":r["p999"],"sampleCount":r["sampleCount"],"uniquePlayers":r["uniquePlayers"],"uniquePlayerDays":r["uniquePlayerDays"],"effectiveSampleSize":r["effectiveSampleSize"],"modelVersion":r["modelVersion"],"featureVersion":r["featureVersion"],"weaponMapVersion":r["weaponMapVersion"],"baselineGeneration":r["generation"],"baselineId":r["id"],"map":r["map"],"populationBucket":r["populationBucket"],"weaponCategory":r["weaponCategory"],"windowDays":r["windowDays"],"calculatedAt":r["calculatedAt"],"histogram":r["histogram"]})}).collect::<Vec<_>>();
    Ok(
        json!({"status":if stale{"STALE"}else{state["baseline_status"].as_str().unwrap_or("STALE")},"updatedAt":updated,"lastFailureAt":state["last_failure_at"],"refreshMinutes":5,"dataBefore":updated.map(|t|t-chrono::Duration::minutes(10)),"metrics":metrics}),
    )
}
async fn short(state: &AppState, id: &str) -> Result<Value> {
    let snapshot: Option<Value> =
        sqlx::query_scalar("SELECT value FROM site_settings WHERE key='rust:shortRiskSnapshot'")
            .fetch_optional(&state.db)
            .await?;
    let s = snapshot.unwrap_or(Value::Null);
    let valid = s["model_sha256"]
        == "b4b68f4dc09b95d24e44d3ab70322f46fc78d50980d44a8e86409ace734649cc"
        && s["algorithm"] == "IsolationForest"
        && crate::integrity_enforcement::date(&s["evaluated_at"])
            .is_some_and(|t| (0..=30000).contains(&(Utc::now() - t).num_milliseconds()))
        && s["players"].is_array();
    let mut players = if valid {
        s["players"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["server_id"] == id && p["anomaly_score"].as_f64().is_some())
            .cloned()
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    for p in &mut players {
        p["level"] = json!(if num(&p["anomaly_score"]) > KICK {
            "kick"
        } else if num(&p["anomaly_score"]) > WARNING {
            "warning"
        } else {
            "normal"
        });
    }
    players.sort_by(|a, b| num(&b["anomaly_score"]).total_cmp(&num(&a["anomaly_score"])));
    Ok(
        json!({"available":valid,"evaluatedAt":if valid{s["evaluated_at"].clone()}else{Value::Null},"autoKick":std::env::var("WARCON_SHORT_RISK_ENABLED").is_ok_and(|v|v=="1"),"warningPercentile":99.6,"kickPercentile":99.9,"players":players,"history":[]}),
    )
}
async fn reviews(
    state: &AppState,
    org: &str,
    id: &str,
    ids: &[String],
) -> Result<(Vec<Value>, Vec<Value>)> {
    let labels=sqlx::query_scalar("SELECT jsonb_build_object('caseId',case_id,'label',label,'reason',reason,'createdAt',created_at) FROM integrity_labels WHERE org_id=$1 AND case_id=ANY($2) ORDER BY created_at DESC,id DESC").bind(org).bind(ids).fetch_all(&state.db).await?;
    let penalties=sqlx::query_scalar("SELECT jsonb_build_object('caseId',a.case_id,'id',a.id,'deliveryState',a.delivery_state,'expiresAt',e.expires_at,'active',e.id IS NOT NULL AND e.removed_at IS NULL AND a.reverted_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now())) FROM integrity_actions a LEFT JOIN list_entries e ON e.id=a.list_entry_id WHERE a.server_id=$1 AND a.source='REVIEW' AND a.case_id=ANY($2)").bind(id).bind(ids).fetch_all(&state.db).await?;
    Ok((labels, penalties))
}
async fn history(c: &Context, id: &str, size: i64) -> Result<Value> {
    let filter = c.query("actionsFilter");
    let filter = if ["manual", "automatic"].contains(&filter.as_str()) {
        filter
    } else {
        "all".into()
    };
    let before = crate::integrity_enforcement::date(&json!(c.query("actionsBefore")))
        .filter(|t| *t <= Utc::now())
        .unwrap_or_else(Utc::now);
    let total: i64 = sqlx::query_scalar(include_str!("../../sql/page-history-count.sql"))
        .bind(id)
        .bind(before)
        .bind(&filter)
        .fetch_one(&c.state.db)
        .await?;
    let size = if [10, 20, 50, 100].contains(&size) {
        size
    } else {
        50
    };
    let pages = ((total + size - 1) / size).max(1);
    let page = c
        .query("actionsPage")
        .parse::<i64>()
        .unwrap_or(1)
        .clamp(1, pages);
    let rows: Vec<Value> = sqlx::query_scalar(include_str!("../../sql/page-history.sql"))
        .bind(id)
        .bind(before)
        .bind(&filter)
        .bind(size)
        .bind((page - 1) * size)
        .bind(WARNING)
        .bind(KICK)
        .fetch_all(&c.state.db)
        .await?;
    let calibration = crate::model_http::calibration();
    let rows=rows.into_iter().map(|v|{let v=crate::ai_evidence::row(v);let label=if v["source"]=="SHORT_MODEL"{if v["action"]=="WARNING"{"P99.6"}else{"P99.9"}}else if v["source"]=="LONG_MODEL"{if v["action"]=="QUARANTINE_24H"{"P99"}else if v["threshold"].as_f64()==calibration["p98"].as_f64(){"P98"}else if v["threshold"].as_f64()==calibration["p97"].as_f64(){"P97"}else if v["threshold"].as_f64()==calibration["p95"].as_f64(){"P95"}else{"历史阈值"}}else{""};json!({"id":v["id"],"steamId":v["steamId"],"caseId":v["caseId"],"source":v["source"],"action":v["action"],"deliveryState":v["state"],"deliveryReason":v["reason"],"name":v["name"],"score":v["score"],"threshold":v["threshold"],"percentileLabel":label,"createdAt":v["createdAt"],"effectiveAt":v["effectiveAt"],"expiresAt":v["expiresAt"],"revertedAt":v["revertedAt"]})}).collect::<Vec<_>>();
    Ok(
        json!({"rows":rows,"page":page,"pages":pages,"total":total,"pageSize":size,"filter":filter,"before":before}),
    )
}
pub async fn load(c: &mut Context) -> Result<Value> {
    let a = c.actor().await?;
    let id = c.param("id").to_owned();
    let source = c.input.source.clone();
    if source == "src/routes/(app)/orgs/[id]/integrity/+page.server.ts" {
        crate::api::roles::require_org_owner(&c.state, &a, &id).await?;
        return org(c, &id).await;
    }
    let scope = auth::server_scope(&c.state, &a, &id, "integrity.view").await?;
    let can_configure = crate::api::lists::role_for(&c.state, &a, &scope.org_id)
        .await?
        .is_some_and(|r| r.owner);
    let r = rules::load(&c.state.db, &scope.org_id).await?;
    let committee = rules::committee_enabled(&r.assessment_mode);
    let cfg = crate::integrity_ai::settings(&c.state, &scope.org_id).await?;
    if source == "src/routes/(app)/server/[id]/integrity/ai/+page.server.ts" {
        let cases:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('name',p.persona,'id',c.id,'steamId',c.steam_id,'createdAt',c.created_at,'status',c.status) FROM integrity_cases c LEFT JOIN steam_profiles p ON p.steam_id=c.steam_id WHERE c.server_id=$1 ORDER BY c.created_at DESC LIMIT 100").bind(&id).fetch_all(&c.state.db).await?;
        let ids = case_ids(&cases);
        let mut config = crate::integrity_ai::view(cfg.as_ref());
        if config.is_object() && config["budgetDay"] != Utc::now().format("%Y-%m-%d").to_string() {
            config["dailyRequests"] = json!(0);
        }
        return Ok(
            json!({"jobs":crate::ai_queue::views(&c.state,&ids).await?,"prompt":crate::ai_protocol::SYSTEM,"promptVersion":crate::ai_protocol::PROMPT_VERSION,"canConfigure":can_configure,"config":config,"cases":cases}),
        );
    }
    let before = crate::integrity_enforcement::date(&json!(c.query("before")))
        .filter(|t| *t <= Utc::now())
        .unwrap_or_else(Utc::now);
    let view = c.query("view");
    let view = if ["archived", "cleared", "all"].contains(&view.as_str()) {
        view
    } else {
        "pending".into()
    };
    let is_cases = source == "src/routes/(app)/server/[id]/integrity/cases/+page.server.ts";
    let condition = if !is_cases {
        "c.status NOT IN ('AI_ARCHIVED','AI_CLEARED')"
    } else {
        match view.as_str() {
            "archived" => "c.status NOT IN ('OPEN','AI_CLEARED')",
            "cleared" => "c.status='AI_CLEARED'",
            "all" => "c.status<>'AI_CLEARED'",
            _ => "c.status='OPEN'",
        }
    };
    let total:i64=sqlx::query_scalar(&format!("SELECT count(*) FROM integrity_cases c WHERE c.server_id=$1 AND {RETAINED} AND {condition} AND c.created_at<=$2")).bind(&id).bind(before).fetch_one(&c.state.db).await?;
    let size = if is_cases { 20 } else { 5 };
    let pages = ((total + size - 1) / size).max(1);
    let page = if is_cases {
        c.query("page").parse::<i64>().unwrap_or(1).clamp(1, pages)
    } else {
        1
    };
    let cases:Vec<Value>=sqlx::query_scalar(&format!("SELECT {CASE_FIELDS} FROM integrity_cases c LEFT JOIN steam_profiles p ON p.steam_id=c.steam_id WHERE c.server_id=$1 AND {RETAINED} AND {condition} AND c.created_at<=$2 ORDER BY c.created_at DESC,c.id DESC LIMIT $3 OFFSET $4")).bind(&id).bind(before).bind(size).bind((page-1)*size).fetch_all(&c.state.db).await?;
    let ids = case_ids(&cases);
    let (labels, penalties) = reviews(&c.state, &scope.org_id, &id, &ids).await?;
    let ai = if committee {
        crate::ai_queue::views(&c.state, &ids).await?
    } else {
        vec![]
    };
    let enabled = committee && cfg.as_ref().is_some_and(|v| v["auto_enabled"] == true);
    if is_cases {
        return Ok(
            json!({"view":view,"page":page,"pages":pages,"pageSize":size,"total":total,"before":before,"cases":cases,"labels":labels,"aiJobs":ai,"aiAutoEnabled":enabled,"canConfigure":can_configure,"reviewPenalties":penalties}),
        );
    }
    if source != "src/routes/(app)/server/[id]/integrity/+page.server.ts" {
        return Err(ApiError::missing());
    }
    let retention = crate::integrity_retention::policy(&c.state, &id).await?;
    let h = history(
        c,
        &id,
        retention["policy"]["pageSize"].as_i64().unwrap_or(50),
    )
    .await?;
    let mut review_ids = ids.clone();
    review_ids.extend(
        h["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| a["source"] == "REVIEW")
            .filter_map(|a| a["caseId"].as_str().map(str::to_owned)),
    );
    let (_, penalties) = reviews(&c.state, &scope.org_id, &id, &review_ids).await?;
    let legacy = r.assessment_mode == "legacy";
    let scores: Vec<Value> = if legacy || committee {
        sqlx::query_scalar("SELECT jsonb_build_object('id',id,'steamId',steam_id,'scoredAt',scored_at,'score',score,'level',level,'breakdown',breakdown,'statistical',statistical,'ruleVersion',rule_version) FROM integrity_scores WHERE server_id=$1 ORDER BY scored_at DESC LIMIT 50").bind(&id).fetch_all(&c.state.db).await?
    } else {
        vec![]
    };
    let reports:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'targetSteamId',target_steam_id,'reason',reason,'createdAt',created_at,'status',status) FROM integrity_reports WHERE server_id=$1 ORDER BY created_at DESC LIMIT 50").bind(&id).fetch_all(&c.state.db).await?;
    let live: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=$1")
            .bind(&id)
            .fetch_optional(&c.state.db)
            .await?;
    let live = live.unwrap_or(Value::Null);
    let events = if legacy {
        charts::live_events(&c.state, &id).await?
    } else {
        vec![]
    };
    let roster = if legacy {
        live["players"].as_array().cloned().unwrap_or_default()
    } else {
        vec![]
    };
    let saved: Vec<Value> = if legacy {
        sqlx::query_scalar("SELECT to_jsonb(s) FROM integrity_scores s WHERE server_id=$1 AND scored_at>=now()-interval '15 minutes' ORDER BY score DESC,scored_at DESC LIMIT 1000").bind(&id).fetch_all(&c.state.db).await?
    } else {
        vec![]
    };
    let mut by = HashMap::new();
    for s in saved {
        let s = crate::ai_evidence::row(s);
        if let Some(id) = s["steamId"].as_str() {
            by.entry(id.to_owned()).or_insert(s);
        }
    }
    let ids = roster
        .iter()
        .filter_map(|p| p["steamId"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let risks = charts::current_risks(&c.state, &scope.org_id, &ids, &by, &r.config).await?;
    let overrides = crate::integrity_weapons::overrides(&c.state.db, &scope.org_id).await?;
    let metrics = charts::infantry(
        &events[..events.len().min(3000)],
        &live["status"],
        &overrides,
        Utc::now().timestamp_millis(),
    );
    let mut online=roster.iter().filter(|p|p["steamId"].as_str().is_some_and(|id|crate::api::notes::steam_id(id).is_ok())).map(|p|{let id=p["steamId"].as_str().unwrap();let risk=risks.get(id).cloned().unwrap_or(Value::Null);json!({"steamId":id,"name":p["name"],"kills":p["kills"],"deaths":p["deaths"],"infantry":metrics.get(id),"riskScore":risk["score"],"riskLevel":risk["level"],"riskBreakdown":risk["breakdown"].as_array().cloned().unwrap_or_default()})}).collect::<Vec<_>>();
    online.sort_by(|a, b| {
        b["riskScore"]
            .as_f64()
            .unwrap_or(-1.)
            .total_cmp(&a["riskScore"].as_f64().unwrap_or(-1.))
    });
    let feed: bool =
        sqlx::query_scalar("SELECT feed_token_hash IS NOT NULL FROM servers WHERE id=$1")
            .bind(&id)
            .fetch_one(&c.state.db)
            .await?;
    let mode = if !r.enforcement["autoSuspendedAt"].is_null() {
        "suspended"
    } else if [
        "autoKickEnabled",
        "autoQuarantine24hEnabled",
        "autoQuarantine7dEnabled",
    ]
    .iter()
    .any(|k| r.enforcement[k] == true)
    {
        "experimental"
    } else {
        "dry_run"
    };
    Ok(
        json!({"longModelResults":if rules::long_enabled(&r.assessment_mode){crate::model_http::runs(&c.state.db,&scope.org_id,Some(&id)).await?}else{vec![]},"distributions":distributions(&c.state,&scope.org_id,&id,committee).await?,"shortRisk":if rules::short_enabled(&r.assessment_mode){Some(short(&c.state,&id).await?)}else{None},"aiJobs":ai,"aiAutoEnabled":enabled,"reviewPenalties":penalties,"cases":cases,"scores":scores,"reports":reports,"actions":h["rows"],"actionsPagination":{"page":h["page"],"pages":h["pages"],"total":h["total"],"pageSize":h["pageSize"],"filter":h["filter"],"before":h["before"]},"historyPolicy":retention["policy"],"historyPolicyRevision":retention["revision"],"historyLastCleanup":retention["lastCleanup"],"feedAt":live["feed_at"],"feedConfigured":feed,"playersAt":live["players_at"],"feedRowsTruncated":events.len()>3000,"feedMetricsAvailable":charts::feed_metrics_available(&events,&live["status"]),"onlinePlayers":online,"ruleVersion":r.version,"assessmentMode":r.assessment_mode,"comparison":comparison(&c.state,&scope.org_id,num(&r.config["koThreshold"]),committee).await?,"committeeShadow":shadow_view(&c.state,&id,committee).await?,"labels":labels,"kpmBands":r.config["kpmBands"],"mode":mode,"canConfigure":can_configure,"orgIntegrityUrl":if can_configure{Some(format!("/orgs/{}/integrity",segment(&scope.org_id)))}else{None}}),
    )
}
fn case_ids(cases: &[Value]) -> Vec<String> {
    cases
        .iter()
        .filter_map(|c| c["id"].as_str().map(str::to_owned))
        .collect()
}
async fn org(c: &Context, id: &str) -> Result<Value> {
    let r = rules::load(&c.state.db, id).await?;
    let enabled = rules::committee_enabled(&r.assessment_mode);
    let rows: Vec<Value> = if enabled {
        sqlx::query_scalar("SELECT to_jsonb(b) FROM integrity_baselines b WHERE org_id=$1")
            .bind(id)
            .fetch_all(&c.state.db)
            .await?
    } else {
        vec![]
    };
    let mut metrics = HashSet::new();
    let mut external = HashSet::new();
    let mut max = 0.;
    for row in &rows {
        let n = num(&row["sample_count"]);
        max = f64::max(max, n);
        if n >= if row["source"] == "external" {
            30.
        } else {
            200.
        } {
            metrics.insert(row["metric"].as_str());
        }
        if row["source"] == "external" && n >= 30. {
            external.insert(row["metric"].as_str());
        }
    }
    let overrides = crate::integrity_weapons::overrides(&c.state.db, id)
        .await?
        .into_iter()
        .map(|(cause, category)| json!({"cause":cause,"category":category}))
        .collect::<Vec<_>>();
    let cfg = crate::model_http::config(&c.state.db, id).await?;
    Ok(
        json!({"modelConfig":crate::model_http::view(&cfg),"modelRuns":if rules::long_enabled(&r.assessment_mode){crate::model_http::runs(&c.state.db,id,None).await?}else{vec![]},"orgId":id,"ruleVersion":r.version,"assessmentMode":r.assessment_mode,"comparison":comparison(&c.state,id,num(&r.config["koThreshold"]),enabled).await?,"imports":if enabled{crate::integrity_imports::list(&c.state,id).await?}else{vec![]},"baselineSummary":{"metrics":metrics.len(),"externalMetrics":external.len(),"maxSamples":max,"calculatedAt":rows.first().map(|r|r["calculated_at"].clone())},"ruleConfig":r.config,"ruleDefaults":crate::integrity_score::defaults(),"enforcement":r.enforcement,"weaponOverrides":overrides,"weaponDefaults":crate::integrity_weapons::defaults(),"weaponCategories":crate::integrity_weapons::CATEGORIES}),
    )
}
