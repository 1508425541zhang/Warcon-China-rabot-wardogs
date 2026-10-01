//! Select three pairs once; only new, accepted victim deaths permit movement.
use crate::{
    actions,
    config::AppState,
    error::{ApiError, Result},
    game::{Client, Error},
    game_automation as auto,
    integrity_enforcement::date,
    observer::Memory,
};
use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};
pub fn plan(status: &Value, players: &[Value], lead: f64) -> Option<Value> {
    if !lead.is_finite() || lead < 40. {
        return None;
    }
    let mut scores = status["scores"].as_array()?.clone();
    if scores.len() != 3
        || scores.iter().any(|s| {
            s["name"].as_str().is_none_or(str::is_empty)
                || s["score"].as_f64().is_none_or(|n| !n.is_finite() || n < 0.)
        })
        || scores
            .iter()
            .map(|s| s["name"].as_str())
            .collect::<HashSet<_>>()
            .len()
            != 3
    {
        return None;
    }
    scores.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["score"].as_f64().unwrap())
    });
    let strong = &scores[0];
    let weak = &scores[2];
    if strong["score"].as_f64()? <= 0.
        || strong["score"].as_f64() == scores[1]["score"].as_f64()
        || weak["score"].as_f64() == scores[1]["score"].as_f64()
        || strong["score"].as_f64()? - scores[1]["score"].as_f64()? <= lead
    {
        return None;
    }
    let eligible: Vec<_> = players
        .iter()
        .filter(|p| {
            p["steamId"]
                .as_str()
                .is_some_and(|s| crate::api::notes::steam_id(s).is_ok())
                && ["kpm", "kd"]
                    .iter()
                    .all(|k| p[*k].as_f64().is_some_and(|n| n.is_finite() && n >= 0.))
        })
        .collect();
    if eligible
        .iter()
        .map(|p| p["steamId"].as_str())
        .collect::<HashSet<_>>()
        .len()
        != eligible.len()
    {
        return None;
    }
    let rank = |a: &&Value, b: &&Value| {
        b["kpm"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["kpm"].as_f64().unwrap())
            .then(
                b["kd"]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&a["kd"].as_f64().unwrap()),
            )
            .then(a["steamId"].as_str().cmp(&b["steamId"].as_str()))
    };
    let mut top: Vec<_> = eligible
        .iter()
        .copied()
        .filter(|p| p["faction"] == strong["name"])
        .collect();
    top.sort_by(rank);
    top.truncate(3);
    let mut bottom: Vec<_> = eligible
        .iter()
        .copied()
        .filter(|p| p["faction"] == weak["name"])
        .collect();
    bottom.sort_by(|a, b| {
        a["kpm"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["kpm"].as_f64().unwrap())
            .then(
                a["kd"]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&b["kd"].as_f64().unwrap()),
            )
            .then(a["steamId"].as_str().cmp(&b["steamId"].as_str()))
    });
    bottom.truncate(3);
    if top.len() != 3
        || bottom.len() != 3
        || top.iter().zip(&bottom).any(|(a, b)| {
            a["kpm"].as_f64().unwrap() < b["kpm"].as_f64().unwrap()
                || a["kpm"].as_f64() == b["kpm"].as_f64()
                    && a["kd"].as_f64().unwrap() <= b["kd"].as_f64().unwrap()
        })
    {
        return None;
    }
    Some(
        json!({"strong":strong["name"],"weak":weak["name"],"pairs":top.iter().zip(&bottom).map(|(a,b)|json!({"strong":a,"weak":b})).collect::<Vec<_>>()}),
    )
}
pub fn fresh_death(k: &Value, selected: chrono::DateTime<Utc>, plan: &Value, now: i64) -> bool {
    k["killer"]["steamId"]
        .as_str()
        .is_some_and(|s| crate::api::notes::steam_id(s).is_ok())
        && k["killer"]["steamId"] != k["victim"]["steamId"]
        && k["suicide"] != true
        && k["teamKill"] != true
        && date(&k["ts"])
            .is_some_and(|t| t > selected && (0..=5000).contains(&(now - t.timestamp_millis())))
        && k["instanceId"] == plan["instanceId"]
        && k["matchId"] == plan["gameMatchId"]
        && k["eventTime"]
            .as_f64()
            .is_some_and(|t| plan["selectedClock"].as_f64().is_some_and(|s| t > s))
}
pub async fn select(state: &AppState, m: &Memory, trusted: bool, boundary: bool) -> Result<()> {
    if !trusted || boundary || !auto::fresh(m, 30000) {
        return Ok(());
    }
    state.runtime.check().await?;
    let rule: Option<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(r)FROM skill_balance_rules r WHERE server_id=$1 AND enabled",
    )
    .bind(&m.id)
    .fetch_optional(&state.db)
    .await?;
    let Some(rule) = rule else { return Ok(()) };
    let Some(round) = auto::round(state, m).await? else {
        return Ok(());
    };
    let mid = round["id"].as_i64().unwrap();
    let now = Utc::now();
    let grace = rule["grace_seconds"].as_i64().unwrap_or(300);
    if now.timestamp_millis()
        - date(&round["started_at"])
            .unwrap()
            .timestamp_millis()
            .max(m.started_at)
        < grace * 1000
    {
        return Ok(());
    }
    let id = format!("skill-balance:{}:{mid}", m.id);
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE skill_balance_runs SET state='cancelled',reason='对局已结束，旧候选作废',updated_at=now()WHERE server_id=$1 AND match_id<>$2 AND state IN('waiting_death','waiting_safe')").bind(&m.id).bind(mid).execute(&mut *tx).await?;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT state FROM skill_balance_runs WHERE id=$1")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;
    if existing.as_ref().is_some_and(|s| s != "waiting_safe") {
        return Ok(());
    }
    let latest:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(k)FROM kills k WHERE server_id=$1 AND match_row=$2 ORDER BY ts DESC LIMIT 1").bind(&m.id).bind(mid).fetch_optional(&state.db).await?;
    let Some(latest) = latest.filter(|k| {
        date(&k["ts"]).is_some_and(|t| (0..=60000).contains(&(now - t).num_milliseconds()))
    }) else {
        cancel(state, &id, "当前对局Feed过期，候选作废").await?;
        return Ok(());
    };
    let clock = m.status["matchSeconds"].as_f64().unwrap_or(
        latest["event_time"].as_f64().unwrap_or(f64::NAN)
            + (now - date(&latest["ts"]).unwrap())
                .num_milliseconds()
                .max(0) as f64
                / 1000.,
    );
    if !clock.is_finite() || clock < grace as f64 || auto::ended(&m.status) {
        cancel(state, &id, "对局处于开局或结束保护").await?;
        return Ok(());
    }
    let coverage:(Option<f64>,Option<f64>,Option<chrono::DateTime<Utc>>)=sqlx::query_as("SELECT min(event_time)::float8,max(event_time)::float8,max(ts)FROM kills WHERE server_id=$1 AND match_row=$2 AND ts>=$3").bind(&m.id).bind(mid).bind(date(&round["started_at"])).fetch_one(&state.db).await?;
    if coverage.0.is_none_or(|n| n > clock - 180.)
        || coverage.1.is_none_or(|n| n > clock + 5. || clock - n > 60.)
        || coverage
            .2
            .is_none_or(|t| (now - t).num_milliseconds() > 60000)
    {
        cancel(state, &id, "当前对局Feed无法覆盖完整窗口").await?;
        return Ok(());
    }
    let kills:Vec<(String,i64,i64)>=sqlx::query_as("SELECT killer_steam_id,count(distinct(instance_id,event_id)),count(distinct(instance_id,event_id))FILTER(WHERE event_time>$4-180)FROM kills WHERE server_id=$1 AND match_row=$2 AND ts>=$3 AND event_time<=$4 AND NOT suicide AND NOT team_kill AND killer_steam_id<>victim_steam_id GROUP BY killer_steam_id").bind(&m.id).bind(mid).bind(date(&round["started_at"])).bind(clock).fetch_all(&state.db).await?;
    let deaths:Vec<(Option<String>,i64)>=sqlx::query_as("SELECT victim_steam_id,count(distinct(instance_id,event_id))FROM kills WHERE server_id=$1 AND match_row=$2 AND ts>=$3 AND event_time<=$4 GROUP BY victim_steam_id").bind(&m.id).bind(mid).bind(date(&round["started_at"])).bind(clock).fetch_all(&state.db).await?;
    let sessions:Vec<(String,Option<String>)>=sqlx::query_as("SELECT steam_id,faction FROM player_sessions WHERE server_id=$1 AND left_at IS NULL AND joined_at<now()-interval '180 seconds'").bind(&m.id).fetch_all(&state.db).await?;
    let permits: Vec<String> = sqlx::query_scalar(
        "SELECT steam_id FROM faction_move_permits WHERE server_id=$1 AND expires_at>=now()",
    )
    .bind(&m.id)
    .fetch_all(&state.db)
    .await?;
    let ranked: Vec<_> = m
        .players
        .iter()
        .filter(|p| {
            sessions
                .iter()
                .any(|s| s.0 == p.steam_id && s.1 == p.faction)
                && !permits.contains(&p.steam_id)
        })
        .map(|p| {
            let stats = kills.iter().find(|s| s.0 == p.steam_id);
            let k = stats.map(|s| s.1).unwrap_or(0);
            let d = deaths
                .iter()
                .find(|s| s.0.as_deref() == Some(&p.steam_id))
                .map(|s| s.1)
                .unwrap_or(0);
            let mut v = json!(p);
            v["kills"] = json!(k);
            v["deaths"] = json!(d);
            v["kpm"] = json!(stats.map(|s| s.2).unwrap_or(0) as f64 / 3.);
            v["kd"] = json!(k as f64 / d.max(1) as f64);
            v
        })
        .collect();
    let Some(mut plan) = plan(
        &m.status,
        &ranked,
        rule["lead_points"].as_f64().unwrap_or(40.),
    ) else {
        cancel(state, &id, "分差、选人或阵营条件不再满足").await?;
        return Ok(());
    };
    plan["selectedClock"] = json!(clock);
    plan["instanceId"] = latest["instance_id"].clone();
    plan["gameMatchId"] = latest["match_id"].clone();
    plan["ruleUpdatedAt"] = json!(auto::version(date(&rule["updated_at"]).unwrap()));
    let moves:Vec<_>=plan["pairs"].as_array().unwrap().iter().flat_map(|p|[json!({"steamId":p["strong"]["steamId"],"from":plan["strong"],"to":plan["weak"],"state":"waiting"}),json!({"steamId":p["weak"]["steamId"],"from":plan["weak"],"to":plan["strong"],"state":"waiting"})]).collect();
    let mut tx = state.worker_transaction().await?;
    let current: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT updated_at FROM skill_balance_rules WHERE server_id=$1 AND enabled FOR SHARE",
    )
    .bind(&m.id)
    .fetch_optional(&mut *tx)
    .await?;
    if current != date(&rule["updated_at"]) {
        return Ok(());
    }
    sqlx::query("INSERT INTO skill_balance_runs(id,server_id,match_id,state,reason,plan,moves,created_at,updated_at)VALUES($1,$2,$3,'waiting_death','已选出3对玩家，逐人等待新的被击杀事件',$4,$5,$6,$6)ON CONFLICT(id)DO UPDATE SET state=excluded.state,reason=excluded.reason,plan=excluded.plan,moves=excluded.moves,created_at=excluded.created_at,updated_at=excluded.updated_at WHERE skill_balance_runs.state='waiting_safe'").bind(&id).bind(&m.id).bind(mid).bind(plan).bind(json!(moves)).bind(now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
async fn cancel(state: &AppState, id: &str, reason: &str) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    sqlx::query("UPDATE skill_balance_runs SET state='cancelled',reason=$2,updated_at=now()WHERE id=$1 AND state IN('waiting_death','waiting_safe')").bind(id).bind(reason).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn deaths(state: &AppState, server: &str, events: &[Value]) -> Result<()> {
    if !events.iter().any(|k| {
        date(&k["ts"]).is_some_and(|t| (0..=5000).contains(&(Utc::now() - t).num_milliseconds()))
    }) {
        return Ok(());
    }
    let run:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(r)FROM skill_balance_runs r WHERE server_id=$1 AND state='waiting_death' ORDER BY created_at DESC LIMIT 1").bind(server).fetch_optional(&state.db).await?;
    let Some(run) = run else { return Ok(()) };
    let plan = &run["plan"];
    let selected = date(&run["created_at"]).unwrap();
    let mid = run["match_id"].as_i64().unwrap();
    let id = run["id"].as_str().unwrap();
    let mut events: Vec<_> = events
        .iter()
        .filter(|k| {
            k["matchRow"] == mid && fresh_death(k, selected, plan, Utc::now().timestamp_millis())
        })
        .collect();
    events.sort_by(|a, b| {
        b["eventTime"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["eventTime"].as_f64().unwrap())
    });
    if events.is_empty() {
        return Ok(());
    }
    let _lane = match crate::dispatcher::acquire(server, 1, Duration::from_secs(5)).await {
        Ok(g) => g,
        Err(_) => return Ok(()),
    };
    state.runtime.check().await?;
    let client = Client::for_server(state, server)
        .await
        .map_err(|_| ApiError::bad("无法建立调队连接。"))?;
    for death in events {
        if !fresh_death(death, selected, plan, Utc::now().timestamp_millis()) {
            continue;
        }
        let current:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(r)||jsonb_build_object('org_id',s.org_id,'server_name',s.name)FROM skill_balance_runs r JOIN servers s ON s.id=r.server_id JOIN organizations o ON o.id=s.org_id WHERE r.id=$1 AND r.state='waiting_death' AND o.suspended_at IS NULL").bind(id).fetch_optional(&state.db).await?;
        let Some(current) = current else { continue };
        let mut moves = current["moves"].as_array().cloned().unwrap_or_default();
        let Some(index) = moves
            .iter()
            .position(|m| m["steamId"] == death["victim"]["steamId"] && m["state"] == "waiting")
        else {
            continue;
        };
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM skill_balance_rules r JOIN matches m ON m.server_id=r.server_id WHERE r.server_id=$1 AND r.enabled AND date_trunc('milliseconds',r.updated_at)=$2 AND m.id=$3 AND m.ended_at IS NULL)").bind(server).bind(date(&plan["ruleUpdatedAt"])).bind(mid).fetch_one(&state.db).await?;
        if !valid {
            cancel(
                state,
                id,
                "已关闭、设置改变或对局结束；已完成调队不强制撤回",
            )
            .await?;
            continue;
        }
        let status = actions::run(&client, "status", &json!({}))
            .await
            .map_err(|_| ApiError::bad("无法读取调队状态。"))?;
        let roster = actions::run(&client, "players", &json!({}))
            .await
            .map_err(|_| ApiError::bad("无法读取调队名单。"))?;
        let map: Option<String> = sqlx::query_scalar("SELECT map FROM matches WHERE id=$1")
            .bind(mid)
            .fetch_one(&state.db)
            .await?;
        if crate::feed::map_id(status["map"].as_str().unwrap_or(""))
            != crate::feed::map_id(map.as_deref().unwrap_or(""))
            || status["matchSeconds"]
                .as_f64()
                .is_some_and(|n| n < death["eventTime"].as_f64().unwrap() - 5.)
            || auto::ended(&status)
        {
            cancel(state, id, "地图或对局阶段改变，停止后续调队").await?;
            continue;
        }
        let Some(other) = moves.get(index / 2 * 2 + if index % 2 == 0 { 1 } else { 0 }) else {
            cancel(state, id, "配对数据无效").await?;
            continue;
        };
        if ["sending", "unknown"].contains(&other["state"].as_str().unwrap_or("")) {
            continue;
        }
        if other["state"] != "confirmed" {
            let pairs: Vec<_> = plan["pairs"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|p| [p["strong"].clone(), p["weak"].clone()])
                .collect();
            let lead: f64 = sqlx::query_scalar(
                "SELECT lead_points::float8 FROM skill_balance_rules WHERE server_id=$1",
            )
            .bind(server)
            .fetch_one(&state.db)
            .await?;
            let still = self::plan(&status, &pairs, lead);
            if still.is_none_or(|p| p["strong"] != plan["strong"] || p["weak"] != plan["weak"]) {
                continue;
            }
        }
        let target = moves[index].clone();
        let steam = target["steamId"].as_str().unwrap();
        if !roster["players"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|p| p["steamId"] == steam && p["faction"] == target["from"])
        {
            continue;
        }
        let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM kills WHERE server_id=$1 AND match_row=$2 AND killer_steam_id=$3 AND event_time>$4)OR coalesce((SELECT max(event_time)>$4+5 FROM kills WHERE server_id=$1 AND match_row=$2 AND instance_id=$5),false)OR EXISTS(SELECT 1 FROM faction_move_permits WHERE server_id=$1 AND steam_id=$3 AND expires_at>now())").bind(server).bind(mid).bind(steam).bind(death["eventTime"].as_f64()).bind(death["instanceId"].as_str()).fetch_one(&state.db).await?;
        if blocked {
            continue;
        }
        let mut tx = state.worker_transaction().await?;
        let active: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
            "SELECT updated_at FROM skill_balance_rules WHERE server_id=$1 AND enabled FOR SHARE",
        )
        .bind(server)
        .fetch_optional(&mut *tx)
        .await?;
        let locked: Option<Value> = sqlx::query_scalar(
            "SELECT to_jsonb(r)FROM skill_balance_runs r WHERE id=$1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(locked) = locked.filter(|r| r["state"] == "waiting_death") else {
            continue;
        };
        moves = locked["moves"].as_array().cloned().unwrap_or_default();
        if active.map(|t| auto::version(t)) != plan["ruleUpdatedAt"].as_str().map(str::to_owned)
            || !fresh_death(death, selected, plan, Utc::now().timestamp_millis())
            || moves.get(index).is_none_or(|m| {
                m["state"] != "waiting"
                    || m["eventId"] == death["eventId"]
                    || m["eventTime"]
                        .as_f64()
                        .is_some_and(|t| t >= death["eventTime"].as_f64().unwrap())
            })
        {
            continue;
        }
        moves[index]["state"] = json!("sending");
        moves[index]["eventId"] = death["eventId"].clone();
        moves[index]["instanceId"] = death["instanceId"].clone();
        moves[index]["eventTime"] = death["eventTime"].clone();
        moves[index]["attemptedAt"] = json!(auto::version(Utc::now()));
        sqlx::query("UPDATE skill_balance_runs SET moves=$2,reason='收到新的被击杀事件，正在调队',updated_at=now()WHERE id=$1").bind(id).bind(json!(moves)).execute(&mut *tx).await?;
        tx.commit().await?;
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM skill_balance_rules WHERE server_id=$1 AND enabled AND date_trunc('milliseconds',updated_at)=$2)").bind(server).bind(date(&plan["ruleUpdatedAt"])).fetch_one(&state.db).await?;
        state.runtime.check().await?;
        let (outcome, reason) = if !valid
            || !fresh_death(death, selected, plan, Utc::now().timestamp_millis())
        {
            ("waiting", "死亡事件在排队中超时，等待下一次被击杀")
        } else {
            match client
                .json(
                    "PATCH",
                    &format!("/v1/players/{steam}"),
                    Some(json!({"faction":target["to"]})),
                )
                .await
            {
                Ok(_) => {
                    let after = actions::run(&client, "players", &json!({})).await.ok();
                    if after.as_ref().is_some_and(|v| {
                        v["players"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|p| p["steamId"] == steam && p["faction"] == target["to"])
                    }) {
                        ("confirmed", "被击杀事件触发，阵营已确认；未调用强制死亡")
                    } else {
                        ("unknown", "调队结果未确认，停止自动重试")
                    }
                }
                Err(Error::Game(e)) if ["faction_full", "team_full"].contains(&e.code.as_str()) => {
                    (
                        "waiting",
                        "目标阵营满员，调队保护冷却120秒后等待新的被击杀事件",
                    )
                }
                Err(_) => ("unknown", "调队失败，结果未知；停止自动重试"),
            }
        };
        let mut tx = state.worker_transaction().await?;
        let locked: Value = sqlx::query_scalar(
            "SELECT to_jsonb(r)FROM skill_balance_runs r WHERE id=$1 FOR UPDATE",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let mut moves = locked["moves"].as_array().cloned().unwrap_or_default();
        if moves
            .get(index)
            .is_some_and(|m| m["state"] == "sending" && m["eventId"] == death["eventId"])
        {
            moves[index]["state"] = json!(outcome);
            moves[index]["reason"] = json!(reason);
            let final_state = if moves.iter().all(|m| m["state"] == "confirmed") {
                "done"
            } else {
                locked["state"].as_str().unwrap()
            };
            sqlx::query("UPDATE skill_balance_runs SET moves=$2,state=$3,reason=$4,updated_at=now()WHERE id=$1").bind(id).bind(json!(moves)).bind(final_state).bind(if final_state=="done"{"六名玩家均在各自被击杀事件后完成调队"}else{reason}).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO audit_log(server_id,server_name,org_id,category,action,target,outcome,detail)VALUES($1,$2,$3,'trigger','skill_balance.death_move',$4,$5,$6)").bind(server).bind(current["server_name"].as_str()).bind(current["org_id"].as_str()).bind(steam).bind(if outcome=="confirmed"{"ok"}else{"error"}).bind(json!({"runId":id,"eventId":death["eventId"],"from":target["from"],"to":target["to"],"state":outcome,"reason":reason})).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(())
}
