//! Discord credentials, validation and transactional notification input.
use crate::{
    config::AppState,
    crypto,
    error::{ApiError, Result},
    integrity_statistics::camel_row,
};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
pub const EVENTS: &[&str] = &[
    "bans",
    "commands",
    "triggers",
    "players",
    "management",
    "auth",
    "teamkills",
    "watched",
    "integrity",
];
pub fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn clip(s: &str, n: usize) -> String {
    crate::feed::truncate(s, n)
}
pub fn stamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
pub fn encode(s: &str) -> String {
    const SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~')
        .remove(b'!')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    percent_encoding::utf8_percent_encode(s, SET).to_string()
}
pub fn validate_url(raw: &str) -> Result<(String, String)> {
    let u = url::Url::parse(raw.trim())
        .map_err(|_| ApiError::bad("请输入完整 Discord Webhook 地址。"))?;
    let host = u.host_str().unwrap_or("");
    let parts: Vec<_> = u.path().split('/').collect();
    if u.scheme() != "https"
        || ![
            "discord.com",
            "discordapp.com",
            "ptb.discord.com",
            "canary.discord.com",
        ]
        .contains(&host)
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
        || parts.len() != 5
        || parts[1] != "api"
        || parts[2] != "webhooks"
        || !(15..=25).contains(&parts[3].len())
        || !parts[3].bytes().all(|c| c.is_ascii_digit())
        || !(30..=200).contains(&parts[4].len())
        || !parts[4]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(ApiError::bad(
            "请输入 https://discord.com/api/webhooks/<id>/<token> 格式的地址。",
        ));
    }
    Ok((
        format!("https://{host}/api/webhooks/{}/{}", parts[3], parts[4]),
        format!("{host}/api/webhooks/{}/…", parts[3]),
    ))
}
pub fn classify(row: &Value) -> Option<&'static str> {
    let action = text(&row["action"]);
    if action == "integrity.report.create" {
        return None;
    }
    if action == "trigger.integrity" || action.starts_with("integrity.enforcement.") {
        return Some("integrity");
    }
    Some(match text(&row["category"]) {
        "rcon" => {
            if ["rcon.ban", "rcon.unban"].contains(&action) {
                "bans"
            } else {
                "commands"
            }
        }
        "trigger" => "triggers",
        "player" => "players",
        "org" | "system" => {
            if action.starts_with("list") {
                "bans"
            } else {
                "management"
            }
        }
        "server" | "user" => "management",
        "auth" => "auth",
        _ => return None,
    })
}
pub fn scope(scope: &Value, server: Option<&str>) -> bool {
    scope.is_null()
        || server.is_some_and(|id| scope.as_array().is_some_and(|a| a.iter().any(|v| v == id)))
}
pub fn view(row: Value) -> Value {
    let mut r = camel_row(row);
    r.as_object_mut().unwrap().retain(|k, _| {
        [
            "id",
            "label",
            "urlHint",
            "events",
            "serverIds",
            "enabled",
            "statusEnabled",
            "statusStyle",
            "statusIntervalS",
            "linkStatus",
            "linkLeaderboard",
            "linkPanel",
            "statusSentAt",
            "lastSentAt",
            "lastStatus",
            "lastError",
            "createdAt",
        ]
        .contains(&k.as_str())
    });
    r
}
pub async fn input(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    org: &str,
    server: &str,
    kind: &str,
    payload: &Value,
) -> Result<()> {
    sqlx::query("INSERT INTO webhook_events(id,org_id,server_id,kind,payload) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING").bind(id).bind(org).bind(server).bind(kind).bind(payload).execute(&mut **tx).await?;
    Ok(())
}
pub fn case_embed(app: &str, r: &Value) -> Value {
    let mut lines = vec![
        format!("案件：{}", text(&r["caseId"])),
        format!("玩家：{}", text(&r["steamId"])),
        format!("服务器：{}", text(&r["serverName"])),
        format!("地图：{}", text(&r["map"])),
        format!("风险分：{}/100", r["score"]),
        format!(
            "180 秒步兵：{} 次击杀，{:.2} KPM，{} 位独立受害者",
            r["infantryKills"],
            r["kpm180"].as_f64().unwrap_or(0.),
            r["uniqueVictims"]
        ),
        format!("独立举报人：{}", r["uniqueReporters"]),
    ];
    for b in r["breakdown"].as_array().into_iter().flatten() {
        lines.push(format!(
            "+{} {}：{}",
            b["points"],
            text(&b["code"]),
            text(&b["detail"])
        ));
    }
    lines.push("案件已保存。实际处罚请查看面板执行记录。".into());
    json!({"title":format!("社区风控审核 · {}",r["statisticalLevel"].as_str().unwrap_or(text(&r["level"]))),"description":clip(&lines.join("\n"),2000),"color":0xe8a441,"timestamp":r["createdAt"],"footer":{"text":app}})
}
pub fn audit_embed(app: &str, row: &Value) -> Value {
    let r = camel_row(row.clone());
    let bare = ["server.create", "server.update", "server.delete"].contains(&text(&r["action"]));
    let mut lines = vec![format!(
        "**{}**{}",
        clip(r["actorName"].as_str().unwrap_or("系统"), 60),
        if !bare && !text(&r["target"]).is_empty() {
            format!(" → `{}`", clip(text(&r["target"]), 120))
        } else {
            String::new()
        }
    )];
    if !text(&r["serverName"]).is_empty() {
        lines.push(format!("服务器：{}", clip(text(&r["serverName"]), 80)))
    }
    if r["outcome"] != "ok" {
        lines.push(format!("结果：**{}**", text(&r["outcome"])))
    }
    if !bare && !text(&r["message"]).is_empty() {
        lines.push(clip(text(&r["message"]), 600))
    }
    json!({"title":clip(text(&r["action"]),200),"description":clip(&lines.join("\n"),2000),"color":if r["outcome"]=="ok"{0x7bc462}else if r["outcome"]=="error"{0xd86060}else{0x8a8a90},"timestamp":r["ts"],"footer":{"text":app}})
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostResult {
    pub ok: bool,
    pub status: u16,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unknown_message: bool,
}
pub async fn discord_call(
    state: &AppState,
    enc: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> PostResult {
    let failed = |message: &str| PostResult {
        error: message.into(),
        ..Default::default()
    };
    let Ok(raw) = crypto::decrypt_secret(&state.config.encryption_key, enc) else {
        return failed("无法解密 Webhook 地址。");
    };
    let Ok((url, _)) = validate_url(&raw) else {
        return failed("Webhook 地址无效。");
    };
    if !["POST", "PATCH", "DELETE"].contains(&method)
        || !(path == "?wait=true"
            || (path.starts_with("/messages/")
                && path[10..].bytes().all(|c| c.is_ascii_digit())
                && (1..=25).contains(&path[10..].len())))
    {
        return failed("Webhook 请求格式无效。");
    }
    let request = async {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let mut req = client.request(
            reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
            format!("{url}{path}"),
        );
        if let Some(body) = body {
            req = req.json(body)
        }
        let res = req.send().await?;
        let status = res.status().as_u16();
        let retry = res
            .headers()
            .get("retry-after")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<f64>().ok());
        if res.content_length().is_some_and(|n| n > 65536) {
            return Ok::<_, reqwest::Error>(failed("Discord 响应过大。"));
        }
        let mut data = Vec::new();
        let mut res = res;
        while let Some(chunk) = res.chunk().await? {
            if data.len() + chunk.len() > 65536 {
                return Ok(failed("Discord 响应过大。"));
            }
            data.extend_from_slice(&chunk)
        }
        let data: Value = serde_json::from_slice(&data).unwrap_or(Value::Null);
        Ok(PostResult {
            ok: (200..300).contains(&status),
            status,
            error: if (200..300).contains(&status) {
                String::new()
            } else {
                format!("Discord 返回 HTTP {status}。")
            },
            retry_after_ms: if status == 429 {
                Some(
                    ((retry.or_else(|| data["retry_after"].as_f64()).unwrap_or(5.) * 1000.)
                        .ceil()
                        .clamp(1000., 60000.)) as u64,
                )
            } else {
                None
            },
            message_id: data["id"]
                .as_str()
                .filter(|s| s.bytes().all(|c| c.is_ascii_digit()))
                .map(str::to_owned),
            unknown_message: data["code"] == 10008,
        })
    }
    .await;
    request.unwrap_or_else(|_| failed("无法连接 Discord 或请求超时。"))
}
pub fn payload(state: &AppState, embed: Value) -> Value {
    json!({"username":state.config.identity.app_name,"allowed_mentions":{"parse":[]},"embeds":[embed]})
}
pub async fn record(tx: &mut Transaction<'_, Postgres>, id: &str, r: &PostResult) -> Result<()> {
    sqlx::query("UPDATE webhooks SET last_status=$2,last_error=$3,last_sent_at=CASE WHEN $4 THEN now() ELSE last_sent_at END WHERE id=$1").bind(id).bind(r.status as i32).bind(clip(&r.error,300)).bind(r.ok).execute(&mut **tx).await?;
    Ok(())
}
pub async fn cleanup(tx: &mut Transaction<'_, Postgres>, row: &Value, delay: i64) -> Result<()> {
    for (_, id) in row["status_messages"].as_object().into_iter().flatten() {
        if let Some(id) = id.as_str() {
            sqlx::query("INSERT INTO webhook_queue(hook_id,source_id,url_enc,method,path,not_before) VALUES($1,$2,$3,'DELETE',$4,now()+($5::bigint*interval '1 millisecond')) ON CONFLICT DO NOTHING").bind(row["id"].as_str()).bind(format!("delete:{id}")).bind(row["url_enc"].as_str()).bind(format!("/messages/{id}")).bind(delay).execute(&mut **tx).await?;
        }
    }
    Ok(())
}
