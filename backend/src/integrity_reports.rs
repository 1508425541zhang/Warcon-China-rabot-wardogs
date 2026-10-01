//! Reports authenticate their reporter, freeze bounded evidence and never create a punishment.
use crate::{
    auth::{self, Actor},
    config::AppState,
    error::{ApiError, Result},
    integrity_rules,
    integrity_score::{self, Signals},
    webhooks::{clip, text},
};
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};
pub fn resolve(input: &str, players: &[Value]) -> Result<Value> {
    let input = input.trim();
    let mut seen = std::collections::HashSet::new();
    let roster = players
        .iter()
        .filter(|p| seen.insert(text(&p["steamId"]).to_owned()))
        .collect::<Vec<_>>();
    let steam = input.len() == 17 && input.bytes().all(|c| c.is_ascii_digit());
    let mut matches = roster
        .iter()
        .filter(|p| {
            if steam {
                text(&p["steamId"]) == input
            } else {
                text(&p["name"]) == input
            }
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches = roster
            .iter()
            .filter(|p| text(&p["name"]).to_lowercase() == input.to_lowercase())
            .collect()
    }
    if matches.is_empty() && input.encode_utf16().count() >= 3 {
        matches = roster
            .iter()
            .filter(|p| {
                text(&p["name"])
                    .to_lowercase()
                    .contains(&input.to_lowercase())
            })
            .collect()
    }
    if matches.len() > 1 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "multiple_targets",
            "多个玩家名称匹配，请使用完整名称或 SteamID64。",
        ));
    }
    matches.first().map(|p| (**p).clone()).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "target_not_found",
            "该玩家最近未出现在此服务器。",
        )
    })
}
pub async fn submit(
    state: &AppState,
    actor: &Actor,
    h: &HeaderMap,
    server: &str,
    target: &str,
    reason: &str,
    qq_member: Option<&str>,
) -> Result<Value> {
    let target = clip(target.trim(), 200);
    let reason = clip(reason.trim(), 300);
    if server.is_empty()
        || server.len() > 100
        || target.is_empty()
        || reason.encode_utf16().count() < 3
    {
        return Err(ApiError::bad("请填写服务器、目标和至少三个字的原因。"));
    }
    let s:Value=sqlx::query_scalar("SELECT jsonb_build_object('org',o.id,'public',s.public_status AND o.allow_public_status) FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1 AND o.suspended_at IS NULL").bind(server).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    if let Some(member) = qq_member {
        if !crate::qq_config::identity(member)
            || crate::qq_config::load(state)
                .await?
                .policy(server)
                .is_none()
        {
            return Err(ApiError::missing());
        }
        let (linked, identity) = crate::qq_identity::linked(state, server, member).await?;
        if identity.id != actor.id {
            return Err(ApiError::forbidden());
        }
        crate::api::notes::steam_id(&linked)?;
    } else if s["public"] != true {
        auth::server_scope(state, actor, server, "integrity.view").await?;
    }
    let mut tx = state.worker_transaction().await?;
    let verified: Option<String> =
        if let Some(member) = qq_member.filter(|member| actor.id == format!("qq:{member}")) {
            sqlx::query_scalar(
            "SELECT steam_id FROM qq_links WHERE server_id=$1 AND member_id=$2 AND user_id IS NULL",
        )
        .bind(server)
        .bind(member)
        .fetch_optional(&mut *tx)
        .await?
        } else {
            sqlx::query_scalar(
                "SELECT account_id FROM account WHERE user_id=$1 AND provider_id='steam' LIMIT 1",
            )
            .bind(&actor.id)
            .fetch_optional(&mut *tx)
            .await?
        };
    let reporter = verified.ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "steam_link_required",
            "请先绑定已验证的 Steam 账号。",
        )
    })?;
    crate::api::notes::steam_id(&reporter)?;
    let roster:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',steam_id,'name',name) FROM player_sessions WHERE server_id=$1 AND last_seen>=now()-interval '15 minutes' ORDER BY last_seen DESC LIMIT 250").bind(server).fetch_all(&mut *tx).await?;
    let target = resolve(&target, &roster)?;
    let steam = text(&target["steamId"]);
    if steam == reporter {
        return Err(ApiError::bad("不能举报自己。"));
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("reporter:{reporter}"))
        .execute(&mut *tx)
        .await?;
    let limited:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integrity_reports WHERE org_id=$1 AND reporter_steam_id=$2 AND target_steam_id=$3 AND created_at>=now()-interval '10 minutes') OR (SELECT count(*)>=5 FROM integrity_reports WHERE reporter_steam_id=$2 AND created_at>=now()-interval '1 hour')").bind(text(&s["org"])).bind(&reporter).bind(steam).fetch_one(&mut *tx).await?;
    if limited {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "report_limit",
            "最近已有举报或已达到每小时上限，请稍后再试。",
        ));
    }
    let now = chrono::Utc::now();
    let from = now - chrono::Duration::seconds(180);
    let until = now + chrono::Duration::seconds(180);
    let id:i64=sqlx::query_scalar("INSERT INTO integrity_reports(org_id,server_id,target_steam_id,reporter_steam_id,reason,source,created_at,evidence_from,evidence_until) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id").bind(text(&s["org"])).bind(server).bind(steam).bind(&reporter).bind(&reason).bind(if qq_member.is_some(){"qq"}else{"panel"}).bind(now).bind(from).bind(until).fetch_one(&mut *tx).await?;
    let events:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND ts>=$2 AND ts<=$3 AND(killer_steam_id=$4 OR victim_steam_id=$4) ORDER BY ts DESC LIMIT 1001").bind(server).bind(from).bind(now).bind(steam).fetch_all(&mut *tx).await?;
    if events.len() > 1000 {
        return Err(ApiError::bad("举报证据超过 1000 条，未保存不完整举报。"));
    }
    for event in events {
        let at = crate::integrity_enforcement::date(&event["ts"])
            .ok_or_else(|| ApiError::bad("证据时间无效。"))?;
        let mut frozen = crate::integrity_cases::row_view(event.clone());
        frozen["ts"] = json!(at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        sqlx::query("INSERT INTO integrity_report_events(report_id,instance_id,event_id,received_at,event) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING").bind(id).bind(text(&event["instance_id"])).bind(text(&event["event_id"])).bind(at).bind(frozen).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO integrity_reporter_stats(org_id,steam_id,reports_submitted) VALUES($1,$2,1) ON CONFLICT(org_id,steam_id) DO UPDATE SET reports_submitted=integrity_reporter_stats.reports_submitted+1").bind(text(&s["org"])).bind(&reporter).execute(&mut *tx).await?;
    let n:i64=sqlx::query_scalar("SELECT count(DISTINCT reporter_steam_id) FROM integrity_reports WHERE org_id=$1 AND target_steam_id=$2 AND created_at>=now()-interval '24 hours'").bind(text(&s["org"])).bind(steam).fetch_one(&mut *tx).await?;
    let row: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM integrity_rules r WHERE org_id=$1")
            .bind(text(&s["org"]))
            .fetch_optional(&mut *tx)
            .await?;
    let rules = integrity_rules::from_row(row.as_ref())?;
    let signal = Signals {
        committee_mode: false,
        behavior_reasons: vec![],
        kpm180: 0.,
        unique_victims: 0.,
        previous_kpm: vec![],
        unique_reporters: n as f64,
        repeat_high_risk_window: false,
        infantry_kills: 0.,
        headshots: 0.,
        penetrations: 0.,
        burst_points: 0.,
        vac_bans: 0.,
        game_bans: 0.,
        days_since_last_ban: None,
        wardogs_playtime_hours: None,
    };
    let score = integrity_score::score(&signal, &rules.config);
    sqlx::query("INSERT INTO integrity_scores(report_id,source,org_id,server_id,steam_id,scored_at,rule_version,score,level,breakdown,current_behavior_anomaly) VALUES($1,'report',$2,$3,$4,$5,$6,$7,$8,$9,false)").bind(id).bind(text(&s["org"])).bind(server).bind(steam).bind(now).bind(rules.version).bind(score.score).bind(score.level).bind(json!(score.breakdown)).execute(&mut *tx).await?;
    crate::audit::event(
        &mut tx,
        actor,
        Some(text(&s["org"])),
        h,
        "player",
        "integrity.report.create",
        steam,
        json!({"reportId":id,"serverId":server,"reporterSteamId":reporter}),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"targetSteamId":steam}))
}
