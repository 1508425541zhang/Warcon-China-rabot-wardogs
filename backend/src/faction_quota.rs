//! Native proportional team quotas, with ETag configuration preparation and one
//! unconfirmed move at a time. No player is kicked to create capacity.
use crate::{
    actions,
    auth::{Actor, ServerScope},
    config::AppState,
    config_document as ini,
    error::{ApiError, Result},
    game::Client,
    game_automation as auto,
    integrity_enforcement::date,
    observer::Memory,
};
use axum::http::{HeaderMap, StatusCode};
use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};
pub const COLORS: [&str; 3] = ["blue", "red", "green"];
const SECTION: &str = "/Script/WDGame.WDGameStateSession";
const SWITCH: &str = "bLockOverpopulatedTeamsConfig";
fn key(id: &str) -> String {
    format!("factionQuota:{id}")
}
fn runtime_key(id: &str) -> String {
    format!("factionQuotaRuntime:{id}")
}
pub fn default_config() -> Value {
    json!({"revision":"empty","enabled":false,"limits":{"blue":50,"red":50,"green":0},"graceSeconds":60,"waitMatchId":null,"originalJoinLock":null})
}
pub fn validate(v: &Value) -> Result<Value> {
    if !v["revision"]
        .as_str()
        .is_some_and(|s| s.encode_utf16().count() <= 100)
        || !v["enabled"].is_boolean()
        || COLORS
            .iter()
            .any(|c| !crate::automation_policy::integer_range(&v["limits"][*c], 0, 100))
    {
        return Err(ApiError::bad("配额格式无效。"));
    }
    if COLORS
        .iter()
        .filter(|c| v["limits"][**c].as_f64().unwrap() > 0.)
        .count()
        < 2
    {
        return Err(ApiError::bad("至少开放两个阵营。"));
    }
    if COLORS
        .iter()
        .map(|c| v["limits"][*c].as_f64().unwrap())
        .sum::<f64>()
        > 100.
    {
        return Err(ApiError::bad("三方人数合计不能超过100。"));
    }
    let grace = v.get("graceSeconds").cloned().unwrap_or(json!(60));
    if !crate::automation_policy::integer_range(&grace, 30, 600) {
        return Err(ApiError::bad("开局保护时间应为30～600秒。"));
    }
    Ok(
        json!({"revision":v["revision"],"enabled":v["enabled"],"limits":{"blue":v["limits"]["blue"].as_f64().unwrap()as i64,"red":v["limits"]["red"].as_f64().unwrap()as i64,"green":v["limits"]["green"].as_f64().unwrap()as i64},"graceSeconds":grace.as_f64().unwrap()as i64}),
    )
}
pub fn teams(scores: &[Value]) -> Option<Value> {
    let mut out = json!({});
    for score in scores {
        let hex = score["colorHex"]
            .as_str()?
            .strip_prefix('#')
            .unwrap_or(score["colorHex"].as_str()?);
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let rgb = [
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ];
        let max = *rgb.iter().max()?;
        let min = *rgb.iter().min()?;
        if max - min < 40 || rgb.iter().filter(|v| **v == max).count() != 1 {
            return None;
        }
        let color = if rgb[0] == max {
            "red"
        } else if rgb[1] == max {
            "green"
        } else {
            "blue"
        };
        let name = score["name"].as_str().filter(|s| !s.is_empty())?;
        if !out[color].is_null() {
            return None;
        }
        out[color] = json!(name)
    }
    if COLORS.iter().all(|c| out[*c].is_string())
        && COLORS
            .iter()
            .map(|c| out[*c].as_str())
            .collect::<HashSet<_>>()
            .len()
            == 3
    {
        Some(out)
    } else {
        None
    }
}
pub fn plan(players: &[Value], teams: &Value, limits: &Value, blocked: &HashSet<String>) -> Value {
    let roster: Vec<_> = players
        .iter()
        .filter(|p| COLORS.iter().any(|c| teams[*c] == p["faction"]))
        .collect();
    let counts: Vec<i64> = COLORS
        .iter()
        .map(|c| roster.iter().filter(|p| p["faction"] == teams[*c]).count() as i64)
        .collect();
    let total: f64 = COLORS
        .iter()
        .map(|c| limits[*c].as_f64().unwrap_or(0.))
        .sum();
    let count_json = COLORS
        .iter()
        .zip(&counts)
        .map(|(c, n)| (c.to_string(), json!(n)))
        .collect::<serde_json::Map<_, _>>();
    if roster.len() as f64 > total || total <= 0. {
        return json!({"counts":count_json,"targets":limits,"moves":[],"reason":"在线阵营人数超过配额总和，暂无可用位置；不会踢人腾位。"});
    }
    let raw: Vec<_> = COLORS
        .iter()
        .map(|c| roster.len() as f64 * limits[*c].as_f64().unwrap() / total)
        .collect();
    let mut targets: Vec<_> = raw.iter().map(|n| n.floor() as i64).collect();
    let mut rest = roster.len() as i64 - targets.iter().sum::<i64>();
    let mut indices = vec![0, 1, 2];
    indices.sort_by(|a, b| {
        raw[*b]
            .fract()
            .total_cmp(&raw[*a].fract())
            .then(counts[*b].cmp(&counts[*a]))
            .then(a.cmp(b))
    });
    for c in indices {
        if rest > 0 && (targets[c] as f64) < limits[COLORS[c]].as_f64().unwrap() {
            targets[c] += 1;
            rest -= 1
        }
    }
    let mut projected = counts.clone();
    let mut moves = vec![];
    for from in 0..3 {
        let mut candidates: Vec<_> = roster
            .iter()
            .copied()
            .filter(|p| {
                p["faction"] == teams[COLORS[from]]
                    && p["steamId"].as_str().is_some_and(|s| {
                        crate::api::notes::steam_id(s).is_ok() && !blocked.contains(s)
                    })
            })
            .collect();
        candidates.sort_by(|a, b| {
            a["kills"]
                .as_f64()
                .unwrap_or(0.)
                .total_cmp(&b["kills"].as_f64().unwrap_or(0.))
                .then(a["steamId"].as_str().cmp(&b["steamId"].as_str()))
        });
        for player in candidates {
            if projected[from] <= targets[from] {
                break;
            }
            let to = (0..3)
                .filter(|c| projected[*c] < targets[*c])
                .max_by(|a, b| {
                    (targets[*a] - projected[*a])
                        .cmp(&(targets[*b] - projected[*b]))
                        .then(b.cmp(a))
                });
            let Some(to) = to else { break };
            moves.push(json!({"steamId":player["steamId"],"from":teams[COLORS[from]],"to":teams[COLORS[to]]}));
            projected[from] -= 1;
            projected[to] += 1
        }
    }
    let target_json = COLORS
        .iter()
        .zip(&targets)
        .map(|(c, n)| (c.to_string(), json!(n)))
        .collect::<serde_json::Map<_, _>>();
    json!({"counts":count_json,"targets":target_json,"reason":if !moves.is_empty(){""}else if counts!=targets{"等待之前调队确认或冷却结束。"}else{"人数已符合配额比例。"},"moves":moves})
}
pub async fn config(state: &AppState, id: &str) -> Result<Value> {
    let value: Option<Value> = sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1")
        .bind(key(id))
        .fetch_optional(&state.db)
        .await?;
    let mut out = default_config();
    if let Some(v) = value {
        if let Some(o) = v.as_object() {
            for (k, v) in o {
                out[k] = v.clone()
            }
        }
    }
    Ok(out)
}
pub async fn view(state: &AppState, id: &str) -> Result<Value> {
    let runtime: Option<Value> = sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1")
        .bind(runtime_key(id))
        .fetch_optional(&state.db)
        .await?;
    Ok(
        json!({"config":config(state,id).await?,"runtime":runtime.unwrap_or(json!({"reason":"默认关闭；启用后开始检查人数。","events":[]}))}),
    )
}
async fn conflicts(state: &AppState, id: &str) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM faction_lock_rules WHERE server_id=$1 AND enabled)OR EXISTS(SELECT 1 FROM skill_balance_rules WHERE server_id=$1 AND enabled)").bind(id).fetch_one(&state.db).await?)
}
fn conflict(msg: &str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "conflict", msg)
}
async fn store_prepared(state: &AppState, id: &str, revision: &str, value: &Value) -> Result<()> {
    if sqlx::query(
        "UPDATE site_settings SET value=$3,updated_at=now()WHERE key=$1 AND value->>'revision'=$2",
    )
    .bind(key(id))
    .bind(revision)
    .bind(value)
    .execute(&state.db)
    .await?
    .rows_affected()
        == 0
    {
        return Err(conflict("准备期间设置已改变，请刷新。"));
    }
    Ok(())
}
async fn call(
    state: &AppState,
    id: &str,
    action: &str,
    params: &Value,
    headers: &HeaderMap,
) -> Result<Value> {
    crate::gateway::run_held(state, id, action, params, 0, Some(headers))
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "game",
                "游戏配置接口不可用；设置保持关闭。",
            )
        })
}
pub async fn save(
    state: &AppState,
    actor: &Actor,
    server: &ServerScope,
    h: &HeaderMap,
    input: &Value,
) -> Result<Value> {
    let requested = validate(input)?;
    if requested["enabled"] == true && conflicts(state, &server.id).await? {
        return Err(conflict(
            "请先关闭“禁止自行换边”和“强弱阵营平衡”，避免反复调队。",
        ));
    }
    let revision = uuid::Uuid::new_v4().to_string();
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(key(&server.id))
        .execute(&mut *tx)
        .await?;
    let previous: Option<Value> =
        sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1 FOR UPDATE")
            .bind(key(&server.id))
            .fetch_optional(&mut *tx)
            .await?;
    let mut previous = previous.unwrap_or_else(default_config);
    for (k, v) in default_config().as_object().unwrap() {
        if previous.get(k).is_none() {
            previous[k] = v.clone()
        }
    }
    if previous["revision"] != requested["revision"] {
        return Err(conflict("设置已变化，请刷新后重试。"));
    }
    if previous["preparing"] == true
        && Utc::now().timestamp_millis() - previous["preparationAt"].as_i64().unwrap_or(0) < 120000
    {
        return Err(conflict("正在准备游戏配置，请稍后重试。"));
    }
    let mut claim = previous.clone();
    claim["revision"] = json!(revision);
    claim["enabled"] = json!(false);
    claim["preparing"] = json!(true);
    claim["preparationAt"] = json!(Utc::now().timestamp_millis());
    sqlx::query("INSERT INTO site_settings(key,value,updated_by,updated_at)VALUES($1,$2,$3,now())ON CONFLICT(key)DO UPDATE SET value=excluded.value,updated_by=excluded.updated_by,updated_at=excluded.updated_at").bind(key(&server.id)).bind(claim).bind(&actor.id).execute(&mut *tx).await?;
    tx.commit().await?;
    let mut next = previous.clone();
    for (k, v) in requested.as_object().unwrap() {
        next[k] = v.clone()
    }
    next["revision"] = json!(revision);
    next["preparing"] = json!(false);
    let preparation=async{
  let _lane=crate::dispatcher::acquire(&server.id,0,Duration::from_secs(15)).await?;
  if requested["enabled"]==true||!previous["originalJoinLock"].is_null(){let caps=call(state,&server.id,"capabilities",&json!({}),h).await?;if requested["enabled"]==true&&caps["features"]["changeTeam"]!=true{return Err(conflict("服务器不支持官方管理员调队接口，无法启用。"))}if caps["features"]["configDocument"]!=true{return Err(conflict("服务器不支持可写配置文档。"))}
   let doc=call(state,&server.id,"config",&json!({}),h).await?;let text=doc["text"].as_str().unwrap_or("");let existing=ini::scalar(text,SECTION,SWITCH).ok_or_else(||conflict("服务器未提供原生阵营加入限制设置，未自动修改。"))?;let desired=if requested["enabled"]==true{Some("false")}else{previous["originalJoinLock"].as_str()};
   if let Some(desired)=desired.filter(|d|!existing.eq_ignore_ascii_case(d)&&(requested["enabled"]==true||existing.eq_ignore_ascii_case("false"))){if requested["enabled"]==true{next["originalJoinLock"]=previous.get("originalJoinLock").filter(|v|!v.is_null()).cloned().unwrap_or(json!(existing));let mid:Option<i64>=sqlx::query_scalar("SELECT id FROM matches WHERE server_id=$1 AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1").bind(&server.id).fetch_optional(&state.db).await?;next["waitMatchId"]=json!(mid)}
    let mut held=next.clone();held["enabled"]=json!(false);held["preparing"]=json!(true);held["preparationAt"]=json!(Utc::now().timestamp_millis());store_prepared(state,&server.id,&revision,&held).await?;
    let result=call(state,&server.id,"configApply",&json!({"revision":doc["revision"],"text":ini::set_scalar(text,SECTION,SWITCH,desired)}),h).await?;if result["ok"]!=true{return Err(conflict("游戏配置未应用，请刷新后重试。"))}let verified=call(state,&server.id,"config",&json!({}),h).await?;if ini::scalar(verified["text"].as_str().unwrap_or(""),SECTION,SWITCH).is_none_or(|s|!s.eq_ignore_ascii_case(desired)){return Err(conflict("游戏配置中的加入限制未改变，可能被启动参数锁定。"))}
   }
  }
  if requested["enabled"]!=true{next["originalJoinLock"]=Value::Null;next["waitMatchId"]=Value::Null}Ok::<_,ApiError>(())
 }.await;
    if let Err(error) = preparation {
        next["enabled"] = json!(false);
        next["preparing"] = json!(false);
        store_prepared(state, &server.id, &revision, &next).await?;
        return Err(error);
    }
    store_prepared(state, &server.id, &revision, &next).await?;
    let mut tx = state.db.begin().await?;
    crate::audit::event(
        &mut tx,
        actor,
        Some(&server.org_id),
        h,
        "trigger",
        "faction_quota.settings",
        &server.id,
        next.clone(),
    )
    .await?;
    tx.commit().await?;
    sqlx::query("SELECT pg_notify('warcon_observe',$1)")
        .bind(json!({"serverId":server.id}).to_string())
        .execute(&state.db)
        .await?;
    Ok(next)
}
async fn persist(
    state: &AppState,
    m: &Memory,
    cfg: &Value,
    runtime: &Value,
    reason: &str,
) -> Result<()> {
    let mut value = runtime.clone();
    value["reason"] = json!(reason);
    let mut tx = state.worker_transaction().await?;
    sqlx::query("INSERT INTO site_settings(key,value,updated_at)SELECT $1,$2,now()WHERE EXISTS(SELECT 1 FROM site_settings WHERE key=$3 AND value->>'revision'=$4)ON CONFLICT(key)DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at").bind(runtime_key(&m.id)).bind(value).bind(key(&m.id)).bind(cfg["revision"].as_str()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn run(
    state: &AppState,
    client: &Client,
    m: &Memory,
    trusted: bool,
    boundary: bool,
) -> Result<()> {
    state.runtime.check().await?;
    let cfg = config(state, &m.id).await?;
    if cfg["enabled"] != true || validate(&cfg).is_err() {
        return Ok(());
    }
    let now = Utc::now();
    let mut rt = view(state, &m.id).await?["runtime"].clone();
    if !trusted || boundary || !auto::fresh(m, 30000) || auto::ended(&m.status) {
        return persist(state, m, &cfg, &rt, "观测尚未稳定或正在换局，暂停调队。").await;
    }
    let Some(round) = auto::round(state, m).await? else {
        return persist(state, m, &cfg, &rt, "等待开局或重连保护时间结束。").await;
    };
    if round["map"] != m.status["map"]
        || now.timestamp_millis()
            - date(&round["started_at"])
                .unwrap()
                .timestamp_millis()
                .max(m.started_at)
            < cfg["graceSeconds"].as_i64().unwrap_or(60) * 1000
    {
        return persist(state, m, &cfg, &rt, "等待开局或重连保护时间结束。").await;
    }
    if cfg["waitMatchId"] == round["id"] {
        return persist(
            state,
            m,
            &cfg,
            &rt,
            "原生人数差限制将在下一局解除；当前等待换局。",
        )
        .await;
    }
    if conflicts(state, &m.id).await? {
        return persist(
            state,
            m,
            &cfg,
            &rt,
            "检测到禁止自行换边或强弱平衡已启用，暂停配额调队。",
        )
        .await;
    }
    let Some(teams) = teams(
        m.status["scores"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    ) else {
        return persist(
            state,
            m,
            &cfg,
            &rt,
            "无法唯一识别蓝、红、绿阵营，暂停调队。",
        )
        .await;
    };
    if m.players
        .iter()
        .map(|p| &p.steam_id)
        .collect::<HashSet<_>>()
        .len()
        != m.players.len()
    {
        return persist(state, m, &cfg, &rt, "玩家快照含重复身份，暂停调队。").await;
    }
    if now.timestamp_millis() - rt["lastCheck"].as_i64().unwrap_or(0) >= 30000 {
        rt["lastCheck"] = json!(now.timestamp_millis());
        rt["nativeReady"] = json!(false);
        let doc = actions::read_config(client).await;
        match doc {
            Ok(doc) => {
                rt["nativeReady"] = json!(
                    ini::scalar(doc["text"].as_str().unwrap_or(""), SECTION, SWITCH)
                        .is_some_and(|s| s.eq_ignore_ascii_case("false"))
                )
            }
            Err(_) => {
                return persist(
                    state,
                    m,
                    &cfg,
                    &rt,
                    "无法读取官方游戏配置，暂停调队；稍后重新检查。",
                )
                .await;
            }
        }
        if rt["nativeReady"] != true {
            return persist(
                state,
                m,
                &cfg,
                &rt,
                "原生人数差限制尚未解除，请重新保存启用设置。",
            )
            .await;
        }
    }
    if rt["nativeReady"] != true {
        return persist(
            state,
            m,
            &cfg,
            &rt,
            rt["reason"].as_str().unwrap_or("等待官方游戏配置核对。"),
        )
        .await;
    }
    let mut events = rt["events"].as_array().cloned().unwrap_or_default();
    for event in &mut events {
        if ["sending", "unknown"].contains(&event["state"].as_str().unwrap_or(""))
            && m.players.iter().any(|p| {
                event["steamId"] == p.steam_id && event["to"].as_str() == p.faction.as_deref()
            })
        {
            event["state"] = json!("confirmed");
            event["reason"] = json!("已在新玩家快照中确认目标阵营。")
        }
    }
    rt["events"] = json!(events);
    if events.iter().any(|e| {
        ["sending", "unknown"].contains(&e["state"].as_str().unwrap_or(""))
            && date(&e["at"]).is_some_and(|t| (now - t).num_milliseconds() < 120000)
    }) {
        return persist(
            state,
            m,
            &cfg,
            &rt,
            "等待上一条调队在新快照中确认，暂不占用更多名额。",
        )
        .await;
    }
    let blocked: HashSet<_> = events
        .iter()
        .filter(|e| date(&e["at"]).is_some_and(|t| (now - t).num_milliseconds() < 120000))
        .filter_map(|e| e["steamId"].as_str())
        .map(str::to_owned)
        .collect();
    let roster: Vec<_> = m.players.iter().map(|p| json!(p)).collect();
    let plan = plan(&roster, &teams, &cfg["limits"], &blocked);
    rt["counts"] = plan["counts"].clone();
    rt["targets"] = plan["targets"].clone();
    let Some(movement) = plan["moves"].as_array().and_then(|a| a.first()) else {
        return persist(state, m, &cfg, &rt, plan["reason"].as_str().unwrap_or("")).await;
    };
    let mut event = movement.clone();
    event["at"] = json!(auto::version(now));
    event["state"] = json!("sending");
    event["reason"] = json!("已提交调队，等待新快照确认。");
    event["revision"] = cfg["revision"].clone();
    events.insert(0, event);
    events.truncate(50);
    rt["events"] = json!(events);
    let mut tx = state.worker_transaction().await?;
    let current: Option<Value> =
        sqlx::query_scalar("SELECT value FROM site_settings WHERE key=$1 FOR SHARE")
            .bind(key(&m.id))
            .fetch_optional(&mut *tx)
            .await?;
    if current
        .as_ref()
        .is_none_or(|c| c["enabled"] != true || c["revision"] != cfg["revision"])
    {
        return Ok(());
    }
    sqlx::query("INSERT INTO faction_move_permits(server_id,steam_id,faction,expires_at)VALUES($1,$2,$3,now()+interval '120 seconds')").bind(&m.id).bind(movement["steamId"].as_str()).bind(movement["to"].as_str()).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO site_settings(key,value,updated_at)VALUES($1,$2,now())ON CONFLICT(key)DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at").bind(runtime_key(&m.id)).bind(&rt).execute(&mut *tx).await?;
    tx.commit().await?;
    let current = config(state, &m.id).await?;
    if !auto::fresh(m, 30000)
        || current["enabled"] != true
        || current["revision"] != cfg["revision"]
    {
        rt["events"][0]["state"] = json!("cancelled");
        rt["events"][0]["reason"] = json!("观测已过期或设置已改变，未执行调队。");
        return persist(state, m, &cfg, &rt, "观测已过期或设置已改变，未执行调队。").await;
    }
    state.runtime.check().await?;
    let success = actions::run(
        client,
        "changeTeam",
        &json!({"steamId":movement["steamId"],"faction":movement["to"]}),
    )
    .await
    .is_ok();
    let reason = if success {
        "调队已发送；将在下一次玩家快照核对人数。"
    } else {
        rt["events"][0]["state"] = json!("unknown");
        rt["events"][0]["reason"] =
            json!("请求失败或结果不确定；两分钟内不重复操作该玩家，请核对阵营。");
        "请求失败或结果不确定；两分钟内不重复操作该玩家，请核对阵营。"
    };
    persist(state, m, &cfg, &rt, reason).await?;
    let mut tx = state.worker_transaction().await?;
    auto::audit(
        &mut tx,
        m,
        "faction_quota.move",
        movement["steamId"].as_str().unwrap(),
        if success { "delivered" } else { "error" },
        movement,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
