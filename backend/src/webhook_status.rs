//! Three live card layouts and revision-fenced Discord status mirroring.
use crate::{config::AppState, error::Result, webhooks as h};
use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};
pub fn escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if "\\`*_~|<>[]".contains(c) {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}
fn n(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.)
}
fn pretty(s: &str) -> String {
    let mut out = String::new();
    let mut previous = ' ';
    for c in s.chars() {
        if c == '_' || c == '-' {
            out.push(' ')
        } else {
            if (previous.is_ascii_lowercase() || previous.is_ascii_digit())
                && c.is_ascii_uppercase()
            {
                out.push(' ')
            }
            out.push(c)
        }
        previous = c;
    }
    out.trim().to_owned()
}
fn display(map: &str) -> String {
    match map {
        "Kavkazi" => "Bakurani",
        "Europe" => "Ozeti",
        "NorthAmerica" => "Zestafona",
        _ => return pretty(map),
    }
    .into()
}
fn map_id(map: &str) -> &str {
    match map {
        "Bakurani" => "Kavkazi",
        "Ozeti" => "Europe",
        "Zestafona" => "NorthAmerica",
        _ => map,
    }
}
pub fn square(hex: &str, name: &str, index: usize) -> &'static str {
    let raw = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    let value = u32::from_str_radix(raw, 16).ok().filter(|_| raw.len() == 6);
    let Some(v) = value else {
        return match name {
            "RED" => "🟥",
            "BLU" => "🟦",
            "GRN" => "🟩",
            _ => ["🟥", "🟦", "🟩", "🟨", "🟪"][index % 5],
        };
    };
    let r = ((v >> 16) & 255) as f64 / 255.;
    let g = ((v >> 8) & 255) as f64 / 255.;
    let b = (v & 255) as f64 / 255.;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = (max + min) / 2.;
    if d < 0.12 {
        return "⬜";
    }
    let hue = if max == r {
        (g - b) / d % 6.
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    let hue = (hue * 60.).rem_euclid(360.);
    if !(15. ..340.).contains(&hue) {
        "🟥"
    } else if hue < 45. {
        if l < 0.35 { "🟫" } else { "🟧" }
    } else if hue < 70. {
        "🟨"
    } else if hue < 170. {
        "🟩"
    } else if hue < 260. {
        "🟦"
    } else {
        "🟪"
    }
}
fn relative(v: &Value) -> String {
    crate::integrity_enforcement::date(v)
        .map(|d| format!("<t:{}:R>", d.timestamp()))
        .unwrap_or_else(|| "—".into())
}
pub fn fit_lines(lines: &[String], max: usize) -> String {
    if lines.is_empty() {
        return "—".into();
    }
    for count in (1..=lines.len()).rev() {
        let mut shown = lines[..count].to_vec();
        if count < lines.len() {
            shown.push(format!("另有 {} 人", lines.len() - count))
        }
        let s = shown.join("\n");
        if s.encode_utf16().count() <= max {
            return s;
        }
    }
    h::clip(&format!("另有 {} 人", lines.len()), max)
}
pub fn embed_length(e: &Value) -> usize {
    ["title", "description"]
        .iter()
        .map(|k| h::text(&e[*k]).encode_utf16().count())
        .sum::<usize>()
        + h::text(&e["author"]["name"]).encode_utf16().count()
        + h::text(&e["footer"]["text"]).encode_utf16().count()
        + e["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                h::text(&f["name"]).encode_utf16().count()
                    + h::text(&f["value"]).encode_utf16().count()
            })
            .sum::<usize>()
}
pub fn fit(mut e: Value) -> Value {
    while embed_length(&e) > 6000 {
        let Some(fields) = e["fields"].as_array_mut() else {
            break;
        };
        let Some((index, length)) = fields
            .iter()
            .enumerate()
            .map(|(i, f)| (i, h::text(&f["value"]).encode_utf16().count()))
            .max_by_key(|r| r.1)
        else {
            break;
        };
        if length < 40 {
            break;
        }
        let lines = h::text(&fields[index]["value"])
            .lines()
            .filter(|s| !s.starts_with("另有 "))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        fields[index]["value"] = json!(fit_lines(&lines, length * 7 / 10));
    }
    e
}
fn sorted(players: &Value) -> Vec<Value> {
    let mut players = players.as_array().cloned().unwrap_or_default();
    players.sort_by(|a, b| {
        n(&b["kills"])
            .total_cmp(&n(&a["kills"]))
            .then(n(&a["deaths"]).total_cmp(&n(&b["deaths"])))
    });
    players
}
fn field(name: String, value: String, inline: bool) -> Value {
    json!({"name":h::clip(&name,256),"value":h::clip(&value,1024),"inline":inline})
}
fn player(p: &Value, bold: bool) -> String {
    let name = escape(h::text(&p["name"]));
    format!(
        "{} {}/{}",
        if bold { format!("**{name}**") } else { name },
        n(&p["kills"]),
        n(&p["deaths"])
    )
}
fn art(origin: &str, map: &str, lighting: &str, variant: &str) -> Option<String> {
    if !origin.starts_with("https://") || map.is_empty() {
        return None;
    }
    let dirs = vec![map_id(map).to_owned(), display(map_id(map))];
    let lights = vec![lighting, "DayClear"];
    let roots = ["build/client", "static"];
    let available = roots.iter().any(|r| std::path::Path::new(r).exists());
    for light in lights {
        if light.is_empty() {
            continue;
        }
        for dir in &dirs {
            let path = format!(
                "/maps/{}/{}-{variant}.webp",
                h::encode(dir),
                h::encode(light)
            );
            if !available
                || roots.iter().any(|root| {
                    std::path::Path::new(root)
                        .join(format!("maps/{dir}/{light}-{variant}.webp"))
                        .exists()
                })
            {
                return Some(format!("{}{path}", origin.trim_end_matches('/')));
            }
        }
    }
    None
}
pub fn render(state: &AppState, hook: &Value, server: &Value, live: &Value, now: i64) -> Value {
    let origin = state.config.origin.trim_end_matches('/');
    let name = h::text(&server["name"]);
    let s = &live["status"];
    let mut e = json!({"title":h::clip(name,200),"description":"","color":0x8a8a90,"timestamp":chrono::DateTime::from_timestamp_millis(now).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"author":{"name":h::clip(h::text(&server["org_name"]),200)},"footer":{"text":state.config.identity.app_name}});
    if origin.starts_with("https://") {
        e["author"]["icon_url"] = json!(format!("{origin}/icon-192.png"))
    }
    let mut fields = Vec::new();
    if !h::text(&live["gameServerId"]).is_empty() {
        fields.push(field(
            "入服代码".into(),
            format!("```\n{}\n```", h::text(&live["gameServerId"])),
            false,
        ))
    }
    if live["observedAt"].is_null() {
        e["description"] = json!("⚪ 等待首次采集。");
        return e;
    }
    e["timestamp"] = live["observedAt"].clone();
    if live["ok"] != true || !s.is_object() {
        e["description"] = json!(format!(
            "🔴 **无法连接**\n{}\n最后在线 {}\n检查时间 {}",
            h::clip(h::text(&live["error"]), 200),
            relative(&live["statusAt"]),
            relative(&live["observedAt"])
        ));
        e["color"] = json!(0xd86060)
    } else {
        let players = sorted(&live["players"]);
        let mut scores: Vec<_> = s["scores"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, v)| (i, v))
            .collect();
        scores.sort_by(|a, b| n(&b.1["score"]).total_cmp(&n(&a.1["score"])));
        let cap = n(&s["scoreCap"]);
        let cap = if cap > 0. { cap } else { 100. };
        let busy = n(&s["playerCount"]) > 0.;
        let filled = if n(&s["maxPlayers"]) > 0. {
            (n(&s["playerCount"]) / n(&s["maxPlayers"]) * 16.)
                .round()
                .clamp(0., 16.) as usize
        } else {
            0
        };
        let online = format!(
            "{} **{} / {}**{} 在线  {}{}",
            if busy { "🟢" } else { "⚪" },
            n(&s["playerCount"]),
            n(&s["maxPlayers"]),
            if n(&live["reservedSlots"]) > 0. {
                format!(" +{} 预留", n(&live["reservedSlots"]))
            } else {
                String::new()
            },
            "▰".repeat(filled),
            "▱".repeat(16 - filled)
        );
        let where_ = format!(
            "**{}** · {} · {}",
            display(h::text(&s["map"])),
            pretty(h::text(&s["lighting"])),
            s["experiences"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| pretty(h::text(v)))
                .collect::<Vec<_>>()
                .join(" + ")
        );
        let rows: Vec<_> = scores
            .iter()
            .map(|(i, f)| {
                let count = (n(&f["score"]) / cap * 10.).round().clamp(0., 10.) as usize;
                format!(
                    "{}{} **{}** {}",
                    square(h::text(&f["colorHex"]), h::text(&f["name"]), *i).repeat(count),
                    "⬛".repeat(10 - count),
                    n(&f["score"]),
                    escape(h::text(&f["name"]))
                )
            })
            .collect();
        let match_ = if !scores.is_empty() {
            format!(
                "目标 {cap} 分{}",
                if let Some(secs) = s["matchSeconds"].as_f64() {
                    format!(
                        " · 已进行 {} 分 {} 秒",
                        (secs / 60.).floor(),
                        secs as i64 % 60
                    )
                } else {
                    String::new()
                }
            )
        } else {
            String::new()
        };
        let style = h::text(&hook["status_style"]);
        let mut desc = vec![online, where_];
        if style == "compact" {
            desc.push(
                scores
                    .iter()
                    .map(|(i, f)| {
                        format!(
                            "{} {} **{}**",
                            square(h::text(&f["colorHex"]), h::text(&f["name"]), *i),
                            escape(h::text(&f["name"])),
                            n(&f["score"])
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
            for (i, f) in &scores {
                fields.insert(
                    0,
                    field(
                        format!(
                            "{} {}",
                            square(h::text(&f["colorHex"]), h::text(&f["name"]), *i),
                            h::text(&f["name"])
                        ),
                        format!(
                            "{} 人",
                            players.iter().filter(|p| p["faction"] == f["name"]).count()
                        ),
                        true,
                    ),
                );
            }
            if !players.is_empty() {
                fields.push(field(
                    "击杀榜".into(),
                    players
                        .iter()
                        .take(3)
                        .map(|p| player(p, true))
                        .collect::<Vec<_>>()
                        .join(" · "),
                    false,
                ))
            }
        } else if style == "scoreboard" {
            desc.extend(rows);
            if !players.is_empty() {
                let table = players
                    .iter()
                    .take(20)
                    .map(|p| {
                        format!(
                            "{:>3} {:>3} {:<17} {}",
                            n(&p["kills"]),
                            n(&p["deaths"]),
                            h::clip(h::text(&p["name"]).replace('`', "'").as_str(), 17),
                            h::clip(h::text(&p["faction"]), 10)
                        )
                    })
                    .collect::<Vec<_>>();
                fields.push(field(
                    "计分板（前 20 名）".into(),
                    format!(
                        "```\n击杀 死亡 玩家 阵营\n{}\n```",
                        h::clip(&table.join("\n"), 940)
                    ),
                    false,
                ))
            }
        } else {
            desc.extend(rows);
            for (i, f) in &scores {
                let mine = players
                    .iter()
                    .filter(|p| p["faction"] == f["name"])
                    .collect::<Vec<_>>();
                fields.insert(
                    0,
                    field(
                        format!(
                            "{} {} · {}",
                            square(h::text(&f["colorHex"]), h::text(&f["name"]), *i),
                            h::text(&f["name"]),
                            mine.len()
                        ),
                        fit_lines(
                            &mine
                                .iter()
                                .enumerate()
                                .map(|(i, p)| player(p, i < 2))
                                .collect::<Vec<_>>(),
                            1024,
                        ),
                        true,
                    ),
                );
            }
            let loose = players
                .iter()
                .filter(|p| !scores.iter().any(|(_, f)| f["name"] == p["faction"]))
                .collect::<Vec<_>>();
            if !loose.is_empty() {
                fields.push(field(
                    format!("未分配 · {}", loose.len()),
                    fit_lines(
                        &loose
                            .iter()
                            .enumerate()
                            .map(|(i, p)| player(p, i < 2))
                            .collect::<Vec<_>>(),
                        1024,
                    ),
                    false,
                ))
            }
        }
        if !match_.is_empty() {
            desc.push(match_)
        }
        e["description"] = json!(h::clip(&desc.join("\n"), 3900));
        e["color"] = json!(if busy {
            scores
                .first()
                .and_then(|(_, f)| {
                    u32::from_str_radix(h::text(&f["colorHex"]).trim_start_matches('#'), 16).ok()
                })
                .unwrap_or(0x7bc462)
        } else {
            0x8a8a90
        });
        let restart = crate::integrity_enforcement::date(&live["startedAt"]);
        let restart = restart
            .map(|at| {
                format!(
                    "启动于 {}{}\n",
                    relative(&live["startedAt"]),
                    if now - at.timestamp_millis() >= 86400000 {
                        " · 本局结束后重启"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_default();
        fields.push(field(
            "\u{200b}".into(),
            format!("{restart}更新于 {}", relative(&live["observedAt"])),
            false,
        ));
        if let Some(url) = art(
            origin,
            h::text(&s["map"]),
            h::text(&s["lighting"]),
            if style == "banner" { "wide" } else { "square" },
        ) {
            e[if style == "banner" {
                "image"
            } else {
                "thumbnail"
            }] = json!({"url":url})
        }
    }
    e["fields"] = json!(fields.into_iter().take(25).collect::<Vec<_>>());
    let id = h::encode(h::text(&server["id"]));
    let mut links = Vec::new();
    if hook["link_status"] == true
        && server["public_status"] == true
        && server["allow_public_status"] == true
    {
        links.push(("实时状态", format!("{origin}/s/{id}")))
    }
    if hook["link_leaderboard"] == true
        && server["public_leaderboards"] == true
        && server["allow_public_leaderboards"] == true
    {
        links.push(("排行榜", format!("{origin}/s/{id}/leaderboard")))
    }
    if hook["link_panel"] == true {
        links.push(("管理面板", format!("{origin}/server/{id}")))
    }
    if let Some((_, url)) = links.first() {
        e["url"] = json!(url)
    }
    if links.len() > 1 {
        e["description"] = json!(format!(
            "{}\n{}",
            h::text(&e["description"]),
            links
                .iter()
                .skip(1)
                .map(|(label, url)| format!("[{label}]({url})"))
                .collect::<Vec<_>>()
                .join(" · ")
        ))
    }
    fit(e)
}
pub async fn card(state: &AppState, hook: &Value, server: &str, name: &str) -> Result<Value> {
    let info:Value=sqlx::query_scalar("SELECT to_jsonb(s)||jsonb_build_object('org_name',o.name,'allow_public_status',o.allow_public_status,'allow_public_leaderboards',o.allow_public_leaderboards) FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1").bind(server).fetch_optional(&state.db).await?.unwrap_or(json!({"id":server,"name":name}));
    let live = crate::live::read(state, &[server.to_owned()])
        .await?
        .remove(server)
        .unwrap_or(Value::Null);
    Ok(render(
        state,
        hook,
        &info,
        &live,
        Utc::now().timestamp_millis(),
    ))
}
pub async fn run(state: AppState) -> Result<()> {
    let mut sent: HashMap<String, (String, String, i64)> = HashMap::new();
    let mut retry = HashMap::<String, i64>::new();
    let mut tick = tokio::time::interval(Duration::from_secs(20));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=state.runtime.stop.cancelled()=>break,_=tick.tick()=>{
            state.runtime.check().await?;let now=Utc::now().timestamp_millis();let hooks:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(w) FROM webhooks w JOIN organizations o ON o.id=w.org_id WHERE w.enabled AND w.status_enabled AND o.suspended_at IS NULL").fetch_all(&state.db).await?;
            sent.retain(|key,_|hooks.iter().any(|h|key.starts_with(&format!("{}:",h::text(&h["id"])))));retry.retain(|id,_|hooks.iter().any(|h|h["id"]==*id));
            for hook in hooks{let hook_id=h::text(&hook["id"]);if retry.get(hook_id).is_some_and(|at|*at>now){continue}
                let servers:Vec<(String,String)>=sqlx::query_as("SELECT id,name FROM servers WHERE org_id=$1 ORDER BY sort_order,name").bind(hook["org_id"].as_str()).fetch_all(&state.db).await?;let servers=servers.into_iter().filter(|(id,_)|h::scope(&hook["server_ids"],Some(id))).collect::<Vec<_>>();let gap=n(&hook["status_interval_s"])*1000.;let gap=gap.max(servers.len() as f64*4000.) as i64;
                let mut map=hook["status_messages"].as_object().cloned().unwrap_or_default();let stale:Vec<_>=map.iter().filter(|(id,_)|!servers.iter().any(|s|s.0==***id)).map(|(k,v)|(k.clone(),v.clone())).collect();
                if !stale.is_empty(){let mut tx=state.worker_transaction().await?;let mut old=hook.clone();old["status_messages"]=json!(stale.iter().cloned().collect::<serde_json::Map<_,_>>());h::cleanup(&mut tx,&old,0).await?;for (id,_)in &stale{map.remove(id);sent.remove(&format!("{hook_id}:{id}"));}sqlx::query("UPDATE webhooks SET status_messages=$2 WHERE id=$1 AND updated_at=$3 AND enabled AND status_enabled").bind(hook_id).bind(json!(map)).bind(crate::integrity_enforcement::date(&hook["updated_at"])).execute(&mut *tx).await?;tx.commit().await?;}
                for (server,name)in &servers{let key=format!("{hook_id}:{server}");let embed=card(&state,&hook,server,name).await?;let mut substance=embed.clone();substance.as_object_mut().unwrap().remove("timestamp");if let Some(fields)=substance["fields"].as_array_mut(){fields.retain(|f|f["name"]!="\u{200b}")}let substance=substance.to_string();let previous=sent.get(&key);
                    if previous.is_some_and(|(_,old,at)|now-*at<gap || (old==&substance && now-*at<300000)){continue}
                    state.runtime.check().await?;let current:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM webhooks WHERE id=$1 AND updated_at=$2 AND enabled AND status_enabled)").bind(hook_id).bind(crate::integrity_enforcement::date(&hook["updated_at"])).fetch_one(&state.db).await?;if !current{break}
                    let mut message=map.get(server).and_then(Value::as_str).map(str::to_owned);let payload=h::payload(&state,embed);let mut result=if let Some(id)=&message{h::discord_call(&state,h::text(&hook["url_enc"]),"PATCH",&format!("/messages/{id}"),Some(&payload)).await}else{h::PostResult::default()};
                    if result.unknown_message{message=None}let posted=message.is_none();if posted{result=h::discord_call(&state,h::text(&hook["url_enc"]),"POST","?wait=true",Some(&payload)).await;if result.ok{message=result.message_id.clone()}}
                    let mut tx=state.worker_transaction().await?;if result.ok{if let Some(id)=&message{map.insert(server.clone(),json!(id));}}
                    let changed=sqlx::query("UPDATE webhooks SET status_messages=$2,status_sent_at=CASE WHEN $4 THEN now() ELSE status_sent_at END,last_sent_at=CASE WHEN $4 THEN now() ELSE last_sent_at END,last_status=$5,last_error=$6 WHERE id=$1 AND updated_at=$3 AND enabled AND status_enabled").bind(hook_id).bind(json!(map)).bind(crate::integrity_enforcement::date(&hook["updated_at"])).bind(result.ok).bind(result.status as i32).bind(&result.error).execute(&mut *tx).await?;
                    if posted && changed.rows_affected()==0 && message.is_some(){let old=json!({"id":hook_id,"url_enc":hook["url_enc"],"status_messages":{"orphan":message}});h::cleanup(&mut tx,&old,0).await?}tx.commit().await?;
                    if changed.rows_affected()==0{sent.remove(&key);break}if result.ok{sent.insert(key,(message.unwrap_or_default(),substance,now));}else{retry.insert(hook_id.into(),now+result.retry_after_ms.unwrap_or(60000) as i64);break}
                }
            }
        }}
    }
    Ok(())
}
