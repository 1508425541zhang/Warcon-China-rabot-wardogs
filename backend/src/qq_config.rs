//! QQ connection settings are read from the database on every operation, never stale process globals.
use crate::{
    config::AppState,
    crypto,
    error::{ApiError, Result},
    webhooks::{clip, text},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;
fn yes() -> bool {
    true
}
fn twenty() -> i64 {
    20
}
fn one() -> i64 {
    1
}
fn ten() -> i64 {
    10
}
fn reserve_cost() -> i64 {
    120
}
fn hours() -> i64 {
    24
}
fn seconds() -> i64 {
    120
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub anti_cheat_notices: bool,
    pub server_id: String,
    pub groups: Vec<String>,
    #[serde(default = "twenty")]
    pub low_at: i64,
    #[serde(default = "one")]
    pub points_per_minute: i64,
    #[serde(default = "ten")]
    pub vote_cost: i64,
    #[serde(default = "twenty")]
    pub broadcast_cost: i64,
    #[serde(default = "reserve_cost")]
    pub reserve_cost: i64,
    #[serde(default = "hours")]
    pub reserve_hours: i64,
    #[serde(default = "seconds")]
    pub vote_seconds: i64,
    pub maps: Vec<String>,
}
pub fn number_id(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && !s.starts_with('0') && s.bytes().all(|c| c.is_ascii_digit())
}
pub fn open_id(s: &str) -> bool {
    (16..=128).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
pub fn identity(s: &str) -> bool {
    if let Some(s) = s.strip_prefix("ob11:") {
        return number_id(s, 1, 16);
    }
    let p: Vec<_> = s.split(':').collect();
    p.len() == 3 && p[0] == "official" && number_id(p[1], 5, 16) && open_id(p[2])
}
pub fn policies(value: &Value) -> Result<Vec<Policy>> {
    let list: Vec<Policy> =
        serde_json::from_value(value.clone()).map_err(|_| ApiError::bad("QQ 规则格式无效。"))?;
    if list.len() > 100 {
        return Err(ApiError::bad("QQ 规则最多 100 项。"));
    }
    let mut servers = HashSet::new();
    let mut groups = HashSet::new();
    for p in &list {
        if p.server_id.is_empty()
            || p.server_id.len() > 100
            || !servers.insert(&p.server_id)
            || p.groups.is_empty()
            || p.groups
                .iter()
                .any(|g| (!number_id(g, 5, 16) && !open_id(g)) || !groups.insert(g))
            || !(1..=200).contains(&p.low_at)
            || !(1..=100).contains(&p.points_per_minute)
            || [p.vote_cost, p.broadcast_cost, p.reserve_cost]
                .iter()
                .any(|n| !(1..=100000).contains(n))
            || !(1..=720).contains(&p.reserve_hours)
            || !(30..=240).contains(&p.vote_seconds)
            || !(2..=10).contains(&p.maps.len())
            || p.maps.iter().any(|m| m.is_empty() || m.len() > 100)
            || p.maps.iter().collect::<HashSet<_>>().len() != p.maps.len()
        {
            return Err(ApiError::bad(
                "服务器、群号和地图不可重复，各项数值须在允许范围内。",
            ));
        }
    }
    Ok(list)
}
pub fn connection(raw: &str, self_id: &str, provider: &str) -> Result<String> {
    if !number_id(self_id, 5, 16) {
        return Err(ApiError::bad("请输入有效机器人 QQ 号或 AppID。"));
    }
    if provider == "official" {
        return if raw == "https://api.bot.qq.com" {
            Ok(raw.into())
        } else {
            Err(ApiError::bad(
                "官方机器人接口地址必须为 https://api.bot.qq.com。",
            ))
        };
    }
    let u = url::Url::parse(raw).map_err(|_| ApiError::bad("请输入完整 QQ 接口地址。"))?;
    if !["http", "https"].contains(&u.scheme())
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || (u.scheme() == "http"
            && !["127.0.0.1", "localhost", "[::1]"].contains(&u.host_str().unwrap_or("")))
    {
        return Err(ApiError::bad(
            "远程连接必须使用 HTTPS；本机可用回环 HTTP；地址不能带凭证、查询或片段。",
        ));
    }
    Ok(u.to_string().trim_end_matches('/').into())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    pub revision: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    pub enabled: bool,
    pub url: String,
    pub self_id: String,
    pub policies: Vec<Policy>,
    #[serde(default)]
    pub token_enc: String,
    #[serde(default)]
    pub secret_enc: String,
}
fn default_provider() -> String {
    "napcat".into()
}
#[derive(Clone)]
pub struct Configuration {
    pub stored: Stored,
    pub token: String,
    pub secret: String,
    pub database: bool,
    pub updated_at: Value,
}
impl Configuration {
    pub fn policy(&self, server: &str) -> Option<&Policy> {
        if !self.stored.enabled {
            return None;
        }
        self.stored
            .policies
            .iter()
            .find(|p| p.enabled && p.server_id == server)
    }
    pub fn active(&self) -> bool {
        self.stored.enabled
            && !self.secret.is_empty()
            && (self.stored.provider == "official" || !self.token.is_empty())
    }
}
pub async fn load_conn(state: &AppState, db: &mut sqlx::PgConnection) -> Result<Configuration> {
    let row: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM site_settings s WHERE key='qqCommunity'")
            .fetch_optional(db)
            .await?;
    if let Some(row) = row {
        let stored: Stored = serde_json::from_value(row["value"].clone())
            .map_err(|_| ApiError::bad("已保存 QQ 配置无效。"))?;
        policies(&json!(stored.policies))?;
        let decode = |enc: &str| {
            if enc.is_empty() {
                Ok(String::new())
            } else {
                crypto::decrypt_secret(&state.config.encryption_key, enc)
                    .map_err(|_| ApiError::bad("无法解密 QQ 密钥。"))
            }
        };
        let token = decode(&stored.token_enc)?;
        let secret = decode(&stored.secret_enc)?;
        Ok(Configuration {
            stored,
            token,
            secret,
            database: true,
            updated_at: row["updated_at"].clone(),
        })
    } else {
        let env = |key: &str| std::env::var(key).unwrap_or_default();
        let provider = std::env::var("QQ_BOT_PROVIDER").unwrap_or_else(|_| "napcat".into());
        let raw = std::env::var("QQ_BOT_POLICIES").unwrap_or_else(|_| "[]".into());
        let list = policies(
            &serde_json::from_str(&raw).map_err(|_| ApiError::bad("环境 QQ 规则格式无效。"))?,
        )?;
        let url = env("ONEBOT_HTTP_URL");
        let token = env("ONEBOT_ACCESS_TOKEN");
        let secret = env("ONEBOT_EVENT_SECRET");
        let self_id = env("ONEBOT_SELF_ID");
        if !["official", "napcat", "llbot"].contains(&provider.as_str()) {
            return Err(ApiError::bad("QQ 连接类型无效。"));
        }
        if !url.is_empty() {
            connection(&url, &self_id, &provider)?;
        }
        Ok(Configuration {
            stored: Stored {
                revision: "environment".into(),
                provider,
                enabled: !url.is_empty(),
                url,
                self_id,
                policies: list,
                token_enc: String::new(),
                secret_enc: String::new(),
            },
            token,
            secret,
            database: false,
            updated_at: Value::Null,
        })
    }
}
pub async fn load(state: &AppState) -> Result<Configuration> {
    {
        let mut connection = state.db.acquire().await?;
        load_conn(state, &mut connection).await
    }
}
pub fn view(c: &Configuration) -> Value {
    json!({"revision":c.stored.revision,"source":if c.database{"database"}else{"environment"},"provider":c.stored.provider,"enabled":c.stored.enabled,"url":c.stored.url,"selfId":c.stored.self_id,"hasToken":!c.token.is_empty(),"hasSecret":!c.secret.is_empty(),"policies":c.stored.policies,"updatedAt":c.updated_at})
}
pub async fn save(
    state: &AppState,
    actor: &crate::auth::Actor,
    headers: &axum::http::HeaderMap,
    input: &Value,
) -> Result<Value> {
    let revision = text(&input["revision"]);
    let enabled = input["enabled"]
        .as_bool()
        .ok_or_else(|| ApiError::bad("启用设置必须为布尔值。"))?;
    let list = policies(&input["policies"])?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('qq-settings',0))")
        .execute(&mut *tx)
        .await?;
    let old = load_conn(state, &mut tx).await?;
    if revision != old.stored.revision {
        return Err(ApiError::new(
            axum::http::StatusCode::CONFLICT,
            "revision_conflict",
            "配置已被其他管理员修改，请刷新。",
        ));
    }
    let provider = input["provider"].as_str().unwrap_or(&old.stored.provider);
    if !["official", "napcat", "llbot"].contains(&provider) {
        return Err(ApiError::bad("请选择有效的 QQ 接口类型。"));
    }
    let raw = text(&input["url"]).trim();
    let self_id = text(&input["selfId"]).trim();
    if raw.len() > 2000 {
        return Err(ApiError::bad("QQ 接口地址过长。"));
    }
    let url = if !raw.is_empty() || !self_id.is_empty() || enabled {
        connection(raw, self_id, provider)?
    } else {
        raw.into()
    };
    let same = provider == old.stored.provider && self_id == old.stored.self_id;
    let secret = |name: &str, clear: &str, previous: &str| -> Result<String> {
        if input[clear] == true {
            return Ok(String::new());
        }
        let s = text(&input[name]);
        if s.is_empty() {
            return Ok(if same { previous.into() } else { String::new() });
        }
        if s.len() < 16 || s.len() > 512 || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(ApiError::bad(
                "密钥为 16–512 字符，不得包含空白或控制字符。",
            ));
        }
        Ok(s.into())
    };
    let token = secret("token", "clearToken", &old.token)?;
    let secret = secret("secret", "clearSecret", &old.secret)?;
    if enabled
        && (secret.is_empty()
            || (provider != "official" && token.is_empty())
            || !list.iter().any(|p| p.enabled))
    {
        return Err(ApiError::bad("启用前请填写密钥并启用至少一项服务器规则。"));
    }
    if provider == "official" && list.iter().any(|p| p.groups.iter().any(|g| !open_id(g))) {
        return Err(ApiError::bad("官方机器人需填写群 group_openid。"));
    }
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM servers FOR SHARE")
        .fetch_all(&mut *tx)
        .await?;
    if list.iter().any(|p| !ids.contains(&p.server_id)) {
        return Err(ApiError::bad("规则中的服务器不存在。"));
    }
    let encode = |s: &str| {
        if s.is_empty() {
            Ok(String::new())
        } else {
            crypto::encrypt_secret(&state.config.encryption_key, s)
                .map_err(|_| ApiError::bad("无法保存 QQ 密钥。"))
        }
    };
    let stored = Stored {
        revision: uuid::Uuid::new_v4().to_string(),
        provider: provider.into(),
        enabled,
        url,
        self_id: self_id.into(),
        policies: list,
        token_enc: encode(&token)?,
        secret_enc: encode(&secret)?,
    };
    sqlx::query("INSERT INTO site_settings(key,value,updated_by,updated_at) VALUES('qqCommunity',$1,$2,now()) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_by=excluded.updated_by,updated_at=now()").bind(json!(stored)).bind(&actor.id).execute(&mut *tx).await?;
    crate::audit::event(&mut tx,actor,None,headers,"system","qq.settings.update","qqCommunity",json!({"revision":stored.revision,"provider":stored.provider,"enabled":stored.enabled,"url":stored.url,"selfId":stored.self_id,"hasToken":!token.is_empty(),"hasSecret":!secret.is_empty(),"policies":stored.policies})).await?;
    tx.commit().await?;
    Ok(view(&load(state).await?))
}
pub async fn vips(db: &mut sqlx::PgConnection) -> Result<Value> {
    Ok(
        sqlx::query_scalar("SELECT value FROM site_settings WHERE key='qqVip'")
            .fetch_optional(db)
            .await?
            .unwrap_or(json!({"revision":"empty","entries":[]})),
    )
}
pub async fn save_vips(
    tx: &mut Transaction<'_, Postgres>,
    actor: &str,
    input: &Value,
) -> Result<Value> {
    let list = input["entries"]
        .as_array()
        .filter(|a| a.len() <= 500)
        .ok_or_else(|| ApiError::bad("VIP 名单最多 500 项。"))?;
    let mut unique = HashSet::new();
    let mut entries = Vec::new();
    for row in list {
        let server = text(&row["serverId"]);
        let steam = text(&row["steamId"]);
        if server.is_empty()
            || server.len() > 100
            || steam.len() != 17
            || !steam.bytes().all(|b| b.is_ascii_digit())
            || !unique.insert(format!("{server}:{steam}"))
            || ["enabled", "reserve", "allowOverkill", "whitelist"]
                .iter()
                .any(|k| !row[*k].is_boolean())
            || !["reserve", "allowOverkill", "whitelist"]
                .iter()
                .any(|k| row[*k] == true)
            || text(&row["note"]).trim().encode_utf16().count() > 200
        {
            return Err(ApiError::bad("VIP 格式无效、重复或未选择权益。"));
        }
        entries.push(json!({"serverId":server,"steamId":steam,"enabled":row["enabled"],"reserve":row["reserve"],"allowOverkill":row["allowOverkill"],"whitelist":row["whitelist"],"note":clip(text(&row["note"]).trim(),200)}));
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('qq-vip-settings',0))")
        .execute(&mut **tx)
        .await?;
    let old = vips(tx).await?;
    if input["revision"] != old["revision"] {
        return Err(ApiError::new(
            axum::http::StatusCode::CONFLICT,
            "revision_conflict",
            "VIP 名单已变化，请刷新。",
        ));
    }
    let servers: Vec<(String, String)> = sqlx::query_as("SELECT id,org_id FROM servers FOR SHARE")
        .fetch_all(&mut **tx)
        .await?;
    for row in &entries {
        let Some((server, org)) = servers.iter().find(|r| r.0 == text(&row["serverId"])) else {
            return Err(ApiError::bad("VIP 服务器不存在。"));
        };
        if row["enabled"] == true && row["reserve"] == true {
            crate::api::lists::ensure(tx, org, Some(server), "reserve").await?;
        }
    }
    let value = json!({"revision":uuid::Uuid::new_v4().to_string(),"entries":entries});
    sqlx::query("INSERT INTO site_settings(key,value,updated_by,updated_at) VALUES('qqVip',$1,$2,now()) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_by=excluded.updated_by,updated_at=now()").bind(&value).bind(actor).execute(&mut **tx).await?;
    Ok(value)
}
