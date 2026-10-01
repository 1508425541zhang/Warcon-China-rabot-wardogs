//! Community queries and paid command preparation; transport delivery lives in the worker.
use crate::{
    config::AppState,
    error::{ApiError, Result},
    qq_config::{Configuration, Policy},
    qq_economy,
    webhooks::text,
};
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};
use std::collections::HashMap;
pub async fn server(state: &AppState, id: &str) -> Result<Value> {
    if crate::qq_config::load(state).await?.policy(id).is_none() {
        return Err(ApiError::missing());
    }
    sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'name',s.name,'orgId',s.org_id) FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1 AND o.suspended_at IS NULL").bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)
}
pub async fn action(
    state: &AppState,
    id: &str,
    name: &str,
    params: &Value,
    h: Option<&HeaderMap>,
) -> Result<Value> {
    let _lane = if crate::gateway::remote(state) {
        None
    } else {
        Some(crate::dispatcher::acquire(id, 0, std::time::Duration::from_secs(30)).await?)
    };
    server(state, id).await?;
    crate::gateway::run_held(state, id, name, params, 0, h)
        .await
        .map_err(|e| match e {
            crate::game::Error::Api(e) => e,
            crate::game::Error::Game(e) => ApiError::new(
                StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY),
                "game_error",
                e.message,
            ),
        })
}
fn clean(s: &str) -> String {
    let normalized = s
        .chars()
        .map(|c| {
            if c.is_control() || c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>();
    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(24)
        .collect()
}
pub fn friendly(players: &[Value], steam: &str) -> Result<Vec<Value>> {
    let faction = players
        .iter()
        .find(|p| p["steamId"] == steam)
        .and_then(|p| p["faction"].as_str())
        .filter(|f| !f.is_empty())
        .ok_or_else(|| ApiError::bad("你须在本服在线且已加入阵营。"))?;
    let mut index = HashMap::new();
    let mut out = Vec::new();
    for player in players.iter().filter(|p| {
        p["faction"] == faction && crate::api::notes::steam_id(text(&p["steamId"])).is_ok()
    }) {
        let key = text(&player["steamId"]);
        if let Some(i) = index.get(key) {
            out[*i] = player.clone();
        } else {
            index.insert(key, out.len());
            out.push(player.clone());
        }
    }
    Ok(out)
}
pub fn block(name: &str, hex: &str) -> &'static str {
    if let Ok(rgb) = hex::decode(hex.trim_start_matches('#')) {
        if rgb.len() == 3 {
            let max = *rgb.iter().max().unwrap();
            let min = *rgb.iter().min().unwrap();
            if max - min > 30 {
                return ["🟥", "🟩", "🟦"][rgb.iter().position(|n| *n == max).unwrap()];
            }
        }
    }
    match name {
        "RED" | "red" => "🟥",
        "BLU" | "blue" => "🟦",
        "GRN" | "green" => "🟩",
        _ => "⬜",
    }
}
pub fn battle(status: &Value, players: &[Value], page: usize) -> Result<String> {
    let scores = status["scores"].as_array().cloned().unwrap_or_default();
    let pages = players.len().div_ceil(25).max(1);
    if page == 0 || page > pages {
        return Err(ApiError::bad(format!(
            "玩家名单共有 {pages} 页，请使用 /局势 1–{pages}。"
        )));
    }
    let cap = crate::live::finite(&status["scoreCap"]).filter(|n| *n > 0.);
    let clock = crate::live::finite(&status["matchSeconds"]).filter(|n| *n >= 0.);
    let mut lines = vec![
        format!(
            "{} · {}",
            clean(text(&status["serverName"])),
            clean(text(&status["map"]))
        ),
        format!(
            "已进行 {}；在线 {} 人",
            clock
                .map(|n| format!("{}分{}秒", (n / 60.).floor(), (n % 60.).floor()))
                .unwrap_or_else(|| "未知".into()),
            players.len()
        ),
        "阵营比分 / 获胜目标（每格10%）".into(),
    ];
    for team in &scores {
        let name = text(&team["name"]);
        let color = block(name, text(&team["colorHex"]));
        let n = players.iter().filter(|p| p["faction"] == name).count();
        let score = crate::live::finite(&team["score"]).filter(|n| *n >= 0.);
        lines.push(format!(
            "{color} {} · {n}人 · {} / {}",
            clean(name),
            score
                .map(|n| n.to_string())
                .unwrap_or_else(|| "未知".into()),
            cap.map(|n| n.to_string()).unwrap_or_else(|| "未知".into())
        ));
        if let (Some(cap), Some(score)) = (cap, score) {
            let ratio = (score / cap).min(1.);
            let filled = (ratio * 10.).floor() as usize;
            lines.push(format!(
                "{}{} {}%",
                color.repeat(filled),
                "▫️".repeat(10 - filled),
                (ratio * 100.).floor()
            ));
        } else {
            lines.push("胜利进度未知（接口未提供有效比分或目标）".into())
        }
    }
    if scores.is_empty() {
        lines.push("接口暂未提供阵营比分。".into())
    }
    let mut ordered = Vec::new();
    for s in &scores {
        ordered.extend(players.iter().filter(|p| p["faction"] == s["name"]))
    }
    ordered.extend(
        players
            .iter()
            .filter(|p| !scores.iter().any(|s| s["name"] == p["faction"])),
    );
    lines.push(format!("在线玩家 {page}/{pages}页（每页25人）"));
    for p in ordered.into_iter().skip((page - 1) * 25).take(25) {
        let color = scores
            .iter()
            .find(|s| s["name"] == p["faction"])
            .map(|s| block(text(&s["name"]), text(&s["colorHex"])))
            .unwrap_or("⬜");
        lines.push(format!(
            "{color} {} · {}",
            clean(text(&p["name"])),
            clean(
                p["faction"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("未入阵营")
            )
        ))
    }
    if players.is_empty() {
        lines.push("当前无人在线。".into())
    }
    if pages > 1 {
        lines.push(format!("查看其余玩家：/局势 页码（1–{pages}）"))
    }
    Ok(lines.join("\n"))
}
pub async fn query(
    state: &AppState,
    id: &str,
    view: &str,
    page: usize,
    h: Option<&HeaderMap>,
) -> Result<Value> {
    let s = server(state, id).await?;
    match view {
        "maps" => {
            let c = crate::qq_config::load(state).await?;
            let p = c.policy(id).ok_or_else(ApiError::missing)?;
            Ok(json!({"maps":p.maps,"voteCost":p.vote_cost,"voteSeconds":p.vote_seconds}))
        }
        "status" => {
            let d = action(state, id, "status", &json!({}), h).await?;
            Ok(
                json!({"serverId":id,"name":d["serverName"].as_str().filter(|s|!s.is_empty()).unwrap_or(text(&s["name"])),"map":d["map"],"players":d["playerCount"],"capacity":d["maxPlayers"],"matchSeconds":d["matchSeconds"]}),
            )
        }
        "players" => {
            let d = action(state, id, "players", &json!({}), h).await?;
            let ps = d["players"].as_array().cloned().unwrap_or_default();
            Ok(
                json!({"page":page,"pageSize":20,"total":ps.len(),"players":ps.iter().skip(page.saturating_sub(1)*20).take(20).map(|p|json!({"steamId":p["steamId"],"name":p["name"],"faction":p["faction"]})).collect::<Vec<_>>()}),
            )
        }
        "battle" => {
            let status = action(state, id, "status", &json!({}), h).await?;
            let roster = action(state, id, "players", &json!({}), h).await?;
            let ps = roster["players"].as_array().cloned().unwrap_or_default();
            Ok(
                json!({"page":page,"pageSize":25,"total":ps.len(),"pages":ps.len().div_ceil(25).max(1),"scores":status["scores"],"scoreCap":status["scoreCap"],"matchSeconds":status["matchSeconds"],"text":battle(&status,&ps,page)?}),
            )
        }
        _ => Err(ApiError::bad("Unknown query view.")),
    }
}
pub async fn player(state: &AppState, id: &str, target: &str) -> Result<Value> {
    let s = server(state, id).await?;
    let steam = if crate::api::notes::steam_id(target).is_ok() {
        target.to_owned()
    } else {
        if !(2..=100).contains(&target.encode_utf16().count()) {
            return Err(ApiError::bad("请输入 SteamID64 或至少两个字的玩家名。"));
        }
        let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',steam_id,'name',name) FROM player_sessions WHERE server_id=$1 AND name ILIKE $2 ORDER BY last_seen DESC LIMIT 100").bind(id).bind(format!("%{target}%")).fetch_all(&state.db).await?;
        text(&crate::integrity_reports::resolve(target, &rows)?["steamId"]).to_owned()
    };
    let name:String=sqlx::query_scalar("SELECT name FROM player_sessions WHERE server_id=$1 AND steam_id=$2 ORDER BY last_seen DESC LIMIT 1").bind(id).bind(&steam).fetch_optional(&state.db).await?.unwrap_or_else(||steam.clone());
    let career = crate::leaderboards::career(
        state,
        id,
        &[id.into()],
        &HashMap::from([(id.into(), text(&s["name"]).into())]),
        &steam,
    )
    .await?;
    let span:Value=sqlx::query_scalar("SELECT jsonb_build_object('firstSeen',min(joined_at),'lastSeen',max(last_seen)) FROM player_sessions WHERE server_id=$1 AND steam_id=$2").bind(id).bind(&steam).fetch_one(&state.db).await?;
    let mut rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM(SELECT ts,map,killer_steam_id,killer_name,victim_name,cause,headshot,suicide,team_kill FROM kills WHERE server_id=$1 AND(killer_steam_id=$2 OR victim_steam_id=$2) ORDER BY ts DESC LIMIT 12)k").bind(id).bind(&steam).fetch_all(&state.db).await?;
    rows.reverse();
    let timeline=rows.into_iter().map(|r|json!({"at":r["ts"],"map":r["map"],"action":if r["suicide"]==true{"自杀"}else if r["killer_steam_id"]==steam{"击杀"}else{"阵亡"},"other":if r["killer_steam_id"]==steam{r["victim_name"].clone()}else{r["killer_name"].as_str().map(|s|json!(s)).unwrap_or(json!("环境"))},"weapon":r["cause"],"headshot":r["headshot"],"teamKill":r["team_kill"]})).collect::<Vec<_>>();
    let last = career["last"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            format!(
                "{} UTC · {} · {} · {}/{} · {}",
                text(&m["startedAt"])
                    .chars()
                    .take(16)
                    .collect::<String>()
                    .replace('T', " "),
                text(&m["map"]),
                m["faction"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("阵营未知"),
                m["kills"],
                m["deaths"],
                text(&m["result"])
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let summary = format!(
        "{name}（{steam}）\n{}已记录 {} 场，{} 分钟；击杀 {} / 死亡 {}，{} 胜 {} 负 {} 平。\n最高连杀 {}，爆头 {}。\n最近作战：\n{}\n仅反映本服已保存数据；不推测未记录的战术、移动路线或游戏生涯。\n{}",
        text(&s["name"]),
        career["matches"],
        career["minutes"],
        career["kills"],
        career["deaths"],
        career["wins"],
        career["losses"],
        career["draws"],
        career["killStreak"],
        career["headshots"],
        if last.is_empty() {
            "暂无可汇总比赛"
        } else {
            &last
        },
        if timeline.is_empty() {
            "没有可用的击杀回传，无法还原交战时间线。".into()
        } else {
            format!(
                "最近已记录交战（按时间顺序，UTC）：\n{}",
                timeline
                    .iter()
                    .skip(timeline.len().saturating_sub(6))
                    .map(|e| format!(
                        "{} {} {} {}{}{}{}",
                        text(&e["at"])
                            .chars()
                            .skip(5)
                            .take(14)
                            .collect::<String>()
                            .replace('T', " "),
                        text(&e["map"]),
                        text(&e["action"]),
                        text(&e["other"]),
                        if e["weapon"].is_string() {
                            format!("，{}", text(&e["weapon"]))
                        } else {
                            String::new()
                        },
                        if e["headshot"] == true {
                            "，爆头"
                        } else {
                            ""
                        },
                        if e["teamKill"] == true {
                            "，友伤"
                        } else {
                            ""
                        }
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
    );
    Ok(
        json!({"steamId":steam,"name":name,"career":career,"firstSeen":span["firstSeen"],"lastSeen":span["lastSeen"],"timeline":timeline,"summary":summary}),
    )
}
pub async fn open_vote(state: &AppState, p: &Policy, h: Option<&HeaderMap>) -> Result<String> {
    server(state, &p.server_id).await?;
    let caps = action(state, &p.server_id, "capabilities", &json!({}), h).await?;
    if caps["features"]["rotationEdit"] != true {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "rotation_unavailable",
            "当前游戏版本没有实时地图轮换接口，不能开启付费投票。",
        ));
    }
    let maps = action(state, &p.server_id, "maps", &json!({}), h).await?;
    if p.maps.iter().any(|m| {
        !maps["maps"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v["id"] == *m))
    }) {
        return Err(ApiError::bad(
            "候选地图配置与游戏目录不一致，请管理员更新。",
        ));
    }
    let mut tx = state.worker_transaction().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("qq-vote:{}", p.server_id))
        .execute(&mut *tx)
        .await?;
    let prior:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM qq_votes WHERE server_id=$1 AND(state='open' OR ends_at>now()-interval '5 minutes'))").bind(&p.server_id).fetch_one(&mut *tx).await?;
    if prior {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "vote_cooldown",
            "本服已有投票或仍在五分钟冷却期，请发送 /投票 查看。",
        ));
    }
    let c = crate::qq_config::load_conn(state, &mut tx).await?;
    let current = c.policy(&p.server_id).ok_or_else(ApiError::missing)?;
    if json!(current) != json!(p) {
        return Err(ApiError::bad("QQ 规则已改变，请重试。"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO qq_votes(id,server_id,maps,ends_at) VALUES($1,$2,$3,now()+($4*interval '1 second'))").bind(&id).bind(&p.server_id).bind(json!(p.maps)).bind(p.vote_seconds as f64).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}
pub async fn cast_vote(state: &AppState, p: &Policy, steam: &str, choice: &str) -> Result<String> {
    let mut tx = state.worker_transaction().await?;
    let c = crate::qq_config::load_conn(state, &mut tx).await?;
    let p = c.policy(&p.server_id).ok_or_else(ApiError::missing)?;
    let v:Value=sqlx::query_scalar("SELECT to_jsonb(v) FROM qq_votes v WHERE server_id=$1 AND state='open' AND ends_at>now() FOR UPDATE").bind(&p.server_id).fetch_optional(&mut *tx).await?.ok_or_else(||ApiError::new(StatusCode::CONFLICT,"no_vote","没有进行中的投票，请先发送 /发起投票。"))?;
    let maps: Vec<String> = serde_json::from_value(v["maps"].clone())
        .map_err(|_| ApiError::bad("地图投票数据无效。"))?;
    let map = if !choice.is_empty() && choice.bytes().all(|c| c.is_ascii_digit()) {
        choice
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| maps.get(i))
            .map(String::as_str)
            .unwrap_or("")
    } else {
        choice
    };
    if !maps.iter().any(|m| m == map) {
        return Err(ApiError::bad("请选择候选地图编号或完整地图 ID。"));
    }
    let id = text(&v["id"]);
    let prior: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM qq_ballots WHERE vote_id=$1 AND steam_id=$2)",
    )
    .bind(id)
    .bind(steam)
    .fetch_one(&mut *tx)
    .await?;
    if prior {
        return Ok("你已经投过票，本轮每个 Steam 账号限投一次，不会重复扣分。".into());
    }
    qq_economy::debit(
        &mut tx,
        &p.server_id,
        steam,
        p.vote_cost,
        &format!("vote:{id}:{steam}"),
        "下一张地图投票",
    )
    .await?;
    sqlx::query("INSERT INTO qq_ballots(vote_id,steam_id,map) VALUES($1,$2,$3)")
        .bind(id)
        .bind(steam)
        .bind(map)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(format!("投票成功：{map}，消耗 {} 积分。", p.vote_cost))
}
pub async fn vote_status(state: &AppState, server: &str) -> Result<String> {
    let v: Option<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(v) FROM qq_votes v WHERE server_id=$1 ORDER BY ends_at DESC LIMIT 1",
    )
    .bind(server)
    .fetch_optional(&state.db)
    .await?;
    let Some(v) = v else {
        return Ok("暂无投票，发送 /发起投票 开启。".into());
    };
    let counts: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
        "SELECT map,count(*) FROM qq_ballots WHERE vote_id=$1 GROUP BY map",
    )
    .bind(text(&v["id"]))
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();
    let maps = v["maps"]
        .as_array()
        .ok_or_else(|| ApiError::bad("地图投票数据无效。"))?;
    Ok(format!(
        "地图投票 {}，截止 {}\n{}\n{}",
        text(&v["state"]),
        text(&v["ends_at"]),
        maps.iter()
            .enumerate()
            .map(|(i, m)| format!(
                "{}. {}：{} 票",
                i + 1,
                text(m),
                counts.get(text(m)).unwrap_or(&0)
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        if v["winner"].is_string() {
            format!("胜出：{}；用 /订单 查询设置结果。", text(&v["winner"]))
        } else {
            "发送 /投票 编号；平票按候选列表顺序决定。".into()
        }
    ))
}
fn same_purchase(v: &Value, server: &str, steam: &str, kind: &str, message: &str) -> bool {
    v["server_id"] == server
        && v["steam_id"] == steam
        && v["kind"] == kind
        && (kind != "friendly" || v["params"]["message"] == format!("[友方广播] {message}"))
}
pub async fn purchase(
    state: &AppState,
    p: &Policy,
    steam: &str,
    id: &str,
    kind: &str,
    message: &str,
    h: Option<&HeaderMap>,
) -> Result<String> {
    crate::api::notes::steam_id(steam)?;
    let s = server(state, &p.server_id).await?;
    if !["friendly", "reserve"].contains(&kind) || id.is_empty() || id.len() > 1024 {
        return Err(ApiError::bad("订单格式无效。"));
    }
    let prior: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(o) FROM qq_orders o WHERE id=$1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    if let Some(v) = prior {
        if !same_purchase(&v, &p.server_id, steam, kind, message) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "request_conflict",
                "requestId 已用于不同的请求。",
            ));
        }
        return Ok(id.into());
    }
    let (params, recipients) = if kind == "friendly" {
        if message.is_empty()
            || message.encode_utf16().count() > 180
            || message.chars().any(|c| c < ' ' || c == '<' || c == '>')
        {
            return Err(ApiError::bad(
                "广播正文须为 1–180 字，不能含控制字符或富文本标签。",
            ));
        }
        let roster = action(state, &p.server_id, "players", &json!({}), h).await?;
        let players = roster["players"].as_array().cloned().unwrap_or_default();
        let faction = players
            .iter()
            .find(|p| p["steamId"] == steam)
            .and_then(|p| p["faction"].as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    "not_online",
                    "你须在本服在线且已加入阵营。",
                )
            })?;
        (
            json!({"message":format!("[友方广播] {message}"),"faction":faction}),
            friendly(&players, steam)?
                .iter()
                .map(|p| text(&p["steamId"]).to_owned())
                .collect::<Vec<_>>(),
        )
    } else {
        let live = action(state, &p.server_id, "reserved", &json!({}), h).await?;
        let held:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM list_entries e JOIN lists l ON l.id=e.list_id WHERE l.kind='reserve' AND l.org_id=$1 AND(l.server_id=$2 OR l.server_id IS NULL) AND e.steam_id=$3 AND e.removed_at IS NULL AND(e.expires_at IS NULL OR e.expires_at>now()))").bind(text(&s["orgId"])).bind(&p.server_id).bind(steam).fetch_one(&state.db).await?;
        if held
            || live["reserved"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == steam))
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "already_reserved",
                "你已有有效预留位，无需重复兑换。",
            ));
        }
        (json!({"hours":p.reserve_hours}), vec![])
    };
    let mut tx = state.worker_transaction().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("qq-purchase:{}:{steam}", p.server_id))
        .execute(&mut *tx)
        .await?;
    let c = crate::qq_config::load_conn(state, &mut tx).await?;
    let current = c.policy(&p.server_id).ok_or_else(ApiError::missing)?;
    if json!(current) != json!(p) {
        return Err(ApiError::bad("QQ 规则已改变，请重试。"));
    }
    let prior: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(o) FROM qq_orders o WHERE id=$1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(v) = prior {
        if !same_purchase(&v, &p.server_id, steam, kind, message) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "request_conflict",
                "requestId 已用于不同的请求。",
            ));
        }
        return Ok(id.into());
    }
    let recent:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM qq_orders WHERE server_id=$1 AND steam_id=$2 AND kind=$3 AND(created_at>now()-interval '1 minute' OR state IN ('pending','processing','unknown')))").bind(&p.server_id).bind(steam).bind(kind).fetch_one(&mut *tx).await?;
    if recent {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "purchase_cooldown",
            "请先查看已有订单结果，或等待一分钟冷却。",
        ));
    }
    let cost = if kind == "friendly" {
        p.broadcast_cost
    } else {
        p.reserve_cost
    };
    qq_economy::debit(
        &mut tx,
        &p.server_id,
        steam,
        cost,
        &format!("buy:{id}"),
        if kind == "friendly" {
            "友方逐人广播"
        } else {
            "限时预留位"
        },
    )
    .await?;
    sqlx::query(
        "INSERT INTO qq_orders(id,server_id,steam_id,kind,params,cost) VALUES($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(&p.server_id)
    .bind(steam)
    .bind(kind)
    .bind(params)
    .bind(cost)
    .execute(&mut *tx)
    .await?;
    for steam in recipients {
        sqlx::query("INSERT INTO qq_deliveries(order_id,steam_id) VALUES($1,$2)")
            .bind(id)
            .bind(steam)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(id.into())
}
pub async fn close_votes(state: &AppState, c: &Configuration) -> Result<()> {
    for p in c
        .stored
        .policies
        .iter()
        .filter(|p| c.stored.enabled && p.enabled)
    {
        let mut tx = state.worker_transaction().await?;
        let vote:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(v) FROM qq_votes v WHERE server_id=$1 AND state='open' AND ends_at<=now() FOR UPDATE SKIP LOCKED").bind(&p.server_id).fetch_optional(&mut *tx).await?;
        let Some(v) = vote else { continue };
        let counts: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
            "SELECT map,count(*) FROM qq_ballots WHERE vote_id=$1 GROUP BY map",
        )
        .bind(text(&v["id"]))
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        let maps = v["maps"]
            .as_array()
            .ok_or_else(|| ApiError::bad("地图投票数据无效。"))?;
        let max = maps
            .iter()
            .map(|m| *counts.get(text(m)).unwrap_or(&0))
            .max()
            .unwrap_or(0);
        let winner = maps
            .iter()
            .find(|m| max > 0 && counts.get(text(m)) == Some(&max))
            .map(text);
        sqlx::query("UPDATE qq_votes SET state='closed',winner=$2 WHERE id=$1")
            .bind(text(&v["id"]))
            .bind(winner)
            .execute(&mut *tx)
            .await?;
        if let Some(map) = winner {
            sqlx::query("INSERT INTO qq_orders(id,server_id,steam_id,kind,params,cost,created_at) VALUES($1,$2,'','map',$3,0,$4)").bind(format!("map:{}",text(&v["id"]))).bind(&p.server_id).bind(json!({"map":map})).bind(crate::integrity_enforcement::date(&v["ends_at"])).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}
pub async fn command(state: &AppState, c: &Configuration, m: &Value) -> Result<String> {
    let id = text(&m["server_id"]);
    let member = text(&m["member_id"]);
    let p = c.policy(id).ok_or_else(ApiError::missing)?;
    server(state, id).await?;
    let (name, arg) = crate::qq_protocol::command(text(&m["content"]));
    if name == "帮助" {
        return Ok(format!(
            "/服务器，/在线 [页码]，/地图，/流水\n/局势 [页码]：三方比分和分页名单（也可用 /对局、/比分）\n/绑定 SteamID64 游戏内昵称；/解绑\n/战绩 [玩家名或SteamID]，/总结 [玩家]，/举报 SteamID 原因\n/积分：余额；暖服人数≤{}，每分钟{}积分\n/发起投票，/投票 [编号]（{}积分/票）\n/友方广播 正文（{}积分）；/优先队列（{}积分兑换{}小时预留位）；/订单",
            p.low_at,
            p.points_per_minute,
            p.vote_cost,
            p.broadcast_cost,
            p.reserve_cost,
            p.reserve_hours
        ));
    }
    if ["局势", "对局", "比分", "在线"].contains(&name.as_str()) {
        let page = if arg.is_empty() {
            1
        } else {
            arg.parse::<usize>()
                .ok()
                .filter(|n| *n >= 1 && *n <= 999)
                .ok_or_else(|| ApiError::bad("页码须为 1–999。"))?
        };
        let d = query(
            state,
            id,
            if name == "在线" {
                "players"
            } else {
                "battle"
            },
            page,
            None,
        )
        .await?;
        if name != "在线" {
            return Ok(text(&d["text"]).into());
        }
        return Ok(format!(
            "在线 {} 人，第 {page} 页（20人/页）\n{}",
            d["total"],
            d["players"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| format!(
                    "{} · {} · {}",
                    text(&p["name"]),
                    text(&p["steamId"]),
                    p["faction"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("未入阵营")
                ))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if name == "服务器" {
        let d = query(state, id, "status", 1, None).await?;
        return Ok(format!(
            "{}\n地图 {}；在线 {}/{}",
            text(&d["name"]),
            text(&d["map"]),
            d["players"],
            d["capacity"]
        ));
    }
    if name == "地图" {
        return Ok(format!(
            "候选地图（每票 {} 积分）：\n{}",
            p.vote_cost,
            p.maps
                .iter()
                .enumerate()
                .map(|(i, m)| format!("{}. {m}", i + 1))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if name == "绑定" {
        let Some((steam, name)) = arg.split_once(char::is_whitespace) else {
            return Ok("格式：/绑定 SteamID64 游戏内昵称".into());
        };
        let roster = action(state, id, "players", &json!({}), None).await?;
        let r =
            crate::qq_identity::bind_player(state, id, member, steam, name.trim(), &roster).await?;
        return Ok(format!(
            "{}：{steam}。现在可以发送 /积分 或 /战绩。",
            if r["migrated"] == true {
                "已迁移至官方机器人，原积分和 VIP 保留"
            } else if r["already"] == true {
                "已绑定"
            } else {
                "绑定成功"
            }
        ));
    }
    if name == "解绑" {
        crate::qq_identity::unbind_member(state, id, member).await?;
        return Ok(
            "已解绑当前 QQ，Steam 账号积分保留。重新绑定格式：/绑定 SteamID64 游戏内昵称".into(),
        );
    }
    if name == "投票" && arg.is_empty() {
        return vote_status(state, id).await;
    }
    if ["战绩", "总结"].contains(&name.as_str()) && !arg.is_empty() {
        return Ok(text(&player(state, id, &arg).await?["summary"]).into());
    }
    if ![
        "战绩",
        "总结",
        "个人战绩",
        "积分",
        "流水",
        "举报",
        "发起投票",
        "投票",
        "友方广播",
        "优先队列",
        "订单",
    ]
    .contains(&name.as_str())
    {
        return Ok("未知指令，请发送 /帮助 查看指令。".into());
    }
    let (steam, actor) = crate::qq_identity::linked(state, id, member).await?;
    match name.as_str() {
        "战绩" | "总结" | "个人战绩" => Ok(text(
            &player(state, id, if arg.is_empty() { &steam } else { &arg }).await?["summary"],
        )
        .into()),
        "积分" => {
            let w = qq_economy::wallet(state, id, &steam).await?;
            Ok(format!(
                "积分 {}；累计有效暖服 {} 分钟。",
                w["balance"], w["warmMinutes"]
            ))
        }
        "流水" => {
            let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(l) FROM(SELECT delta,reason,created_at FROM qq_ledger WHERE server_id=$1 AND steam_id=$2 ORDER BY created_at DESC LIMIT 10)l").bind(id).bind(&steam).fetch_all(&state.db).await?;
            Ok(if rows.is_empty() {
                "暂无积分流水。".into()
            } else {
                rows.iter()
                    .map(|r| {
                        format!(
                            "{} {}{} {}",
                            text(&r["created_at"]),
                            if r["delta"].as_i64().unwrap_or(0) > 0 {
                                "+"
                            } else {
                                ""
                            },
                            r["delta"],
                            text(&r["reason"])
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        "举报" => {
            let Some((target, reason)) = arg.split_once(char::is_whitespace) else {
                return Err(ApiError::bad("格式：/举报 SteamID64 至少三个字的原因"));
            };
            crate::api::notes::steam_id(target)?;
            let r = crate::integrity_reports::submit(
                state,
                &actor,
                &HeaderMap::new(),
                id,
                target,
                &format!("[QQ群举报] {}", reason.trim()),
                Some(member),
            )
            .await?;
            Ok(format!(
                "举报 #{} 已进入人工审核并采集前后证据；举报不会直接封禁玩家。",
                r["id"]
            ))
        }
        "发起投票" => {
            open_vote(state, p, None).await?;
            vote_status(state, id).await
        }
        "投票" => cast_vote(state, p, &steam, &arg).await,
        "友方广播" | "优先队列" => {
            let order = purchase(
                state,
                p,
                &steam,
                &format!("qq:{}", text(&m["id"])),
                if name == "友方广播" {
                    "friendly"
                } else {
                    "reserve"
                },
                &arg,
                None,
            )
            .await?;
            Ok(format!(
                "订单 {order} 已受理。发送 /订单 查看结果。预留位按游戏支持的机制生效，不保证改变排队算法。"
            ))
        }
        "订单" => {
            let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(o) FROM(SELECT id,kind,state,outcome FROM qq_orders WHERE server_id=$1 AND(steam_id=$2 OR kind='map') ORDER BY created_at DESC LIMIT 5)o").bind(id).bind(&steam).fetch_all(&state.db).await?;
            Ok(if rows.is_empty() {
                "暂无订单。".into()
            } else {
                rows.iter()
                    .map(|o| {
                        format!(
                            "{}\n{} · {} · {}",
                            text(&o["id"]),
                            text(&o["kind"]),
                            text(&o["state"]),
                            o["outcome"].as_str().unwrap_or("等待处理")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        _ => Ok("未知指令，请发送 /帮助。".into()),
    }
}
