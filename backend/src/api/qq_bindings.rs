use crate::{
    auth::{self, Actor, ServerScope},
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, ApiQuery},
    qq_config, qq_identity,
    webhooks::{clip, text},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize, Default)]
pub struct Query {
    #[serde(default)]
    pub q: String,
    pub page: Option<f64>,
}
async fn access(
    state: &AppState,
    h: &HeaderMap,
    m: &Method,
    id: &str,
    manage: bool,
) -> Result<(Actor, ServerScope)> {
    let a = auth::authenticate(state, h, m).await?;
    if a.key.is_some() {
        return Err(ApiError::forbidden());
    }
    let s = auth::server_scope(state, &a, id, "players.moderate").await?;
    if manage && !s.caps.contains(&"automation.manage".into()) {
        return Err(ApiError::forbidden());
    }
    Ok((a, s))
}
pub async fn view(state: &AppState, id: &str, q: &str, page: i64) -> Result<Value> {
    let q = clip(q.trim(), 128);
    const FILTER: &str = "l.server_id=$1 AND($2='' OR strpos(l.member_id,$2)>0 OR strpos(l.steam_id,$2)>0 OR strpos(lower(coalesce(p.name,'')),lower($2))>0)";
    const PLAYER: &str = "LEFT JOIN LATERAL(SELECT name FROM player_sessions WHERE server_id=l.server_id AND steam_id=l.steam_id ORDER BY last_seen DESC LIMIT 1)p ON true";
    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM qq_links l {PLAYER} WHERE {FILTER}"
    ))
    .bind(id)
    .bind(&q)
    .fetch_one(&state.db)
    .await?;
    let page = page.clamp(1, 100000).min(((total + 49) / 50).max(1));
    let links:Vec<Value>=sqlx::query_scalar(&format!("SELECT to_jsonb(r) FROM(SELECT l.member_id,l.steam_id,l.user_id,p.name,u.name AS account_name,m.previous_member_id FROM qq_links l {PLAYER} LEFT JOIN \"user\" u ON u.id=l.user_id LEFT JOIN LATERAL(SELECT detail->>'previousMemberId' AS previous_member_id FROM audit_log WHERE server_id=l.server_id AND target=l.member_id AND action='qq.binding.migrate' AND outcome='ok' AND detail->>'previousMemberId' LIKE 'ob11:%' ORDER BY id DESC LIMIT 1)m ON true WHERE {FILTER} ORDER BY l.member_id LIMIT 50 OFFSET $3)r")).bind(id).bind(&q).bind((page-1)*50).fetch_all(&state.db).await?;
    let recent:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('member_id',member_id,'last_seen',max(created_at)::text) FROM qq_inbox WHERE server_id=$1 AND created_at>now()-interval '30 days' AND member_id LIKE 'official:%' GROUP BY member_id ORDER BY max(created_at) DESC LIMIT 100").bind(id).fetch_all(&state.db).await?;
    Ok(json!({"links":links,"recent":recent,"total":total,"page":page,"query":q}))
}
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiQuery(q): ApiQuery<Query>,
) -> Result<Json<Value>> {
    access(&state, &h, &Method::GET, &id, false).await?;
    Ok(Json(
        view(
            &state,
            &id,
            &q.q,
            q.page.filter(|n| n.is_finite()).unwrap_or(1.) as i64,
        )
        .await?,
    ))
}
pub async fn post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    ApiJson(v): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (a, s) = access(&state, &h, &Method::POST, &id, true).await?;
    let action = text(&v["action"]);
    let member = text(&v["memberId"]);
    let reason = text(&v["reason"]).trim();
    let steam = text(&v["steamId"]);
    if !["add", "change", "remove", "migrate"].contains(&action)
        || !qq_config::identity(member)
        || reason.is_empty()
        || reason.encode_utf16().count() > 300
    {
        return Err(ApiError::bad(
            "请填写有效的用户标识、17 位 SteamID64 和处理原因。",
        ));
    }
    if action != "remove" {
        crate::api::notes::steam_id(steam)?
    }
    if action != "add" {
        crate::api::notes::steam_id(text(&v["expectedSteamId"]))?;
        if v.get("expectedUserId").is_none()
            || (!v["expectedUserId"].is_null() && !v["expectedUserId"].is_string())
        {
            return Err(ApiError::bad("缺少原绑定信息，请刷新页面。"));
        }
    }
    let source = if action == "migrate" {
        text(&v["previousMemberId"])
    } else {
        member
    };
    if action == "migrate"
        && (!source.starts_with("ob11:")
            || !qq_config::identity(source)
            || steam != text(&v["expectedSteamId"]))
    {
        return Err(ApiError::bad(
            "迁移需指定原旧版标识，SteamID 必须保持一致。",
        ));
    }
    let mut tx = state.db.begin().await?;
    qq_identity::lock(&mut tx, &id).await?;
    let c = qq_config::load_conn(&state, &mut tx).await?;
    if ["add", "migrate"].contains(&action)
        && (c.stored.provider != "official"
            || !member.starts_with(&format!("official:{}:", c.stored.self_id)))
    {
        return Err(ApiError::bad("补绑必须使用当前官方机器人的完整用户标识。"));
    }
    let before:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('steam_id',steam_id,'user_id',user_id) FROM qq_links WHERE server_id=$1 AND member_id=$2").bind(&id).bind(source).fetch_optional(&mut *tx).await?;
    if if action == "add" {
        before.is_some()
    } else {
        before.as_ref().is_none_or(|b| {
            b["steam_id"] != v["expectedSteamId"] || b["user_id"] != v["expectedUserId"]
        })
    } {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "binding_conflict",
            "绑定已发生变化，请刷新后重新操作。",
        ));
    }
    let conflict:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM qq_links WHERE server_id=$1 AND ((steam_id=$2 AND member_id<>$3) OR ($4 AND member_id=$5)))").bind(&id).bind(steam).bind(source).bind(action=="migrate").bind(member).fetch_one(&mut *tx).await?;
    if action != "remove" && conflict {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "binding_conflict",
            "该 SteamID 或官方用户已有绑定，请先核实原记录。",
        ));
    }
    match action {
        "remove" => {
            sqlx::query("DELETE FROM qq_links WHERE server_id=$1 AND member_id=$2")
                .bind(&id)
                .bind(member)
                .execute(&mut *tx)
                .await?;
        }
        "migrate" => {
            sqlx::query("UPDATE qq_links SET member_id=$3 WHERE server_id=$1 AND member_id=$2")
                .bind(&id)
                .bind(source)
                .bind(member)
                .execute(&mut *tx)
                .await?;
        }
        "add" => {
            sqlx::query(
                "INSERT INTO qq_links(server_id,member_id,user_id,steam_id) VALUES($1,$2,NULL,$3)",
            )
            .bind(&id)
            .bind(member)
            .bind(steam)
            .execute(&mut *tx)
            .await?;
        }
        _ => {
            sqlx::query("UPDATE qq_links SET steam_id=$3,user_id=NULL WHERE server_id=$1 AND member_id=$2 AND steam_id<>$3").bind(&id).bind(member).bind(steam).execute(&mut *tx).await?;
        }
    }
    sqlx::query("DELETE FROM qq_link_codes WHERE server_id=$1 AND member_id IN ($2,$3)")
        .bind(&id)
        .bind(member)
        .bind(source)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,server_id,server_name,org_id,category,action,target,detail,outcome) VALUES($1,$2,$3,$4,$5,'player',$6,$7,$8,'ok')").bind(&a.id).bind(&a.name).bind(&id).bind(&s.name).bind(&s.org_id).bind(format!("qq.binding.{action}")).bind(member).bind(json!({"before":before,"previousMemberId":source,"steamId":if action=="remove"{Value::Null}else{json!(steam)},"reason":reason})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn account(State(state): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::authenticate_account(&state, &h, &Method::GET).await?;
    let links:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('server_id',server_id,'steam_id',steam_id) FROM qq_links WHERE user_id=$1 ORDER BY server_id").bind(a.id).fetch_all(&state.db).await?;
    Ok(Json(json!({"links":links})))
}
pub async fn bind(
    State(state): State<AppState>,
    h: HeaderMap,
    ApiJson(v): ApiJson<Value>,
) -> Result<Json<Value>> {
    let a = auth::authenticate_account(&state, &h, &Method::POST).await?;
    let result = qq_identity::bind_account(&state, text(&v["code"]).trim(), &a).await?;
    Ok(Json(
        json!({"ok":true,"result":result,"message":"绑定成功，可以回 QQ 群使用机器人。"}),
    ))
}
pub async fn unbind(
    State(state): State<AppState>,
    h: HeaderMap,
    ApiJson(v): ApiJson<Value>,
) -> Result<Json<Value>> {
    let a = auth::authenticate_account(&state, &h, &Method::POST).await?;
    qq_identity::unbind_account(&state, text(&v["serverId"]), &a.id).await?;
    Ok(Json(
        json!({"ok":true,"message":"已解绑，Steam 账号的积分保留。"}),
    ))
}
