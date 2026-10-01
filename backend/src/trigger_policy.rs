//! Original automation settings, time windows, templates and round awards.
use crate::{
    error::{ApiError, Result},
    http::{integer, string, truthy},
};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
pub const KINDS: [&str; 12] = [
    "welcome",
    "faction_change",
    "broadcast",
    "empty_reset",
    "risk_kick",
    "ping_kick",
    "restart_notice",
    "team_kill",
    "seed_reward",
    "match_broadcast",
    "name_filter",
    "kill_rate",
];
pub const AWARDS: [&str; 6] = ["mvp", "multi", "streak", "earned", "rich", "time"];
pub fn label(kind: &str) -> &str {
    match kind {
        "welcome" => "Welcome whisper",
        "faction_change" => "Faction change whisper",
        "broadcast" => "Scheduled broadcast",
        "empty_reset" => "Empty-server map reset",
        "risk_kick" => "Kick on connect risk",
        "ping_kick" => "High ping kick",
        "restart_notice" => "Restart notice",
        "team_kill" => "Team kill limit",
        "seed_reward" => "Seeding reward",
        "match_broadcast" => "Match broadcast",
        "name_filter" => "Name filter",
        "kill_rate" => "Kill rate watch",
        _ => "Unknown",
    }
}
pub fn needed(kind: &str, cfg: &Value) -> &'static str {
    match kind {
        "welcome" | "faction_change" | "broadcast" | "restart_notice" | "match_broadcast" => {
            "chat.send"
        }
        "empty_reset" => "match.control",
        "seed_reward" if cfg["scope"] != "server" => "lists.reserve",
        "seed_reward" => "slots.manage",
        _ => "players.moderate",
    }
}
pub fn risk_score(c: &Value) -> Option<i64> {
    if !c["kickAtScore"].is_null() {
        return crate::http::js_number(&c["kickAtScore"])
            .filter(|n| n.trunc() >= 1.)
            .map(|n| n.trunc().min(100.) as i64);
    }
    match c["kickAtLevel"].as_str() {
        Some("high") => Some(50),
        Some("medium") => Some(20),
        _ => None,
    }
}
fn nullable(v: &Value) -> bool {
    v.is_null() || v == ""
}
fn text(c: &Value, key: &str, default: &str) -> String {
    let s = string(&c[key], 200);
    if s.is_empty() { default.into() } else { s }
}
fn required(c: &Value, key: &str, max: usize) -> Result<String> {
    let s = string(&c[key], max);
    if s.is_empty() {
        Err(ApiError::bad(format!("{key} 不能为空。")))
    } else {
        Ok(s)
    }
}
pub fn default_awards() -> Value {
    json!({"enabled":false,"templates":{"mvp":"本局MVP：{name}，KD {value}（{kills}杀/{deaths}死）","multi":"最佳单次多杀：{name}，同一时间戳 {value} 杀","streak":"最佳一命连杀：{name}，{value} 杀","earned":"本场赚钱最多（观测净增）：{name}，${value}","rich":"本场最富玩家：{name}，赛末观测持有 ${value}","time":"在线最久：{name}，{value} 分钟"}})
}
pub fn validate(kind: &str, c: &Value) -> Result<Value> {
    let b = |k| truthy(&c[k]);
    let n = |k, d, min, max| integer(&c[k], d, min, max);
    let out = match kind {
        "welcome" => {
            json!({"message":required(c,"message",200)?,"onlyFirstVisit":b("onlyFirstVisit"),"afterFaction":b("afterFaction")})
        }
        "faction_change" => json!({"message":required(c,"message",200)?}),
        "broadcast" => {
            let messages: Vec<_> = c["messages"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| {
                    string(&c["messages"], usize::MAX)
                        .split('\n')
                        .map(|s| json!(s))
                        .collect()
                })
                .iter()
                .map(|v| string(v, 200))
                .filter(|s| !s.is_empty())
                .take(20)
                .collect();
            let every = n("everyMinutes", 0, 0, 1440);
            let min = n("minPlayers", 1, 0, 1000);
            let max = if nullable(&c["maxPlayers"]) {
                None
            } else {
                Some(n("maxPlayers", 0, 0, 1000))
            };
            if messages.is_empty() || every == 0 || max.is_some_and(|m| m < min) {
                return Err(ApiError::bad(
                    "至少一条广播；间隔1～1440分钟，人数上限不得低于下限。",
                ));
            }
            json!({"messages":messages,"everyMinutes":every,"minPlayers":min,"maxPlayers":max})
        }
        "empty_reset" => {
            let after = n("afterMinutes", 0, 0, 1440);
            if after == 0 {
                return Err(ApiError::bad("空服时间应为1～1440分钟。"));
            }
            json!({"map":required(c,"map",100)?,"experiences":c["experiences"].as_array().into_iter().flatten().map(|v|string(v,100)).filter(|s|!s.is_empty()).take(10).collect::<Vec<_>>(),"lighting":string(&c["lighting"],100),"zoneAlternator":string(&c["zoneAlternator"],200),"afterMinutes":after,"cooldownMinutes":n("cooldownMinutes",30,1,1440)})
        }
        "risk_kick" => {
            let score = risk_score(c);
            let age = n("minAccountDays", 0, 0, 3650);
            if !b("vacBans")
                && !b("gameBans")
                && age == 0
                && !b("bannedElsewhere")
                && !b("watchlist")
                && score.is_none()
            {
                return Err(ApiError::bad("至少启用一项检查。"));
            }
            json!({"vacBans":b("vacBans"),"gameBans":b("gameBans"),"maxBanAgeDays":n("maxBanAgeDays",0,0,36500),"minAccountDays":age,"privateProfiles":b("privateProfiles"),"bannedElsewhere":b("bannedElsewhere"),"watchlist":b("watchlist"),"kickAtScore":score,"spareReserved":c.get("spareReserved").is_none_or(truthy),"reason":text(c,"reason","Your account does not meet this server’s requirements.")})
        }
        "ping_kick" => {
            let ping = n("maxPingMs", 200, 0, 2000);
            let duration = n("durationSeconds", 60, 0, 3600);
            if ping == 0 || duration == 0 {
                return Err(ApiError::bad("延迟和持续时间必须为正数。"));
            }
            json!({"maxPingMs":ping,"durationSeconds":duration,"reason":text(c,"reason","Ping too high for too long.")})
        }
        "restart_notice" => {
            let lead = n("leadMinutes", 0, 0, 1439);
            let lead_text = string(&c["leadMessage"], 200);
            if lead > 0 && lead_text.is_empty() {
                return Err(ApiError::bad("提前通知文字不能为空。"));
            }
            json!({"message":required(c,"message",200)?,"leadMinutes":lead,"leadMessage":lead_text,"repeatMinutes":n("repeatMinutes",0,0,1440),"minPlayers":n("minPlayers",1,0,1000)})
        }
        "team_kill" => {
            let warn = n("warnAt", 0, 0, 100);
            let kick = n("kickAt", 0, 0, 100);
            if warn == 0 && kick == 0 || warn > 0 && kick > 0 && kick < warn {
                return Err(ApiError::bad("请设置队杀阈值；踢出阈值不得低于警告。"));
            }
            json!({"warnAt":warn,"kickAt":kick,"warnMessage":text(c,"warnMessage","Careful, {name}: that was a team kill ({count} this session)."),"kickReason":text(c,"kickReason","Team killing ({count} this session).")})
        }
        "seed_reward" => {
            let minutes = n("minutes", 0, 0, 90 * 1440);
            let days = n("windowDays", 7, 1, 90);
            let low = n("lowAt", 20, 1, 1000);
            let full = if nullable(&c["fullAt"]) {
                None
            } else {
                Some(n("fullAt", 0, 1, 1000))
            };
            if minutes == 0 || minutes > days * 1440 || full.is_some_and(|v| v <= low) {
                return Err(ApiError::bad("种服时长或满服人数无效。"));
            }
            json!({"scope":if c["scope"]=="server"{"server"}else{"org"},"minutes":minutes,"windowDays":days,"lowAt":low,"fullAt":full,"untilFull":c.get("untilFull").is_none_or(truthy),"slotDays":n("slotDays",7,1,365),"message":string(&c["message"],200)})
        }
        "match_broadcast" => {
            let awards = if c["awards"].is_null() {
                default_awards()
            } else {
                c["awards"].clone()
            };
            let valid = awards.as_object().is_some_and(|a| {
                a.len() == 2 && a.contains_key("enabled") && a.contains_key("templates")
            }) && awards["enabled"].is_boolean()
                && awards["templates"].as_object().is_some_and(|a| {
                    a.len() == 6
                        && AWARDS.iter().all(|k| {
                            a[*k]
                                .as_str()
                                .is_some_and(|s| s.encode_utf16().count() <= 200)
                        })
                });
            if !valid {
                return Err(ApiError::bad("赛后奖项模板格式无效，每条最多200字符。"));
            }
            let end = string(&c["endMessage"], 200);
            let start = string(&c["startMessage"], 200);
            if end.is_empty() && start.is_empty() && awards["enabled"] != true {
                return Err(ApiError::bad("至少启用赛后、开局文字或奖项。"));
            }
            let mut out = json!({"endMessage":end,"startMessage":start,"minPlayers":n("minPlayers",1,0,1000)});
            if c.get("awards").is_some_and(truthy) {
                out["awards"] = awards
            }
            out
        }
        "name_filter" => crate::name_filter::validate(c)?,
        "kill_rate" => {
            let kills = n("maxKills", 0, 0, 1000);
            let heads = n("headshotPct", 0, 0, 100);
            if kills == 0 && heads == 0 {
                return Err(ApiError::bad("请设置击杀数或爆头率阈值。"));
            }
            json!({"windowMinutes":n("windowMinutes",5,1,60),"maxKills":kills,"headshotPct":heads,"headshotMinKills":n("headshotMinKills",15,1,1000),"cooldownMinutes":n("cooldownMinutes",30,1,1440)})
        }
        _ => return Err(ApiError::bad("未知自动化类型。")),
    };
    Ok(out)
}
pub fn render(text: &str, vars: &Value) -> String {
    let lower: HashMap<_, _> = vars
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| {
            (
                k.to_lowercase(),
                match v {
                    Value::String(s) => s.clone(),
                    _ => v.to_string(),
                },
            )
        })
        .collect();
    let re = Regex::new(r"(?i)\{([a-z_]+)\}").unwrap();
    crate::feed::truncate(
        &re.replace_all(text, |c: &regex::Captures| {
            lower
                .get(&c[1].to_lowercase())
                .cloned()
                .unwrap_or(c[0].into())
        }),
        200,
    )
}
pub fn wanted(cfg: &Value, count: i64) -> bool {
    count >= cfg["minPlayers"].as_i64().unwrap_or(1)
        && cfg["maxPlayers"].as_i64().is_none_or(|v| count <= v)
}
pub fn team_stage(cfg: &Value, count: i64) -> Option<&'static str> {
    let k = cfg["kickAt"].as_i64().unwrap_or(0);
    let w = cfg["warnAt"].as_i64().unwrap_or(0);
    if k > 0 && count >= k {
        Some("kick")
    } else if w > 0 && count >= w {
        Some("warn")
    } else {
        None
    }
}
pub fn ping_step(cfg: &Value, prev: &Value, players: &[Value], now: i64, gap: i64) -> Value {
    let valid = prev["lastAt"]
        .as_i64()
        .is_some_and(|t| now >= t && now - t <= gap);
    let mut next = json!({"lastAt":now,"players":{}});
    let mut kicks = vec![];
    for p in players {
        if p["ping"]
            .as_f64()
            .is_none_or(|n| n <= cfg["maxPingMs"].as_f64().unwrap_or(200.))
        {
            continue;
        }
        let Some(id) = p["steamId"].as_str() else {
            continue;
        };
        let mut streak = if valid && prev["players"][id].is_object() {
            prev["players"][id].clone()
        } else {
            json!({"since":now,"fired":false})
        };
        if streak["fired"] != true
            && now - streak["since"].as_i64().unwrap_or(now)
                >= cfg["durationSeconds"].as_i64().unwrap_or(60) * 1000
        {
            streak["fired"] = json!(true);
            kicks.push(id)
        }
        next["players"][id] = streak;
    }
    json!({"state":next,"kicks":kicks})
}
pub fn restart_stage(
    cfg: &Value,
    prev: &Value,
    started: i64,
    count: i64,
    now: i64,
) -> Option<Value> {
    if started == 0 || count < cfg["minPlayers"].as_i64().unwrap_or(1) || now < started {
        return None;
    }
    let until = started + 86400000 - now;
    let mut state = if prev["startedAt"]
        .as_i64()
        .is_some_and(|p| p.abs_diff(started) <= 60000)
    {
        prev.clone()
    } else {
        json!({"startedAt":started})
    };
    if until <= 0 {
        let repeat = cfg["repeatMinutes"].as_i64().unwrap_or(0);
        if state["dueAt"]
            .as_i64()
            .is_some_and(|last| last != 0 && (repeat == 0 || now - last < repeat * 60000))
        {
            return None;
        }
        state["dueAt"] = json!(now);
        Some(json!({"stage":"due","minutes":0,"state":state}))
    } else {
        let lead = cfg["leadMinutes"].as_i64().unwrap_or(0);
        if lead == 0 || truthy(&state["leadAt"]) || until > lead * 60000 {
            return None;
        }
        state["leadAt"] = json!(now);
        Some(
            json!({"stage":"lead","minutes":((until as f64/60000.).round() as i64).max(1),"state":state}),
        )
    }
}
#[derive(Clone, Default)]
pub struct Track {
    pub at: Vec<i64>,
    pub head: Vec<bool>,
    pub flagged: Option<i64>,
}
pub fn rate_verdict(cfg: &Value, kills: usize, heads: usize) -> Option<String> {
    let minutes = cfg["windowMinutes"].as_i64().unwrap_or(5);
    let within = format!("in {minutes} min");
    let max = cfg["maxKills"].as_u64().unwrap_or(0);
    if max > 0 && kills as u64 >= max {
        return Some(format!(
            "{kills} kill{} {within}",
            if kills == 1 { "" } else { "s" }
        ));
    }
    let pct = cfg["headshotPct"].as_f64().unwrap_or(0.);
    if pct > 0. && kills as u64 >= cfg["headshotMinKills"].as_u64().unwrap_or(15) {
        let rate = (100. * heads as f64 / kills as f64).round();
        if rate >= pct {
            return Some(format!("{rate}% headshots over {kills} kills {within}"));
        }
    }
    None
}
pub fn rate_step(cfg: &Value, t: &mut Track, at: i64, head: bool) -> Option<String> {
    let i = t.at.partition_point(|v| *v <= at);
    t.at.insert(i, at);
    t.head.insert(i, head);
    let newest = *t.at.last().unwrap();
    let from = newest - cfg["windowMinutes"].as_i64().unwrap_or(5) * 60000;
    let old = t.at.partition_point(|v| *v <= from);
    t.at.drain(..old);
    t.head.drain(..old);
    if t.flagged
        .is_some_and(|f| newest - f < cfg["cooldownMinutes"].as_i64().unwrap_or(30) * 60000)
    {
        return None;
    }
    let verdict = rate_verdict(cfg, t.at.len(), t.head.iter().filter(|b| **b).count());
    if verdict.is_some() {
        t.flagged = Some(newest)
    }
    verdict
}
fn random_rank(seed: &str) -> u32 {
    let mut n = 2166136261u32;
    for c in seed.chars() {
        let first_utf16 = if c as u32 > 0xffff {
            0xd800 + ((c as u32 - 0x10000) >> 10)
        } else {
            c as u32
        };
        n = (n ^ first_utf16).wrapping_mul(16777619)
    }
    n ^= n >> 16;
    n = n.wrapping_mul(0x85ebca6b);
    n ^= n >> 13;
    n
}
pub fn award_winners(lines: &[Value]) -> Vec<Value> {
    AWARDS
        .iter()
        .filter_map(|key| {
            let mut ranked: Vec<_> = lines
                .iter()
                .filter_map(|p| {
                    let value = match *key {
                        "mvp" => p["deaths"]
                            .as_f64()
                            .and_then(|d| p["kills"].as_f64().map(|k| k / d.max(1.))),
                        "multi" => p["multi"].as_f64(),
                        "streak" => p["streak"].as_f64(),
                        "earned" => p["cashDelta"].as_f64(),
                        "rich" => p["cashHeld"].as_f64(),
                        _ => p["seconds"].as_f64(),
                    }?;
                    if value <= 0. || *key == "multi" && value < 2. {
                        None
                    } else {
                        let id = p["steamId"].as_str().or(p["name"].as_str()).unwrap_or("");
                        let rank = random_rank(&format!(
                            "{}:{key}:{id}",
                            p["awardSeed"].as_str().unwrap_or("undefined")
                        ));
                        Some((p, value, rank, id))
                    }
                })
                .collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)).then(a.3.cmp(b.3)));
            let (p, value, _, _) = ranked.first()?;
            let text = match *key {
                "mvp" => format!("{value:.2}"),
                "time" => format!("{:.1}", value / 60.),
                _ => value.to_string(),
            };
            Some(json!({"key":key,"player":p,"value":text}))
        })
        .collect()
}
pub fn match_messages(
    cfg: &Value,
    end: &Value,
    count: i64,
    vars: &Value,
    lines: &[Value],
) -> Vec<Value> {
    if count < cfg["minPlayers"].as_i64().unwrap_or(1) {
        return vec![];
    }
    let scores = end["scores"].as_array().cloned().unwrap_or_default();
    let leaders = end["leaders"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let mut ranked: Vec<_> = lines
        .iter()
        .filter(|p| p["kills"].as_i64().unwrap_or(0) > 0)
        .collect();
    ranked.sort_by_key(|p| std::cmp::Reverse(p["kills"].as_i64().unwrap_or(0)));
    let best = ranked.first().map(|p| p["kills"].clone());
    let mut vars = vars.clone();
    for (k,v) in json!({"faction":leaders.join(" and "),"score":scores.first().map(|s|s["score"].clone()).unwrap_or(json!(0)),"scores":scores.iter().map(|s|format!("{} {}",s["name"].as_str().unwrap_or(""),s["score"])).collect::<Vec<_>>().join(" · "),"previous":end["map"],"mvp":ranked.iter().filter(|p|Some(p["kills"].clone())==best).filter_map(|p|p["name"].as_str()).collect::<Vec<_>>().join(" and "),"top":ranked.iter().take(3).map(|p|format!("{} {}",p["name"].as_str().unwrap_or(""),p["kills"])).collect::<Vec<_>>().join(" · ")}).as_object().unwrap(){vars[k]=v.clone()}
    let mut out = vec![];
    let end_text = cfg["endMessage"].as_str().unwrap_or("");
    if !end_text.is_empty() && !leaders.is_empty() {
        out.push(json!({"stage":"end","message":render(end_text,&vars)}))
    }
    if cfg["awards"]["enabled"] == true {
        for award in award_winners(lines) {
            let key = award["key"].as_str().unwrap();
            let template = cfg["awards"]["templates"][key].as_str().unwrap_or("");
            if template.trim().is_empty() {
                continue;
            }
            let mut args = vars.clone();
            args["name"] = award["player"]["name"].clone();
            args["value"] = award["value"].clone();
            args["kills"] = award["player"]["kills"].clone();
            args["deaths"] = if award["player"]["deaths"].is_null() {
                json!("—")
            } else {
                award["player"]["deaths"].clone()
            };
            out.push(json!({"stage":format!("award_{key}"),"message":render(template,&args)}))
        }
    }
    let start = cfg["startMessage"].as_str().unwrap_or("");
    if !start.is_empty() {
        out.push(json!({"stage":"start","message":render(start,&vars)}))
    }
    out
}
