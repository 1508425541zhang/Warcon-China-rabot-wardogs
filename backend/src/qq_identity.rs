//! QQ identity changes serialize per server and preserve Steam keyed balances.
use crate::{
    auth::Actor,
    config::AppState,
    error::{ApiError, Result},
    qq_config,
    webhooks::text,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use unicode_normalization::UnicodeNormalization;
pub async fn lock(tx: &mut Transaction<'_, Postgres>, server: &str) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("qq-bind:{server}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}
fn conflict(message: &str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "binding_conflict", message)
}
pub async fn bind_player(
    state: &AppState,
    server: &str,
    member: &str,
    steam: &str,
    name: &str,
    roster: &Value,
) -> Result<Value> {
    crate::api::notes::steam_id(steam)?;
    if !qq_config::identity(member) || name.trim().is_empty() || name.encode_utf16().count() > 100 {
        return Err(ApiError::bad("格式：/绑定 SteamID64 游戏内昵称"));
    }
    let actual = roster["players"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["steamId"] == steam))
        .and_then(|p| p["name"].as_str())
        .map(str::to_owned);
    let actual = if actual.is_some() {
        actual
    } else {
        sqlx::query_scalar("SELECT name FROM player_sessions WHERE server_id=$1 AND steam_id=$2 ORDER BY last_seen DESC LIMIT 1").bind(server).bind(steam).fetch_optional(&state.db).await?
    };
    if actual.is_none_or(|actual| {
        actual.trim().nfc().collect::<String>() != name.trim().nfc().collect::<String>()
    }) {
        return Err(ApiError::bad(
            "SteamID64 与本服游戏昵称不匹配，请核对后重新输入。",
        ));
    }
    let mut tx = state.worker_transaction().await?;
    lock(&mut tx, server).await?;
    let c = qq_config::load_conn(state, &mut tx).await?;
    if c.policy(server).is_none() {
        return Err(ApiError::missing());
    }
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(l) FROM qq_links l WHERE server_id=$1 AND (member_id=$2 OR steam_id=$3)",
    )
    .bind(server)
    .bind(member)
    .bind(steam)
    .fetch_all(&mut *tx)
    .await?;
    if rows
        .iter()
        .any(|r| r["member_id"] == member && r["steam_id"] == steam)
    {
        return Ok(json!({"steamId":steam,"already":true}));
    }
    if rows.iter().any(|r| r["member_id"] == member) {
        return Err(conflict("你已绑定其他 SteamID，请先发送 /解绑。"));
    }
    let legacy = rows
        .iter()
        .find(|r| r["steam_id"] == steam && text(&r["member_id"]).starts_with("ob11:"));
    let mut migrated = false;
    if let Some(old) = legacy.filter(|_| {
        c.stored.provider == "official"
            && member.starts_with(&format!("official:{}:", c.stored.self_id))
    }) {
        sqlx::query(
            "UPDATE qq_links SET member_id=$3 WHERE server_id=$1 AND member_id=$2 AND steam_id=$4",
        )
        .bind(server)
        .bind(text(&old["member_id"]))
        .bind(member)
        .bind(steam)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM qq_link_codes WHERE server_id=$1 AND member_id IN ($2,$3)")
            .bind(server)
            .bind(member)
            .bind(text(&old["member_id"]))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO audit_log(actor_name,server_id,server_name,org_id,category,action,target,detail,outcome) SELECT 'QQ 玩家',id,name,org_id,'player','qq.binding.migrate',$2,$3,'ok' FROM servers WHERE id=$1").bind(server).bind(member).bind(json!({"previousMemberId":old["member_id"],"memberId":member,"steamId":steam,"reason":"官方签名绑定请求通过 SteamID 与本服昵称核验"})).execute(&mut *tx).await?;
        migrated = true;
    } else {
        if !rows.is_empty() {
            return Err(conflict("该 SteamID 已绑定其他 QQ，请联系管理员处理。"));
        }
        let added=sqlx::query("INSERT INTO qq_links(server_id,member_id,user_id,steam_id) VALUES($1,$2,NULL,$3) ON CONFLICT DO NOTHING").bind(server).bind(member).bind(steam).execute(&mut *tx).await?;
        if added.rows_affected() != 1 {
            return Err(conflict(
                "QQ 或 SteamID 已绑定，请查询绑定状态或联系管理员。",
            ));
        }
        sqlx::query("DELETE FROM qq_link_codes WHERE server_id=$1 AND member_id=$2")
            .bind(server)
            .bind(member)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(json!({"steamId":steam,"already":false,"migrated":migrated}))
}
pub async fn unbind_member(state: &AppState, server: &str, member: &str) -> Result<()> {
    let mut tx = state.worker_transaction().await?;
    lock(&mut tx, server).await?;
    sqlx::query("DELETE FROM qq_links WHERE server_id=$1 AND member_id=$2")
        .bind(server)
        .bind(member)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM qq_link_codes WHERE server_id=$1 AND member_id=$2")
        .bind(server)
        .bind(member)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn unbind_account(state: &AppState, server: &str, user: &str) -> Result<()> {
    let mut tx = state.db.begin().await?;
    lock(&mut tx, server).await?;
    sqlx::query("WITH removed AS(DELETE FROM qq_links WHERE user_id=$1 AND server_id=$2 RETURNING member_id) DELETE FROM qq_link_codes WHERE server_id=$2 AND member_id IN (SELECT member_id FROM removed)").bind(user).bind(server).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn link_code(state: &AppState, server: &str, member: &str) -> Result<String> {
    if !qq_config::identity(member) {
        return Err(ApiError::bad("用户标识无效。"));
    }
    let code = hex::encode(rand::random::<[u8; 16]>());
    let mut tx = state.worker_transaction().await?;
    lock(&mut tx, server).await?;
    if qq_config::load_conn(state, &mut tx)
        .await?
        .policy(server)
        .is_none()
    {
        return Err(ApiError::missing());
    }
    sqlx::query(
        "DELETE FROM qq_link_codes WHERE expires_at<now() OR(server_id=$1 AND member_id=$2)",
    )
    .bind(server)
    .bind(member)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO qq_link_codes(code_hash,server_id,member_id,expires_at) VALUES($1,$2,$3,now()+interval '5 minutes')").bind(crate::crypto::hash_token(&code)).bind(server).bind(member).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(code)
}
pub async fn bind_account(state: &AppState, code: &str, actor: &Actor) -> Result<Value> {
    if code.len() != 32
        || !code
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || actor.key.is_some()
    {
        return Err(ApiError::bad("绑定码无效。"));
    }
    let mut tx = state.db.begin().await?;
    let hash = crate::crypto::hash_token(code);
    let server: String = sqlx::query_scalar(
        "SELECT server_id FROM qq_link_codes WHERE code_hash=$1 AND expires_at>now()",
    )
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::bad("绑定码已过期或已使用。"))?;
    lock(&mut tx, &server).await?;
    let request:Value=sqlx::query_scalar("DELETE FROM qq_link_codes WHERE code_hash=$1 AND expires_at>now() RETURNING jsonb_build_object('server',server_id,'member',member_id)").bind(&hash).fetch_optional(&mut *tx).await?.ok_or_else(||ApiError::bad("绑定码已过期或已使用。"))?;
    if qq_config::load_conn(state, &mut tx)
        .await?
        .policy(&server)
        .is_none()
    {
        return Err(ApiError::bad("绑定码已过期或已使用。"));
    }
    let steam: String = sqlx::query_scalar(
        "SELECT account_id FROM account WHERE user_id=$1 AND provider_id='steam' LIMIT 1",
    )
    .bind(&actor.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "steam_link_required",
            "请先在账号设置中登录 Steam 完成验证。",
        )
    })?;
    crate::api::notes::steam_id(&steam)?;
    let added=sqlx::query("INSERT INTO qq_links(server_id,member_id,user_id,steam_id) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(&server).bind(text(&request["member"])).bind(&actor.id).bind(&steam).execute(&mut *tx).await?;
    if added.rows_affected() != 1 {
        return Err(conflict("QQ 或 Steam 已绑定；如需更换，请先在此页面解绑。"));
    }
    tx.commit().await?;
    Ok(json!({"serverId":server,"steamId":steam}))
}
pub async fn linked(state: &AppState, server: &str, member: &str) -> Result<(String, Actor)> {
    let row:Value=sqlx::query_scalar("SELECT jsonb_build_object('steam',l.steam_id,'user',l.user_id,'name',coalesce(u.username,u.name),'role',u.role) FROM qq_links l LEFT JOIN \"user\" u ON u.id=l.user_id WHERE l.server_id=$1 AND l.member_id=$2 AND(l.user_id IS NULL OR (NOT coalesce(u.banned,false) AND EXISTS(SELECT 1 FROM account a WHERE a.user_id=u.id AND a.provider_id='steam' AND a.account_id=l.steam_id)))").bind(server).bind(member).fetch_optional(&state.db).await?.ok_or_else(||ApiError::new(StatusCode::FORBIDDEN,"binding_required","尚未绑定。格式：/绑定 SteamID64 游戏内昵称"))?;
    Ok((
        text(&row["steam"]).into(),
        Actor {
            id: row["user"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("qq:{member}")),
            name: row["name"].as_str().unwrap_or(member).into(),
            owner: row["role"] == "owner",
            key: None,
        },
    ))
}
