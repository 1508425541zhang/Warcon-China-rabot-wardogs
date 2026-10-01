//! Read-only historical replay. No intentions or game mutations are issued here.
use crate::{
    auth::ServerScope, config::AppState, error::Result, integrity_enforcement::date,
    trigger_engine as e, trigger_policy as p,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
fn stamp(ms: i64) -> Value {
    json!(
        DateTime::from_timestamp_millis(ms)
            .unwrap()
            .to_rfc3339_opts(SecondsFormat::Millis, true)
    )
}
pub fn seed_totals(
    rows: &[Value],
    sessions: &[Value],
    cfg: &Value,
    to: i64,
    max_hold: i64,
) -> HashMap<String, Value> {
    let mut spans: Vec<(i64, i64)> = vec![];
    for (i, r) in rows.iter().enumerate() {
        if r["ok"] != true
            || r["player_count"].as_i64().unwrap_or(0) > cfg["lowAt"].as_i64().unwrap_or(20)
        {
            continue;
        }
        let from = date(&r["ts"]).unwrap().timestamp_millis();
        let end = to
            .min(
                rows.get(i + 1)
                    .and_then(|r| date(&r["ts"]))
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(to),
            )
            .min(from + max_hold);
        if end <= from {
            continue;
        }
        if let Some(last) = spans.last_mut().filter(|(_, end)| *end >= from) {
            last.1 = end
        } else {
            spans.push((from, end))
        }
    }
    let fulls: Vec<_> = rows
        .iter()
        .filter(|r| {
            r["ok"] == true
                && cfg["fullAt"]
                    .as_i64()
                    .or(r["max_players"].as_i64().filter(|n| *n > 0))
                    .is_some_and(|n| r["player_count"].as_i64().unwrap_or(0) >= n)
        })
        .filter_map(|r| date(&r["ts"]))
        .map(|d| d.timestamp_millis())
        .collect();
    let mut banked: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
    for s in sessions {
        let Some(id) = s["steam_id"].as_str() else {
            continue;
        };
        let start = date(&s["joined_at"]).unwrap().timestamp_millis();
        let end = date(&s["left_at"])
            .map(|d| d.timestamp_millis())
            .unwrap_or(to);
        let spans: Vec<_> = spans
            .iter()
            .filter_map(|(f, t)| {
                let from = start.max(*f);
                let to = end.min(*t);
                (to > from).then_some((from, to))
            })
            .collect();
        let entries = banked.entry(id.into()).or_default();
        if cfg["untilFull"] == false {
            for (f, t) in spans {
                entries.push((t, t - f))
            }
        } else {
            let mut pending = 0;
            let mut index = 0;
            for full in fulls.iter().copied().filter(|f| *f >= start && *f <= end) {
                while index < spans.len() && spans[index].1 <= full {
                    pending += spans[index].1 - spans[index].0;
                    index += 1
                }
                if pending > 0 {
                    entries.push((full, pending));
                    pending = 0
                }
            }
        }
    }
    banked
        .into_iter()
        .filter_map(|(id, mut entries)| {
            if entries.is_empty() {
                return None;
            }
            entries.sort_by_key(|(at, _)| *at);
            let mut total = 0;
            let mut crossed = None;
            let target = cfg["minutes"].as_i64().unwrap_or(1) * 60000;
            for (at, ms) in entries {
                let before = total;
                total += ms;
                if crossed.is_none() && total >= target {
                    crossed = Some(if cfg["untilFull"] != false {
                        at
                    } else {
                        at - ms + target - before
                    })
                }
            }
            Some((id, json!({"seconds":total/1000,"crossedAt":crossed})))
        })
        .collect()
}
pub async fn replay(
    state: &AppState,
    server: &ServerScope,
    kind: &str,
    cfg: &Value,
) -> Result<Value> {
    let now = Utc::now();
    let from = now - chrono::Duration::hours(24);
    let mut result = json!({"kind":kind,"from":from.to_rfc3339_opts(SecondsFormat::Millis,true),"to":now.to_rfc3339_opts(SecondsFormat::Millis,true),"fires":0,"items":[],"notes":[]});
    let mut items = vec![];
    let mut fires = 0;
    let mut notes = vec![];
    let mut push = |at: Value, text: String| {
        fires += 1;
        if items.len() < 50 {
            items.push(json!({"at":at,"text":text}))
        }
    };
    let reserved: HashSet<String> =
        sqlx::query_scalar::<_, String>("SELECT steam_id FROM server_reserved WHERE server_id=$1")
            .bind(&server.id)
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .collect();
    match kind {
        "welcome" => {
            let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s)||jsonb_build_object('first',NOT EXISTS(SELECT 1 FROM player_sessions e WHERE e.server_id=s.server_id AND e.steam_id=s.steam_id AND e.joined_at<s.joined_at)) FROM player_sessions s WHERE s.server_id=$1 AND s.joined_at>=$2 AND(NOT $3 OR(s.faction IS NOT NULL AND s.faction<>'')) ORDER BY s.joined_at LIMIT 5000").bind(&server.id).bind(from).bind(cfg["afterFaction"]==true).fetch_all(&state.db).await?;
            for r in &rows {
                if cfg["onlyFirstVisit"] == true && r["first"] != true {
                    continue;
                }
                let msg = p::render(
                    cfg["message"].as_str().unwrap_or(""),
                    &json!({"name":r["name"],"server":server.name,"map":"…","players":"…","max":"…"}),
                );
                push(
                    stamp(date(&r["joined_at"]).unwrap().timestamp_millis()),
                    format!("whisper {}: {msg}", r["name"].as_str().unwrap_or("")),
                );
            }
            notes.push(json!(if cfg["afterFaction"] == true {
                "仅回放最终有阵营的会话；时间为加入时，实际在选边后私信。"
            } else {
                "按历史加入事件回放。"
            }));
            if rows.len() == 5000 {
                notes.push(json!("仅回放前5000个加入事件。"))
            }
        }
        "faction_change" => notes.push(json!(
            "会话历史不保存每次换边，无法回放；实时检测从一个有效阵营切换至另一个阵营。"
        )),
        "ping_kick" => notes.push(json!(
            "历史样本没有持续延迟数据；实时检测在新玩家列表中累计，恢复、缺失或采样中断会重置。"
        )),
        "risk_kick" => {
            let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',steam_id,'name',(array_agg(name ORDER BY joined_at))[1],'onAt',GREATEST(min(joined_at),$2))FROM player_sessions WHERE server_id=$1 AND last_seen>=$2 GROUP BY steam_id ORDER BY max(last_seen)DESC LIMIT 2000").bind(&server.id).bind(from).fetch_all(&state.db).await?;
            let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM servers WHERE org_id=$1")
                .bind(&server.org_id)
                .fetch_all(&state.db)
                .await?;
            let signals = crate::player_risk::inputs(
                state,
                &server.org_id,
                &ids,
                Some(&server.id),
                &rows,
                p::risk_score(cfg).is_some(),
            )
            .await?;
            for r in &rows {
                let steam = r["steamId"].as_str().unwrap();
                if let Some(verdict) = signals
                    .get(steam)
                    .and_then(|s| crate::player_risk::verdict(cfg, s, now.timestamp_millis()))
                {
                    push(
                        stamp(date(&r["onAt"]).unwrap().timestamp_millis()),
                        format!(
                            "kick {} ({steam}): {verdict}",
                            r["name"].as_str().unwrap_or("")
                        ),
                    );
                }
            }
            notes.push(json!(format!("检查{}名独立玩家，每人一次；使用已保存的 Steam 与保留席位快照，缺失资料不推断违规。",rows.len())));
        }
        "team_kill" => {
            let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k)||jsonb_build_object('n',(SELECT count(*) FROM kills k2 WHERE k2.server_id=k.server_id AND k2.killer_steam_id=k.killer_steam_id AND k2.team_kill AND k2.ts<=k.ts AND k2.ts>=COALESCE((SELECT max(s.joined_at)FROM player_sessions s WHERE s.server_id=k.server_id AND s.steam_id=k.killer_steam_id AND s.joined_at<=k.ts),k.ts-interval '1 hour')))FROM kills k WHERE server_id=$1 AND team_kill AND killer_steam_id IS NOT NULL AND ts>=$2 ORDER BY ts LIMIT 5000").bind(&server.id).bind(from).fetch_all(&state.db).await?;
            for r in &rows {
                if let Some(stage) = p::team_stage(cfg, r["n"].as_i64().unwrap_or(0)) {
                    let msg = p::render(
                        if stage == "kick" {
                            cfg["kickReason"].as_str()
                        } else {
                            cfg["warnMessage"].as_str()
                        }
                        .unwrap_or(""),
                        &json!({"name":r["killer_name"],"victim":r["victim_name"],"count":r["n"],"server":server.name}),
                    );
                    push(
                        stamp(date(&r["ts"]).unwrap().timestamp_millis()),
                        format!(
                            "{} {}: {msg}",
                            if stage == "kick" { "kick" } else { "whisper" },
                            r["killer_name"].as_str().unwrap_or("")
                        ),
                    )
                }
            }
            notes.push(json!(format!(
                "回放{}条队杀事件，按会话累计；最多5000条。",
                rows.len()
            )));
        }
        "kill_rate" => {
            let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k) FROM kills k WHERE server_id=$1 AND ts>=$2 ORDER BY ts,event_id LIMIT 200000").bind(&server.id).bind(from).fetch_all(&state.db).await?;
            let mut counted = vec![];
            let mut i = 0;
            while i < rows.len() {
                let receipt = date(&rows[i]["ts"]).unwrap().timestamp_millis();
                let mut j = i;
                while j < rows.len() && date(&rows[j]["ts"]).unwrap().timestamp_millis() == receipt
                {
                    j += 1
                }
                let latest = rows[i..j]
                    .iter()
                    .filter_map(|r| r["event_time"].as_f64())
                    .fold(f64::NEG_INFINITY, f64::max);
                for row in &rows[i..j] {
                    let k = crate::api::kills::view(row.clone());
                    if crate::legacy_consumer::counts(&k) {
                        counted.push((
                            receipt
                                - ((latest - k["eventTime"].as_f64().unwrap_or(latest)).max(0.)
                                    * 1000.) as i64,
                            k,
                        ));
                    }
                }
                i = j
            }
            counted.sort_by_key(|(at, _)| *at);
            let mut tracks: HashMap<String, p::Track> = HashMap::new();
            for (at, k) in &counted {
                let steam = k["killer"]["steamId"].as_str().unwrap();
                if let Some(verdict) = p::rate_step(
                    cfg,
                    tracks.entry(steam.into()).or_default(),
                    *at,
                    k["headshot"] == true,
                ) {
                    push(
                        stamp(*at),
                        format!(
                            "flag {} ({steam}): {verdict}",
                            k["killer"]["name"].as_str().unwrap_or(steam)
                        ),
                    )
                }
            }
            notes.push(json!(format!(
                "回放{}次手持武器击杀；载具、车载武器、建筑不计入。最多读取200000条。",
                counted.len()
            )));
        }
        "restart_notice" => {
            let row: Option<Value> =
                sqlx::query_scalar("SELECT to_jsonb(l)FROM server_live l WHERE server_id=$1")
                    .bind(&server.id)
                    .fetch_optional(&state.db)
                    .await?;
            if let Some(row) = row.filter(|r| date(&r["started_at"]).is_some()) {
                let started = date(&row["started_at"]).unwrap().timestamp_millis();
                let due = started + 86400000;
                let mut vars = json!({"server":server.name,"map":"…","players":row["player_count"],"max":"…","uptime":e::uptime(now.timestamp_millis()-started),"minutes":0});
                let lead = cfg["leadMinutes"].as_i64().unwrap_or(0);
                if lead > 0 {
                    vars["minutes"] = json!(lead);
                    push(
                        stamp(due - lead * 60000),
                        format!(
                            "broadcast: {}",
                            p::render(cfg["leadMessage"].as_str().unwrap_or(""), &vars)
                        ),
                    );
                }
                vars["minutes"] = json!(0);
                push(
                    stamp(due),
                    format!(
                        "broadcast: {}",
                        p::render(cfg["message"].as_str().unwrap_or(""), &vars)
                    ),
                );
                notes.push(json!("展示当前启动周期的预计通知时间，不是历史回放。"));
            } else {
                notes.push(json!("尚未读取游戏进程运行时间，无法预测通知。"));
            }
        }
        "name_filter" => {
            let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',steam_id,'name',name,'joinedAt',max(joined_at)) FROM player_sessions WHERE server_id=$1 GROUP BY steam_id,name ORDER BY max(joined_at) DESC LIMIT 5000").bind(&server.id).fetch_all(&state.db).await?;
            for r in &rows {
                if cfg["spareReserved"] == true
                    && reserved.contains(r["steamId"].as_str().unwrap_or(""))
                {
                    continue;
                }
                if let Some(verdict) =
                    crate::name_filter::verdict(cfg, r["name"].as_str().unwrap_or(""))
                {
                    push(
                        stamp(date(&r["joinedAt"]).unwrap().timestamp_millis()),
                        format!(
                            "{} {} ({}): {}",
                            cfg["action"].as_str().unwrap_or(""),
                            r["name"].as_str().unwrap_or(""),
                            r["steamId"].as_str().unwrap_or(""),
                            verdict["verdict"].as_str().unwrap_or("")
                        ),
                    )
                }
            }
            if let Some(last) = rows.last() {
                result["from"] = stamp(date(&last["joinedAt"]).unwrap().timestamp_millis())
            }
            notes.push(json!(format!(
                "检查历史中最新{}条昵称；最多5000条，使用席位快照豁免。",
                rows.len()
            )));
        }
        _ => {
            let rows: Vec<Value> = sqlx::query_scalar(
                "SELECT to_jsonb(s)FROM samples s WHERE server_id=$1 AND ts>=$2 ORDER BY ts",
            )
            .bind(&server.id)
            .bind(from)
            .fetch_all(&state.db)
            .await?;
            if rows.is_empty() {
                notes.push(json!("最近24小时没有采样记录。"));
            } else {
                match kind {
                    "broadcast" => {
                        let mut last = None;
                        let mut index = 0;
                        let messages = cfg["messages"].as_array().unwrap();
                        for r in &rows {
                            let ts = date(&r["ts"]).unwrap().timestamp_millis();
                            let count = r["player_count"].as_i64().unwrap_or(0);
                            if r["ok"] != true
                                || !p::wanted(cfg, count)
                                || last.is_some_and(|last| {
                                    ts - last < cfg["everyMinutes"].as_i64().unwrap_or(1) * 60000
                                })
                            {
                                continue;
                            }
                            last = Some(ts);
                            push(
                                stamp(ts),
                                format!(
                                    "broadcast ({count} on): {}",
                                    messages[index % messages.len()].as_str().unwrap()
                                ),
                            );
                            index += 1
                        }
                    }
                    "empty_reset" => {
                        let mut empty = None;
                        let mut last = None;
                        for r in &rows {
                            let ts = date(&r["ts"]).unwrap().timestamp_millis();
                            if r["ok"] != true || r["player_count"].as_i64().unwrap_or(0) != 0 {
                                empty = None;
                                continue;
                            }
                            let since = *empty.get_or_insert(ts);
                            let current = json!({"map":r["map"],"experiences":r["experiences"].as_str().unwrap_or("").split('+').filter(|s|!s.is_empty()).collect::<Vec<_>>()});
                            if e::on_target(cfg, &current)
                                || ts - since < cfg["afterMinutes"].as_i64().unwrap_or(1) * 60000
                                || last.is_some_and(|at| {
                                    ts - at < cfg["cooldownMinutes"].as_i64().unwrap_or(30) * 60000
                                })
                            {
                                continue;
                            }
                            last = Some(ts);
                            push(
                                stamp(ts),
                                format!(
                                    "reset {} → {} after {} min empty",
                                    r["map"].as_str().unwrap_or("?"),
                                    cfg["map"].as_str().unwrap_or(""),
                                    ((ts - since) as f64 / 60000.).round()
                                ),
                            );
                        }
                        notes.push(json!(
                            "实际重置会改变地图，同一次空服区间中的后续重复不会发生。"
                        ));
                    }
                    "seed_reward" => {
                        let sessions:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s)FROM player_sessions s WHERE server_id=$1 AND last_seen>=$2 ORDER BY joined_at LIMIT 5000").bind(&server.id).bind(from).fetch_all(&state.db).await?;
                        let settings = crate::settings::load(&state.db).await?;
                        let hold = 2 * settings["sampleMs"].as_i64().unwrap_or(30000) + 1000;
                        let totals =
                            seed_totals(&rows, &sessions, cfg, now.timestamp_millis(), hold);
                        let mut crossed: Vec<_> = totals
                            .iter()
                            .filter_map(|(steam, t)| {
                                t["crossedAt"].as_i64().map(|at| (at, steam, t))
                            })
                            .collect();
                        crossed.sort_by_key(|(at, _, _)| *at);
                        for (at, steam, t) in crossed {
                            if reserved.contains(steam) {
                                continue;
                            }
                            let name = sessions
                                .iter()
                                .rev()
                                .find(|s| s["steam_id"] == *steam)
                                .and_then(|s| s["name"].as_str())
                                .unwrap_or(steam);
                            push(
                                stamp(at),
                                format!(
                                    "reserve {name} ({steam}) {}: {} min",
                                    if cfg["scope"] == "server" {
                                        "here"
                                    } else {
                                        "across the organisation"
                                    },
                                    t["seconds"].as_i64().unwrap_or(0) / 60
                                ),
                            );
                        }
                        notes.push(json!(format!(
                            "仅回放最近24小时和前5000个会话；实时规则累计{}天。",
                            cfg["windowDays"]
                        )));
                    }
                    "match_broadcast" => {
                        let settings = crate::settings::load(&state.db).await?;
                        let hold = 2 * settings["sampleMs"].as_i64().unwrap_or(30000) + 1000;
                        let mut prev: Option<Value> = None;
                        let mut failed = 0;
                        let mut held: Option<(i64, Value)> = None;
                        for r in &rows {
                            if r["ok"] != true {
                                failed += 1;
                                if failed > 1 {
                                    prev = None
                                }
                                continue;
                            }
                            failed = 0;
                            let ts = date(&r["ts"]).unwrap().timestamp_millis();
                            let current = json!({"map":r["map"].as_str().unwrap_or(""),"scores":r["scores"].as_array().cloned().unwrap_or_default(),"matchSeconds":null});
                            let end = prev
                                .as_ref()
                                .filter(|old| ts - old["ts"].as_i64().unwrap_or(0) <= hold)
                                .and_then(|old| {
                                    crate::observation_state::boundary(Some(old), &current)
                                });
                            if let Some(end) = end {
                                if !held.as_ref().is_some_and(|(at, _)| {
                                    ts - *at <= 180000
                                        && end["leaders"].as_array().is_none_or(|a| a.is_empty())
                                }) {
                                    held = Some((ts, end))
                                }
                            }
                            if held.as_ref().is_some_and(|(at, _)| ts - *at > 180000) {
                                held = None
                            }
                            let count = r["player_count"].as_i64().unwrap_or(0);
                            if count >= cfg["minPlayers"].as_i64().unwrap_or(1) {
                                if let Some((_, end)) = held.take() {
                                    for send in p::match_messages(
                                        cfg,
                                        &end,
                                        count,
                                        &json!({"server":server.name,"map":r["map"],"players":count,"max":"…","cap":100,"mvp":"…","top":"…"}),
                                        &[],
                                    ) {
                                        push(
                                            stamp(ts),
                                            format!(
                                                "broadcast ({count} on): {}",
                                                send["message"].as_str().unwrap_or("")
                                            ),
                                        )
                                    }
                                }
                            }
                            let mut current = current;
                            current["ts"] = json!(ts);
                            prev = Some(current);
                        }
                        notes.push(json!(
                            "按历史采样中的地图或分数重置回放；采样不含玩家成绩，奖项姓名无法回放。"
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
    drop(push);
    result["fires"] = json!(fires);
    result["items"] = json!(items);
    result["notes"] = json!(notes);
    Ok(result)
}
