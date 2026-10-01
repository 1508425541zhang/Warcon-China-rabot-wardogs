//! QQ signed events and plain-text commands; no CQ or rich message evaluation.
use crate::qq_config::{number_id, open_id};
use axum::http::HeaderMap;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub group_id: String,
    pub member_id: String,
    pub content: String,
}
pub fn verify_onebot(secret: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    use hmac::{Hmac, Mac};
    let Some(sig) = headers
        .get("x-signature")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("sha1="))
    else {
        return false;
    };
    if sig.len() != 40
        || !sig
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return false;
    }
    let Ok(sig) = hex::decode(sig) else {
        return false;
    };
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}
fn official_key(secret: &str) -> Option<openssl::pkey::PKey<openssl::pkey::Private>> {
    if secret.is_empty() {
        return None;
    }
    let mut seed = secret.as_bytes().to_vec();
    while seed.len() < 32 {
        seed.extend_from_within(..)
    }
    openssl::pkey::PKey::private_key_from_raw_bytes(&seed[..32], openssl::pkey::Id::ED25519).ok()
}
pub fn validation(secret: &str, token: &str, timestamp: &str) -> Option<Value> {
    let key = official_key(secret)?;
    let mut signer = openssl::sign::Signer::new_without_digest(&key).ok()?;
    let sig = signer
        .sign_oneshot_to_vec(format!("{timestamp}{token}").as_bytes())
        .ok()?;
    Some(json!({"plain_token":token,"signature":hex::encode(sig)}))
}
pub fn verify_official(secret: &str, headers: &HeaderMap, body: &[u8], now: i64) -> bool {
    let Some(timestamp) = headers
        .get("x-signature-timestamp")
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(sig) = headers
        .get("x-signature-ed25519")
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    if timestamp.len() != 10
        || !timestamp.bytes().all(|c| c.is_ascii_digit())
        || timestamp.parse::<i64>().is_err()
        || (now / 1000 - timestamp.parse::<i64>().unwrap()).abs() > 300
        || sig.len() != 128
    {
        return false;
    }
    let Some(key) = official_key(secret) else {
        return false;
    };
    let Ok(sig) = hex::decode(sig) else {
        return false;
    };
    let Ok(mut verify) = openssl::sign::Verifier::new_without_digest(&key) else {
        return false;
    };
    let mut data = timestamp.as_bytes().to_vec();
    data.extend_from_slice(body);
    verify.verify_oneshot(&sig, &data).unwrap_or(false)
}
fn id(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        if number_id(s, 1, 16) {
            return Some(s.into());
        }
    }
    v.as_u64()
        .filter(|n| *n > 0 && *n <= 9007199254740991)
        .map(|n| n.to_string())
}
fn message_id(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        let numeric = s.strip_prefix('-').unwrap_or(s);
        return ((!numeric.is_empty())
            && numeric.len() <= 20
            && numeric.bytes().all(|c| c.is_ascii_digit()))
        .then(|| s.into());
    }
    v.as_i64()
        .filter(|n| n.unsigned_abs() <= 9007199254740991)
        .map(|n| n.to_string())
}
pub fn onebot(v: &Value, self_id: &str, now: i64) -> Option<Message> {
    if v["post_type"] != "message"
        || v["message_type"] != "group"
        || v["sub_type"] != "normal"
        || id(&v["self_id"]).as_deref() != Some(self_id)
        || crate::http::truthy(&v["anonymous"])
    {
        return None;
    }
    let user = id(&v["user_id"])?;
    let group = id(&v["group_id"])?;
    let time = v["time"].as_i64()?;
    if user == self_id || ((now / 1000) as i128 - time as i128).abs() > 300 {
        return None;
    }
    let mid = message_id(&v["message_id"])?;
    let mut content = if let Some(raw) = v["message"].as_str() {
        if raw.encode_utf16().count() > 4000 {
            return None;
        }
        let prefix = format!("[CQ:at,qq={self_id}]");
        let raw = raw
            .trim_start()
            .strip_prefix(&prefix)
            .unwrap_or(raw)
            .trim_start();
        if raw.contains("[CQ:") {
            return None;
        }
        raw.replace("&#91;", "[")
            .replace("&#93;", "]")
            .replace("&#44;", ",")
            .replace("&amp;", "&")
    } else {
        let array = v["message"].as_array().filter(|v| v.len() <= 100)?;
        let mut text = String::new();
        let mut mentioned = false;
        for seg in array {
            if seg["type"] == "at"
                && id(&seg["data"]["qq"]).as_deref() == Some(self_id)
                && text.trim().is_empty()
                && !mentioned
            {
                mentioned = true;
                continue;
            }
            if seg["type"] != "text" || !seg["data"]["text"].is_string() {
                return None;
            }
            text.push_str(seg["data"]["text"].as_str()?);
        }
        text
    };
    content = content.trim().into();
    let mut chars = content.chars();
    if !matches!(chars.next(), Some('/' | '!' | '！'))
        || chars.next().is_none_or(char::is_whitespace)
        || content.encode_utf16().count() > 2000
    {
        return None;
    }
    Some(Message {
        id: format!("ob11:{self_id}:{group}:{mid}"),
        group_id: group,
        member_id: format!("ob11:{user}"),
        content,
    })
}
pub fn official(v: &Value, app: &str, now: i64) -> Option<Message> {
    if v["op"] != 0
        || !["GROUP_AT_MESSAGE_CREATE", "GROUP_MESSAGE_CREATE"].contains(&v["t"].as_str()?)
    {
        return None;
    }
    let d = &v["d"];
    let group = d["group_openid"].as_str().filter(|s| open_id(s))?;
    let member = d["author"]["member_openid"]
        .as_str()
        .or_else(|| d["author"]["user_openid"].as_str())
        .filter(|s| open_id(s))?;
    let mid = d["id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.encode_utf16().count() <= 512)?;
    let time = crate::integrity_enforcement::date(&d["timestamp"])?;
    if d["author"]["bot"] == true
        || (now - time.timestamp_millis()).abs() > 300000
        || d["attachments"].as_array().is_some_and(|a| !a.is_empty())
    {
        return None;
    }
    let raw = d["content"].as_str()?;
    if raw.encode_utf16().count() > 4000 {
        return None;
    }
    let raw = raw.trim_start();
    let raw = if raw.starts_with("<@") {
        if let Some(end) = raw.find('>') {
            let id = raw[2..end].trim_start_matches('!');
            if !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit()) {
                &raw[end + 1..]
            } else {
                raw
            }
        } else {
            raw
        }
    } else {
        raw
    };
    let content = raw.trim();
    let mut chars = content.chars();
    if !matches!(chars.next(), Some('/' | '!' | '！'))
        || chars.next().is_none_or(char::is_whitespace)
        || content.encode_utf16().count() > 2000
    {
        return None;
    }
    Some(Message {
        id: format!(
            "official:{app}:{group}:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mid)
        ),
        group_id: group.into(),
        member_id: format!("official:{app}:{member}"),
        content: content.into(),
    })
}
pub fn command(content: &str) -> (String, String) {
    let content = content.trim_start();
    let content = if content.starts_with("<@") {
        content
            .find('>')
            .filter(|end| {
                let id = content[2..*end].trim_start_matches('!');
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
            })
            .map(|end| &content[end + 1..])
            .unwrap_or(content)
    } else {
        content
    };
    let content = content.trim();
    let content = if matches!(content.chars().next(), Some('!' | '/' | '！')) {
        &content[content.chars().next().unwrap().len_utf8()..]
    } else {
        content
    };
    let mut parts = content.splitn(2, char::is_whitespace);
    (
        parts
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("帮助")
            .into(),
        parts.next().unwrap_or("").trim().into(),
    )
}
pub async fn accept(
    state: &crate::config::AppState,
    c: &crate::qq_config::Configuration,
    m: &Message,
) -> crate::error::Result<()> {
    let Some(policy) = c
        .stored
        .policies
        .iter()
        .find(|p| c.stored.enabled && p.enabled && p.groups.contains(&m.group_id))
    else {
        return Ok(());
    };
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("qq-event:{}", m.member_id))
        .execute(&mut *tx)
        .await?;
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM qq_inbox WHERE member_id=$1 AND created_at>now()-interval '1 minute' AND id<>$2").bind(&m.member_id).bind(&m.id).fetch_one(&mut *tx).await?;
    if count < 10 {
        sqlx::query("INSERT INTO qq_inbox(id,server_id,group_id,member_id,content) SELECT $1,s.id,$2,$3,$4 FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$5 AND o.suspended_at IS NULL ON CONFLICT DO NOTHING").bind(&m.id).bind(&m.group_id).bind(&m.member_id).bind(&m.content).bind(&policy.server_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
