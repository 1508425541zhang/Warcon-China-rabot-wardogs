//! Advisory Steam/local risk, scoped to servers the reader may access.
use crate::{config::AppState, error::Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;
fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.)
}
pub fn account_age(v: &Value, now: i64) -> Option<i64> {
    crate::integrity_enforcement::date(v).map(|t| {
        ((now - t.timestamp_millis()) as f64 / 86400000.)
            .floor()
            .max(0.) as i64
    })
}
pub fn normal_name(s: &str) -> String {
    s.to_lowercase()
        .nfkd()
        .flat_map(|c| match c {
            '0' => vec!['o'],
            '1' | '!' => vec!['i'],
            '3' => vec!['e'],
            '4' | '@' => vec!['a'],
            '5' | '$' => vec!['s'],
            '7' => vec!['t'],
            '8' => vec!['b'],
            '|' => vec!['l'],
            _ => vec![c],
        })
        .filter(char::is_ascii_lowercase)
        .collect()
}
pub fn edit(a: &str, b: &str) -> usize {
    let a: Vec<_> = a.chars().collect();
    let b: Vec<_> = b.chars().collect();
    let mut prev: Vec<_> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            cur.push(
                (prev[j + 1] + 1)
                    .min(cur[j] + 1)
                    .min(prev[j] + usize::from(x != y)),
            )
        }
        prev = cur
    }
    prev[b.len()]
}
pub fn resembles(a: &str, b: &str) -> bool {
    let a = normal_name(a);
    let b = normal_name(b);
    let shorter = a.len().min(b.len());
    if shorter < 4 {
        return false;
    }
    a == b
        || shorter >= 5 && (a.contains(&b) || b.contains(&a))
        || shorter >= 6 && edit(&a, &b) <= 1
        || shorter >= 10 && edit(&a, &b) <= 2
}
pub fn assess(s: &Value, now: i64) -> Value {
    let mut reasons = vec![];
    let mut add = |code: &str, text: String, weight: f64| {
        reasons.push(json!({"code":code,"text":text,"weight":weight}))
    };
    let profile = &s["profile"];
    let checked = profile.is_object() && profile["error"].as_str().is_none_or(str::is_empty);
    if checked {
        let age = num(&profile["daysSinceLastBan"]);
        let bw = |base: f64| {
            if profile["daysSinceLastBan"].is_null() {
                base
            } else if age < 30. {
                base + 15.
            } else if age < 365. {
                base + 8.
            } else if age < 1095. {
                base
            } else {
                (base - 15.).max(5.)
            }
        };
        let vac = num(&profile["vacBans"]);
        if vac > 0. {
            add(
                "vac",
                format!(
                    "{vac} VAC ban{}{}",
                    if vac == 1. { "" } else { "s" },
                    if profile["daysSinceLastBan"].is_null() {
                        String::new()
                    } else {
                        format!(", last {age} days ago")
                    }
                ),
                70f64.min(bw(45.) + 10f64.min((vac - 1.) * 5.)),
            )
        }
        let bans = num(&profile["gameBans"]);
        if bans > 0. {
            add(
                "gameban",
                format!(
                    "{bans} game ban{}{}",
                    if bans == 1. { "" } else { "s" },
                    if profile["daysSinceLastBan"].is_null() {
                        String::new()
                    } else {
                        format!(", last {age} days ago")
                    }
                ),
                45f64.min(bw(25.) + 5f64.min((bans - 1.) * 5.)),
            )
        }
        if profile["communityBanned"] == true {
            add("community", "Steam community ban".into(), 10.)
        }
        if let Some(b) = profile["economyBan"]
            .as_str()
            .filter(|v| !v.is_empty() && *v != "none")
        {
            add("economy", format!("Steam economy ban ({b})"), 5.)
        }
        if profile["public"] != true {
            add("private", "Steam profile is private".into(), 12.)
        }
        if let Some(age) = account_age(&profile["accountCreatedAt"], now).filter(|n| *n < 90) {
            add(
                "age",
                format!(
                    "Steam account is {age} day{} old",
                    if age == 1 { "" } else { "s" }
                ),
                if age < 7 {
                    30.
                } else if age < 30 {
                    20.
                } else {
                    10.
                },
            )
        }
        if profile["friendsState"] == "private" {
            add(
                "friends_private",
                "Steam friends list is private".into(),
                8.,
            )
        }
        let banned = num(&profile["bannedFriends"]);
        if banned > 0. {
            add(
                "banned_friends",
                format!(
                    "{banned} banned Steam friend{} among {} checked{}",
                    if banned == 1. { "" } else { "s" },
                    num(&profile["friendsChecked"]),
                    if profile["friendsState"] == "partial" {
                        format!(" of {}", num(&profile["friendsTotal"]))
                    } else {
                        String::new()
                    }
                ),
                30f64.min(6. + banned * 5.),
            )
        }
    }
    let p = &s["performance"];
    let decided = num(&p["wins"]) + num(&p["losses"]) + num(&p["draws"]);
    let win = num(&p["wins"]) / decided;
    let kills = num(&p["kills"]);
    let deaths = num(&p["deaths"]);
    if decided >= 20. && win >= 0.8 {
        add(
            "win_rate",
            format!(
                "{}% wins across {decided} recorded matches",
                (win * 100.).round()
            ),
            8.,
        )
    }
    if kills >= 100. && deaths >= 20. && kills / deaths >= 4. {
        add(
            "kd",
            format!(
                "{:.1} K/D across {kills} kills and {deaths} deaths",
                kills / deaths
            ),
            10.,
        )
    }
    let feed = num(&p["feedKills"]);
    let heads = num(&p["headshots"]) / feed;
    if feed >= 50. && heads >= 0.6 {
        add(
            "headshots",
            format!(
                "{}% headshots across {feed} kill-feed kills",
                (heads * 100.).round()
            ),
            10.,
        )
    }
    for b in s["bannedOn"].as_array().into_iter().flatten() {
        let reason = b["reason"].as_str().unwrap_or("");
        add(
            "banned_elsewhere",
            format!(
                "Banned on {}{}",
                b["serverName"].as_str().unwrap_or(""),
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(": {reason}")
                }
            ),
            60.,
        )
    }
    for r in s["resembles"].as_array().into_iter().flatten().take(3) {
        add(
            "resembles",
            format!(
                "Name resembles banned {} ({}) on {}",
                r["name"].as_str().unwrap_or(""),
                r["steamId"].as_str().unwrap_or(""),
                r["serverName"].as_str().unwrap_or("")
            ),
            20.,
        )
    }
    if s["watched"].is_object() {
        let reason = s["watched"]["reason"].as_str().unwrap_or("");
        add(
            "watchlist",
            format!(
                "On the watchlist{}",
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(": {reason}")
                }
            ),
            15.,
        )
    }
    let score = reasons
        .iter()
        .map(|r| num(&r["weight"]))
        .sum::<f64>()
        .clamp(0., 100.);
    json!({"score":score,"level":if score>=50.{"high"}else if score>=20.{"medium"}else{"low"},"reasons":reasons,"steamChecked":checked})
}
pub fn verdict(cfg: &Value, s: &Value, now: i64) -> Option<String> {
    if s["reserved"] == true && cfg["spareReserved"] == true {
        return None;
    }
    if cfg["bannedElsewhere"] == true {
        if let Some(b) = s["bannedOn"].as_array().and_then(|a| a.first()) {
            let reason = b["reason"].as_str().unwrap_or("");
            return Some(format!(
                "banned on {}{}",
                b["serverName"].as_str().unwrap_or(""),
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(" ({reason})")
                }
            ));
        }
    }
    if cfg["watchlist"] == true && s["watched"].is_object() {
        let r = s["watched"]["reason"].as_str().unwrap_or("");
        return Some(format!(
            "on the watchlist{}",
            if r.is_empty() {
                String::new()
            } else {
                format!(" ({r})")
            }
        ));
    }
    let p = &s["profile"];
    if s["steamEnabled"] == true && p.is_object() && p["error"].as_str().is_none_or(str::is_empty) {
        let max = num(&cfg["maxBanAgeDays"]);
        let recent =
            max == 0. || p["daysSinceLastBan"].is_null() || num(&p["daysSinceLastBan"]) <= max;
        for (enabled, k, label) in [
            ("vacBans", "vacBans", "VAC"),
            ("gameBans", "gameBans", "game"),
        ] {
            let bans = num(&p[k]);
            if cfg[enabled] == true && bans > 0. && recent {
                return Some(format!(
                    "{bans} {label} ban{} on record",
                    if bans == 1. { "" } else { "s" }
                ));
            }
        }
        let min = num(&cfg["minAccountDays"]);
        if min > 0. {
            match account_age(&p["accountCreatedAt"], now) {
                None if cfg["privateProfiles"] == true => {
                    return Some("private profile, account age unknown".into());
                }
                Some(n) if (n as f64) < min => {
                    return Some(format!(
                        "Steam account only {n} day{} old (minimum {min})",
                        if n == 1 { "" } else { "s" }
                    ));
                }
                _ => {}
            }
        }
    }
    if let Some(min) = crate::trigger_policy::risk_score(cfg) {
        let risk = assess(s, now);
        if num(&risk["score"]) >= min as f64 {
            let mut reasons = risk["reasons"].as_array().cloned().unwrap_or_default();
            reasons.sort_by(|a, b| num(&b["weight"]).total_cmp(&num(&a["weight"])));
            let why = reasons
                .iter()
                .take(3)
                .filter_map(|r| r["text"].as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let score = num(&risk["score"]);
            let what = if risk["level"] == "low" {
                format!("risk {score}")
            } else {
                format!("{} risk ({score})", risk["level"].as_str().unwrap())
            };
            return Some(format!(
                "{what}: {why}{}",
                if risk["steamChecked"] == true {
                    ""
                } else {
                    " [Steam not checked]"
                }
            ));
        }
    }
    None
}
pub async fn inputs(
    state: &AppState,
    org: &str,
    ids: &[String],
    current: Option<&str>,
    players: &[Value],
    with_names: bool,
) -> Result<HashMap<String, Value>> {
    let steam: Vec<_> = players
        .iter()
        .filter_map(|p| p["steamId"].as_str())
        .filter(|s| crate::api::notes::steam_id(s).is_ok())
        .map(str::to_owned)
        .collect();
    if steam.is_empty() {
        return Ok(HashMap::new());
    }
    let profiles: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM steam_profiles p WHERE steam_id=ANY($1)")
            .bind(&steam)
            .fetch_all(&state.db)
            .await?;
    let marks:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',steam_id,'reason',reason) FROM player_marks WHERE org_id=$1 AND watched AND steam_id=ANY($2)").bind(org).bind(&steam).fetch_all(&state.db).await?;
    let bans:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',b.steam_id,'serverId',b.server_id,'serverName',s.name,'reason',b.reason,'bannedBy',b.banned_by) FROM server_bans b JOIN servers s ON s.id=b.server_id WHERE b.server_id=ANY($1) LIMIT 5000").bind(ids).fetch_all(&state.db).await?;
    let banned_ids: Vec<_> = bans
        .iter()
        .filter_map(|b| b["steamId"].as_str())
        .map(str::to_owned)
        .take(2000)
        .collect();
    let names: Vec<Value> = if with_names {
        sqlx::query_scalar("SELECT DISTINCT ON(steam_id) jsonb_build_object('steamId',steam_id,'name',name) FROM player_sessions WHERE server_id=ANY($1) AND steam_id=ANY($2) ORDER BY steam_id,last_seen DESC").bind(ids).bind(banned_ids).fetch_all(&state.db).await?
    } else {
        vec![]
    };
    let performance = performance(state, ids, &steam).await?;
    let reserved: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT r.steam_id FROM server_reserved r WHERE server_id=$1")
            .bind(current.unwrap_or(""))
            .fetch_all(&state.db)
            .await?;
    let steam_enabled = std::env::var("STEAM_API_KEY").is_ok_and(|s| !s.trim().is_empty());
    let mut out = HashMap::new();
    for player in players {
        let Some(id) = player["steamId"].as_str() else {
            continue;
        };
        let profile = profiles
            .iter()
            .find(|p| p["steam_id"] == id)
            .cloned()
            .map(crate::ai_evidence::row)
            .unwrap_or(Value::Null);
        let watched = marks
            .iter()
            .find(|m| m["steamId"] == id)
            .map(|m| json!({"reason":m["reason"]}))
            .unwrap_or(Value::Null);
        let banned: Vec<_> = bans
            .iter()
            .filter(|b| b["steamId"] == id && b["serverId"].as_str() != current)
            .cloned()
            .collect();
        let resembles:Vec<_>=names.iter().filter(|n|n["steamId"]!=id&&resembles(player["name"].as_str().unwrap_or(""),n["name"].as_str().unwrap_or(""))).take(5).map(|n|json!({"name":n["name"],"steamId":n["steamId"],"serverName":bans.iter().find(|b|b["steamId"]==n["steamId"]).map(|b|b["serverName"].clone()).unwrap_or(json!(""))})).collect();
        out.entry(id.into()).or_insert(json!({"profile":profile,"steamEnabled":steam_enabled,"watched":watched,"bannedOn":banned,"resembles":resembles,"reserved":reserved.iter().any(|r|r==id),"performance":performance.get(id)}));
    }
    Ok(out)
}
pub async fn performance(
    state: &AppState,
    ids: &[String],
    players: &[String],
) -> Result<HashMap<String, Value>> {
    let mut out = HashMap::new();
    if ids.is_empty() || players.is_empty() {
        return Ok(out);
    }
    let feed:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steamId',killer_steam_id,'feedKills',count(*) FILTER(WHERE NOT suicide),'headshots',count(*) FILTER(WHERE headshot AND NOT suicide)) FROM kills WHERE server_id=ANY($1) AND killer_steam_id=ANY($2) GROUP BY killer_steam_id").bind(ids).bind(players).fetch_all(&state.db).await?;
    // The same exact match/session reconciliation used by the career view.
    let lines = include_str!("../sql/leaderboard-lines.sql").replace(
        "($3::text IS NULL OR p.steam_id=$3)",
        "p.steam_id=ANY($3::text[])",
    );
    let query = format!(
        "WITH {lines} SELECT jsonb_build_object('steamId',steam_id,'matches',count(*),'wins',count(*)FILTER(WHERE result='win'),'losses',count(*)FILTER(WHERE result='loss'),'draws',count(*)FILTER(WHERE result='draw'),'kills',COALESCE(sum(kills),0),'deaths',COALESCE(sum(deaths),0)) FROM lines GROUP BY steam_id"
    );
    let played: Vec<Value> = sqlx::query_scalar(&query)
        .bind(ids)
        .bind(chrono::DateTime::from_timestamp(0, 0).unwrap())
        .bind(players)
        .fetch_all(&state.db)
        .await?;
    for row in played {
        out.insert(row["steamId"].as_str().unwrap().to_owned(), row);
    }
    for id in players {
        let p = out
            .entry(id.clone())
            .or_insert(json!({"matches":0,"wins":0,"losses":0,"draws":0,"kills":0,"deaths":0}));
        let f = feed.iter().find(|p| p["steamId"] == *id);
        p["feedKills"] = f.map(|f| f["feedKills"].clone()).unwrap_or(json!(0));
        p["headshots"] = f.map(|f| f["headshots"].clone()).unwrap_or(json!(0));
    }
    Ok(out)
}
