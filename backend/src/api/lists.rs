use crate::{
    audit,
    auth::{self, Actor, ServerScope},
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, ApiQuery, string},
    list_sync,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use std::collections::{HashMap, HashSet};
#[derive(Clone)]
pub struct Role {
    pub owner: bool,
    pub kinds: Vec<String>,
}
pub fn kind(s: &str) -> Result<&str> {
    if ["ban", "reserve"].contains(&s) {
        Ok(s)
    } else {
        Err(ApiError::missing())
    }
}
pub async fn role_for(state: &AppState, actor: &Actor, org: &str) -> Result<Option<Role>> {
    if let Some(key) = &actor.key {
        if key.org_id != org || key.server_ids.is_some() {
            return Ok(None);
        }
        let kinds = ["ban", "reserve"]
            .iter()
            .filter(|k| key.caps.contains(&format!("lists.{k}")))
            .map(|k| k.to_string())
            .collect::<Vec<_>>();
        return Ok((!kinds.is_empty()).then_some(Role {
            owner: false,
            kinds,
        }));
    }
    let owner:bool=actor.owner||sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM org_members WHERE org_id=$1 AND user_id=$2 AND role='owner')").bind(org).bind(&actor.id).fetch_one(&state.db).await?;
    if owner {
        return Ok(Some(Role {
            owner: true,
            kinds: vec!["ban".into(), "reserve".into()],
        }));
    }
    let caps:Vec<Value>=sqlx::query_scalar("SELECT r.capabilities FROM server_grants g JOIN servers s ON s.id=g.server_id JOIN org_roles r ON r.id=g.role_id WHERE g.user_id=$1 AND s.org_id=$2 AND r.org_id=$2").bind(&actor.id).bind(org).fetch_all(&state.db).await?;
    let kinds = ["ban", "reserve"]
        .iter()
        .filter(|k| {
            caps.iter()
                .any(|c| auth::json_strings(c).contains(&format!("lists.{k}")))
        })
        .map(|k| k.to_string())
        .collect::<Vec<_>>();
    Ok((!kinds.is_empty()).then_some(Role {
        owner: false,
        kinds,
    }))
}
async fn require(
    state: &AppState,
    headers: &HeaderMap,
    method: &Method,
    id: &str,
    need: &str,
) -> Result<(Actor, Value, Role)> {
    let actor = auth::authenticate(state, headers, method).await?;
    let org: Value = sqlx::query_scalar("SELECT to_jsonb(o) FROM organizations o WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::missing)?;
    let role = role_for(state, &actor, id)
        .await?
        .ok_or_else(ApiError::missing)?;
    if !org["suspended_at"].is_null() && !actor.owner {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "suspended",
            "Organization suspended.",
        ));
    }
    if (need == "owner" && !role.owner)
        || (["ban", "reserve"].contains(&need) && !role.kinds.iter().any(|k| k == need))
    {
        return Err(ApiError::forbidden());
    }
    Ok((actor, org, role))
}
pub async fn ensure(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    server: Option<&str>,
    k: &str,
) -> Result<String> {
    kind(k)?;
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO lists(id,org_id,server_id,kind,name) VALUES($1,$2,$3,$4,CASE WHEN $3::text IS NULL THEN 'Default' ELSE 'Server' END) ON CONFLICT DO NOTHING").bind(&id).bind(org).bind(server).bind(k).execute(&mut **tx).await?;
    let id:String=sqlx::query_scalar("SELECT id FROM lists WHERE org_id=$1 AND server_id IS NOT DISTINCT FROM $2 AND kind=$3 ORDER BY created_at LIMIT 1 FOR UPDATE").bind(org).bind(server).bind(k).fetch_one(&mut **tx).await?;
    sqlx::query("INSERT INTO server_lists(server_id,list_id) SELECT id,$1 FROM servers WHERE org_id=$2 AND ($3::text IS NULL OR id=$3) ON CONFLICT DO NOTHING").bind(&id).bind(org).bind(server).execute(&mut **tx).await?;
    Ok(id)
}
pub async fn names(
    state: &AppState,
    servers: &[String],
    ids: &[String],
) -> Result<HashMap<String, String>> {
    let rows:Vec<(String,String)>=sqlx::query_as("SELECT DISTINCT ON(steam_id) steam_id,name FROM player_sessions WHERE server_id=ANY($1) AND steam_id=ANY($2) ORDER BY steam_id,last_seen DESC").bind(servers).bind(ids).fetch_all(&state.db).await?;
    let mut out: HashMap<_, _> = rows.into_iter().collect();
    let missing = ids
        .iter()
        .filter(|id| !out.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let rows:Vec<(String,String)>=sqlx::query_as("SELECT steam_id,persona FROM steam_profiles WHERE steam_id=ANY($1) AND persona IS NOT NULL AND persona<>''").bind(&missing).fetch_all(&state.db).await?;
    out.extend(rows);
    Ok(out)
}
async fn refs(state: &AppState, org: &str) -> Result<Vec<(String, String)>> {
    Ok(
        sqlx::query_as("SELECT id,name FROM servers WHERE org_id=$1 ORDER BY sort_order,name")
            .bind(org)
            .fetch_all(&state.db)
            .await?,
    )
}
pub async fn view(state: &AppState, org: &Value, role: &Role) -> Result<Value> {
    let id = org["id"].as_str().unwrap();
    let mut tx = state.db.begin().await?;
    for k in ["ban", "reserve"] {
        ensure(&mut tx, id, None, k).await?;
    }
    tx.commit().await?;
    let lists:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',l.id,'kind',l.kind,'name',l.name,'entryCount',(SELECT count(*) FROM list_entries WHERE list_id=l.id AND removed_at IS NULL)) FROM lists l WHERE org_id=$1 AND server_id IS NULL AND kind=ANY($2) ORDER BY kind,name").bind(id).bind(&role.kinds).fetch_all(&state.db).await?;
    let servers:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'name',s.name,'syncedAt',y.synced_at,'lastError',coalesce(y.last_error,'')) FROM servers s LEFT JOIN server_list_sync y ON y.server_id=s.id WHERE s.org_id=$1 ORDER BY s.sort_order,s.name").bind(id).fetch_all(&state.db).await?;
    Ok(
        json!({"ok":true,"role":if role.owner{"owner"}else{"editor"},"kinds":role.kinds,"membersReserved":org["members_reserved"],"banMessage":if role.kinds.iter().any(|k|k=="ban"){org["ban_message"].clone()}else{Value::Null},"servers":servers,"lists":lists}),
    )
}
pub async fn org_view(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (_, org, role) = require(&state, &headers, &Method::GET, &id, "any").await?;
    Ok(Json(view(&state, &org, &role).await?))
}
pub async fn entries_view(
    state: &AppState,
    org: &Value,
    k: &str,
    removed: bool,
) -> Result<Vec<Value>> {
    let id = org["id"].as_str().unwrap();
    let mut tx = state.db.begin().await?;
    let list = ensure(&mut tx, id, None, k).await?;
    tx.commit().await?;
    let mut rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'steamId',steam_id,'reason',reason,'expiresAt',expires_at,'expired',coalesce(expires_at<=now() AND removed_at IS NULL,false),'addedByName',added_by_name,'addedAt',added_at,'removedAt',removed_at,'removedByName',removed_by_name,'removal',removal,'member',false) FROM list_entries WHERE list_id=$1 AND ($2 OR removed_at IS NULL) ORDER BY added_at DESC LIMIT 2000").bind(&list).bind(removed).fetch_all(&state.db).await?;
    if k == "reserve" && org["members_reserved"] == true && !removed {
        let members:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id','member:'||u.id,'steamId',u.steam_id,'reason',CASE WHEN coalesce(u.username,'')='' THEN 'member' ELSE 'member @'||u.username END,'fallbackName',CASE WHEN coalesce(u.username,'')='' THEN NULL ELSE '@'||u.username END,'expiresAt',NULL,'expired',false,'addedByName','','addedAt',m.created_at,'removedAt',NULL,'removedByName','','removal',NULL,'member',true) FROM org_members m JOIN \"user\" u ON u.id=m.user_id WHERE m.org_id=$1 AND u.steam_id IS NOT NULL AND NOT coalesce(u.banned,false) ORDER BY m.created_at").bind(id).fetch_all(&state.db).await?;
        for member in members {
            if !rows
                .iter()
                .any(|r| r["steamId"] == member["steamId"] && r["removedAt"].is_null())
            {
                rows.push(member)
            }
        }
    }
    let servers = refs(state, id).await?;
    let server_ids = servers.iter().map(|s| s.0.clone()).collect::<Vec<_>>();
    let ids = rows
        .iter()
        .filter_map(|r| r["steamId"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let names = names(state, &server_ids, &ids).await?;
    let seen:Vec<(String,String)>=sqlx::query_as("SELECT server_id,steam_id FROM server_reserved WHERE server_id=ANY($1) AND steam_id=ANY($2)").bind(&server_ids).bind(&ids).fetch_all(&state.db).await?;
    let seen: HashSet<_> = seen.into_iter().collect();
    let standing:Vec<(String,String,String,String)>=sqlx::query_as("SELECT server_id,steam_id,state,error FROM server_list_state WHERE kind=$1 AND server_id=ANY($2) AND steam_id=ANY($3)").bind(k).bind(&server_ids).bind(&ids).fetch_all(&state.db).await?;
    let standing: HashMap<_, _> = standing
        .into_iter()
        .map(|(s, p, state, error)| ((s, p), (state, error)))
        .collect();
    for row in &mut rows {
        let steam = row["steamId"].as_str().unwrap().to_owned();
        row["kind"] = json!(k);
        row["name"] = names
            .get(&steam)
            .map(|n| json!(n))
            .unwrap_or_else(|| row["fallbackName"].clone());
        row.as_object_mut().unwrap().remove("fallbackName");
        row["servers"] = if !row["removedAt"].is_null() {
            json!([])
        } else {
            json!(
                servers
                    .iter()
                    .map(|(id, name)| {
                        let (status, error) = if k == "ban" {
                            ("applied", "")
                        } else if let Some((s, e)) = standing.get(&(id.clone(), steam.clone())) {
                            (s.as_str(), e.as_str())
                        } else if seen.contains(&(id.clone(), steam.clone())) {
                            ("local", "")
                        } else {
                            ("pending", "")
                        };
                        json!({"serverId":id,"serverName":name,"state":status,"error":error})
                    })
                    .collect::<Vec<_>>()
            )
        };
    }
    Ok(rows)
}
pub async fn membership(
    state: &AppState,
    org: &str,
    steam: &str,
    role: Option<&Role>,
) -> Result<Value> {
    let mut out = serde_json::json!({"ban":null,"reserve":null,"canBan":false,"canReserve":false});
    if let Some(role) = role {
        let org: Value = sqlx::query_scalar("SELECT to_jsonb(o)FROM organizations o WHERE id=$1")
            .bind(org)
            .fetch_one(&state.db)
            .await?;
        for kind in &role.kinds {
            out[if kind == "ban" {
                "canBan"
            } else {
                "canReserve"
            }] = serde_json::json!(true);
            out[kind] = entries_view(state, &org, kind, false)
                .await?
                .into_iter()
                .find(|e| e["steamId"] == steam)
                .unwrap_or(Value::Null);
        }
    }
    Ok(out)
}
#[derive(Deserialize)]
pub struct Query {
    #[serde(rename = "includeRemoved")]
    pub removed: Option<String>,
}
pub async fn entries(
    State(state): State<AppState>,
    Path((id, k)): Path<(String, String)>,
    headers: HeaderMap,
    ApiQuery(q): ApiQuery<Query>,
) -> Result<Json<Value>> {
    kind(&k)?;
    let (_, org, _) = require(&state, &headers, &Method::GET, &id, &k).await?;
    Ok(Json(
        json!({"ok":true,"entries":entries_view(&state,&org,&k,q.removed.as_deref()==Some("1")).await?}),
    ))
}
fn expiry(v: &Value) -> Result<Option<DateTime<Utc>>> {
    let s = string(v, 40);
    if s.is_empty() {
        return Ok(None);
    }
    let d = DateTime::parse_from_rfc3339(&s)
        .map_err(|_| {
            ApiError::bad("expiresAt must be an ISO 8601 timestamp, or empty for permanent.")
        })?
        .to_utc();
    let delta = (d - Utc::now()).num_milliseconds();
    if delta < 10000 {
        return Err(ApiError::bad("expiresAt must be in the future."));
    }
    if delta > 315576000000 {
        return Err(ApiError::bad("expiresAt must be within ten years."));
    }
    Ok(Some(d))
}
/// The caller holds the list lock. One active entry per player; duplicate adds are idempotent internally.
pub async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    list: &str,
    steam: &str,
    reason: &str,
    expires: Option<DateTime<Utc>>,
    actor: Option<&str>,
    name: &str,
) -> Result<(String, bool)> {
    sqlx::query("SELECT 1 FROM lists WHERE id=$1 FOR UPDATE")
        .bind(list)
        .execute(&mut **tx)
        .await?;
    if let Some(id) = sqlx::query_scalar::<_, String>(
        "SELECT id FROM list_entries WHERE list_id=$1 AND steam_id=$2 AND removed_at IS NULL",
    )
    .bind(list)
    .bind(steam)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok((id, false));
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO list_entries(id,list_id,steam_id,reason,expires_at,added_by,added_by_name) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(&id).bind(list).bind(steam).bind(reason).bind(expires).bind(actor).bind(name).execute(&mut **tx).await?;
    Ok((id, true))
}
async fn change(
    state: &AppState,
    headers: &HeaderMap,
    method: &Method,
    id: &str,
    server: bool,
    k: &str,
    steam_in: &str,
    body: &Value,
) -> Result<Value> {
    kind(k)?;
    super::notes::steam_id(steam_in)?;
    let steam = steam_in;
    let actor = auth::authenticate(state, headers, method).await?;
    let org_id = if server {
        auth::server_scope(
            state,
            &actor,
            id,
            if k == "ban" {
                "bans.manage"
            } else {
                "slots.manage"
            },
        )
        .await?
        .org_id
    } else {
        require(state, headers, method, id, k).await?;
        id.into()
    };
    let mut tx = state.db.begin().await?;
    super::users::account_lock(&mut tx).await?;
    let _: String = sqlx::query_scalar("SELECT id FROM organizations WHERE id=$1 FOR SHARE")
        .bind(&org_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::missing)?;
    // Re-check sessions, API-key scopes and grants after queueing on identity/organization locks.
    let actor = auth::authenticate(state, headers, method).await?;
    if server {
        auth::server_scope(
            state,
            &actor,
            id,
            if k == "ban" {
                "bans.manage"
            } else {
                "slots.manage"
            },
        )
        .await?;
    } else {
        require(state, headers, method, id, k).await?;
    }
    let list = ensure(&mut tx, &org_id, server.then_some(id), k).await?;
    let mut output = json!({});
    let action;
    if method == Method::POST {
        let reason = string(&body["reason"], 200);
        let expires = expiry(&body["expiresAt"])?;
        let (entry, added) = insert(
            &mut tx,
            &list,
            steam,
            &reason,
            expires,
            Some(&actor.id),
            &actor.name,
        )
        .await?;
        if !added {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "duplicate",
                "Player is already on this list.",
            ));
        }
        output = json!({"entryId":entry});
        action = "list.add";
    } else if method == Method::DELETE {
        let n=sqlx::query("UPDATE list_entries SET removed_at=now(),removed_by=$3,removed_by_name=$4,removal='manual' WHERE list_id=$1 AND steam_id=$2 AND removed_at IS NULL").bind(&list).bind(steam).bind(&actor.id).bind(&actor.name).execute(&mut *tx).await?.rows_affected();
        if n == 0 {
            return Err(ApiError::missing());
        }
        action = "list.remove";
    } else {
        if body.get("reason").is_none() && body.get("expiresAt").is_none() {
            return Err(ApiError::bad(
                "Nothing to change: send reason, expiresAt or both.",
            ));
        }
        let reason = body.get("reason").map(|v| string(v, 200));
        let expires = if body.get("expiresAt").is_some() {
            expiry(&body["expiresAt"])?
        } else {
            None
        };
        let entry:Value=sqlx::query_scalar("UPDATE list_entries SET reason=coalesce($3,reason),expires_at=CASE WHEN $4 THEN $5 ELSE expires_at END WHERE list_id=$1 AND steam_id=$2 AND removed_at IS NULL RETURNING jsonb_build_object('steamId',steam_id,'reason',reason,'expiresAt',expires_at)").bind(&list).bind(steam).bind(reason).bind(body.get("expiresAt").is_some()).bind(expires).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
        output = json!({"entry":entry});
        action = "list.update";
    }
    sqlx::query("UPDATE lists SET updated_at=now() WHERE id=$1")
        .bind(&list)
        .execute(&mut *tx)
        .await?;
    audit::event(&mut tx,&actor,Some(&org_id),headers,if server{"server"}else{"org"},action,steam,json!({"kind":k,"listId":list,"serverId":server.then_some(id),"reason":body["reason"],"expiresAt":body["expiresAt"]})).await?;
    if server {
        super::servers::observe(&mut tx, id, false).await?;
    } else {
        sqlx::query("SELECT pg_notify('warcon_observe',json_build_object('orgId',$1::text)::text)")
            .bind(&org_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    output["ok"] = json!(true);
    if method != Method::PATCH {
        output["sync"] = if server {
            list_sync::reconcile(state, id, 0).await?
        } else {
            list_sync::sync_org(state, id).await?
        };
    }
    if method == Method::POST && !server {
        let (_, org, _) = require(state, headers, &Method::GET, id, k).await?;
        let entry_id = output["entryId"].clone();
        output["entry"] = entries_view(state, &org, k, false)
            .await?
            .into_iter()
            .find(|r| r["id"] == entry_id)
            .unwrap_or(Value::Null);
        output.as_object_mut().unwrap().remove("entryId");
    }
    Ok(output)
}
pub async fn add(
    State(state): State<AppState>,
    Path((id, k)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let steam = string(&body["steamId"], 100);
    Ok((
        StatusCode::CREATED,
        Json(
            change(
                &state,
                &headers,
                &Method::POST,
                &id,
                false,
                &k,
                &steam,
                &body,
            )
            .await?,
        ),
    ))
}
pub async fn update(
    State(state): State<AppState>,
    Path((id, k, steam)): Path<(String, String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        change(
            &state,
            &headers,
            &Method::PATCH,
            &id,
            false,
            &k,
            &steam,
            &body,
        )
        .await?,
    ))
}
pub async fn remove(
    State(state): State<AppState>,
    Path((id, k, steam)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        change(
            &state,
            &headers,
            &Method::DELETE,
            &id,
            false,
            &k,
            &steam,
            &json!({}),
        )
        .await?,
    ))
}
pub async fn server_add(
    State(state): State<AppState>,
    Path((id, k)): Path<(String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let steam = string(&body["steamId"], 100);
    Ok((
        StatusCode::CREATED,
        Json(
            change(
                &state,
                &headers,
                &Method::POST,
                &id,
                true,
                &k,
                &steam,
                &body,
            )
            .await?,
        ),
    ))
}
pub async fn server_update(
    State(state): State<AppState>,
    Path((id, k, steam)): Path<(String, String, String)>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        change(
            &state,
            &headers,
            &Method::PATCH,
            &id,
            true,
            &k,
            &steam,
            &body,
        )
        .await?,
    ))
}
pub async fn server_remove(
    State(state): State<AppState>,
    Path((id, k, steam)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        change(
            &state,
            &headers,
            &Method::DELETE,
            &id,
            true,
            &k,
            &steam,
            &json!({}),
        )
        .await?,
    ))
}
pub async fn sync(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    require(&state, &headers, &Method::POST, &id, "any").await?;
    Ok(Json(
        json!({"ok":true,"sync":list_sync::sync_org(&state,&id).await?}),
    ))
}
pub async fn server_sync(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = auth::authenticate(&state, &headers, &Method::POST).await?;
    let scope = auth::server_scope(&state, &actor, &id, "server.view").await?;
    if !scope
        .caps
        .iter()
        .any(|c| ["lists.ban", "lists.reserve"].contains(&c.as_str()))
    {
        return Err(ApiError::forbidden());
    }
    list_sync::expire_entries(&state).await?;
    Ok(Json(
        json!({"ok":true,"sync":list_sync::reconcile(&state,&id,0).await?}),
    ))
}

fn slot(status: &str, managed: bool) -> Value {
    json!({"state":status,"managed":managed,"name":null,"note":"","member":false,"scope":"org","expiresAt":null})
}
fn ban(status: &str, managed: bool) -> Value {
    json!({"state":status,"managed":managed,"scope":"org","reason":"","addedByName":"","addedAt":null,"expiresAt":null})
}
pub async fn state_view(state: &AppState, actor: &Actor, scope: &ServerScope) -> Result<Value> {
    let id = &scope.id;
    let org = &scope.org_id;
    let role = role_for(state, actor, org).await?;
    let org_ban = role
        .as_ref()
        .is_some_and(|r| r.kinds.iter().any(|k| k == "ban"));
    let org_slot = role
        .as_ref()
        .is_some_and(|r| r.kinds.iter().any(|k| k == "reserve"));
    let staff = org_ban || scope.caps.iter().any(|c| c == "bans.manage");
    let o: Value = sqlx::query_scalar("SELECT to_jsonb(o) FROM organizations o WHERE id=$1")
        .bind(org)
        .fetch_one(&state.db)
        .await?;
    let sync:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('syncedAt',synced_at,'lastError',last_error) FROM server_list_sync WHERE server_id=$1").bind(id).fetch_optional(&state.db).await?;
    let mut out = json!({"ok":true,"canEditOrgBans":org_ban,"canEditOrgSlots":org_slot,"orgOwner":role.as_ref().is_some_and(|r|r.owner),"orgId":org,"banMessage":if staff{o["ban_message"].clone()}else{Value::Null},"bans":{},"reserved":{},"sync":sync});
    let bans: Vec<String> =
        sqlx::query_scalar("SELECT steam_id FROM server_bans WHERE server_id=$1")
            .bind(id)
            .fetch_all(&state.db)
            .await?;
    let slots: Vec<String> =
        sqlx::query_scalar("SELECT steam_id FROM server_reserved WHERE server_id=$1")
            .bind(id)
            .fetch_all(&state.db)
            .await?;
    for s in bans {
        out["bans"][&s] = ban("local", false)
    }
    for s in slots {
        out["reserved"][&s] = slot("local", false)
    }
    let managed: Vec<(String, String)> = sqlx::query_as(
        "SELECT steam_id,state FROM server_list_state WHERE server_id=$1 AND kind='reserve'",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    for (s, status) in managed {
        out["reserved"][&s] = slot(&status, true)
    }
    let desired = list_sync::desired_for(state, id, org, o["members_reserved"] == true).await?;
    for k in ["bans", "reserved"] {
        let rows = desired[k].as_array().unwrap();
        for r in rows {
            let steam = r["steamId"].as_str().unwrap();
            if k == "bans" {
                out[k][steam] = ban("applied", true)
            } else {
                if out[k].get(steam).is_none() {
                    out[k][steam] = slot("pending", true)
                }
                out[k][steam]["member"] = r["member"].clone();
            }
            let entry:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(e)||jsonb_build_object('scope',CASE WHEN l.server_id IS NULL THEN 'org' ELSE 'server' END) FROM list_entries e JOIN lists l ON l.id=e.list_id WHERE e.list_id=$1 AND e.steam_id=$2 AND e.removed_at IS NULL").bind(r["listId"].as_str()).bind(steam).fetch_optional(&state.db).await?;
            if let Some(e) = entry {
                out[k][steam]["scope"] = e["scope"].clone();
                out[k][steam]["expiresAt"] = e["expires_at"].clone();
                if k == "bans" {
                    out[k][steam]["reason"] = e["reason"].clone();
                    out[k][steam]["addedAt"] = e["added_at"].clone();
                    if staff {
                        out[k][steam]["addedByName"] = e["added_by_name"].clone()
                    }
                } else if org_slot || scope.caps.iter().any(|c| c == "slots.manage") {
                    out[k][steam]["note"] = e["reason"].clone();
                }
            }
        }
    }
    let ids = out["reserved"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let servers = refs(state, org)
        .await?
        .into_iter()
        .map(|s| s.0)
        .collect::<Vec<_>>();
    for (s, n) in names(state, &servers, &ids).await? {
        out["reserved"][&s]["name"] = json!(n)
    }
    Ok(out)
}
pub async fn server_state(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let actor = auth::authenticate(&state, &headers, &Method::GET).await?;
    let scope = auth::server_scope(&state, &actor, &id, "server.view").await?;
    Ok(Json(state_view(&state, &actor, &scope).await?))
}
pub async fn candidates(state: &AppState, org: &str) -> Result<Vec<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("WITH observed AS(SELECT server_id,steam_id,'ban'::text AS kind,reason,banned_by FROM server_bans UNION ALL SELECT server_id,steam_id,'reserve','', '' FROM server_reserved) SELECT jsonb_build_object('kind',b.kind,'steamId',b.steam_id,'serverId',s.id,'serverName',s.name,'reason',b.reason,'bannedBy',b.banned_by) FROM observed b JOIN servers s ON s.id=b.server_id WHERE s.org_id=$1 AND NOT EXISTS(SELECT 1 FROM lists l JOIN list_entries e ON e.list_id=l.id WHERE l.org_id=$1 AND l.server_id IS NULL AND l.kind=b.kind AND e.steam_id=b.steam_id AND e.removed_at IS NULL) AND NOT EXISTS(SELECT 1 FROM server_list_state t WHERE t.server_id=b.server_id AND t.kind=b.kind AND t.steam_id=b.steam_id) ORDER BY b.kind,b.steam_id,s.sort_order,s.name").bind(org).fetch_all(&state.db).await?;
    let mut groups: HashMap<(String, String), Value> = HashMap::new();
    for r in rows {
        let key = (
            r["kind"].as_str().unwrap().into(),
            r["steamId"].as_str().unwrap().into(),
        );
        let g = groups.entry(key).or_insert_with(
            || json!({"kind":r["kind"],"steamId":r["steamId"],"name":null,"servers":[]}),
        );
        g["servers"].as_array_mut().unwrap().push(json!({"serverId":r["serverId"],"serverName":r["serverName"],"reason":r["reason"],"bannedBy":r["bannedBy"]}));
    }
    let mut out = groups.into_values().collect::<Vec<_>>();
    let ids = out
        .iter()
        .map(|r| r["steamId"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let servers = refs(state, org)
        .await?
        .into_iter()
        .map(|s| s.0)
        .collect::<Vec<_>>();
    let names = names(state, &servers, &ids).await?;
    for g in &mut out {
        g["name"] = names
            .get(g["steamId"].as_str().unwrap())
            .map(|n| json!(n))
            .unwrap_or(Value::Null)
    }
    out.sort_by(|a, b| {
        b["servers"]
            .as_array()
            .unwrap()
            .len()
            .cmp(&a["servers"].as_array().unwrap().len())
            .then_with(|| a["steamId"].as_str().cmp(&b["steamId"].as_str()))
    });
    Ok(out)
}
pub async fn import_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    require(&state, &headers, &Method::GET, &id, "owner").await?;
    Ok(Json(
        json!({"ok":true,"candidates":candidates(&state,&id).await?}),
    ))
}
pub async fn import_post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    require(&state, &headers, &Method::POST, &id, "owner").await?;
    let picks = body["entries"]
        .as_array()
        .map(|a| a.iter().take(500).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if picks.is_empty() {
        return Err(ApiError::bad("Nothing to import."));
    }
    for p in &picks {
        kind(p["kind"].as_str().unwrap_or(""))?;
        super::notes::steam_id(p["steamId"].as_str().unwrap_or(""))?;
    }
    let mut tx = state.db.begin().await?;
    super::users::account_lock(&mut tx).await?;
    let (actor, _, _) = require(&state, &headers, &Method::POST, &id, "owner").await?;
    let _: String = sqlx::query_scalar("SELECT id FROM organizations WHERE id=$1 FOR SHARE")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
    let available = candidates(&state, &id).await?;
    let mut adopted = vec![];
    for p in &picks {
        let Some(c) = available
            .iter()
            .find(|c| c["kind"] == p["kind"] && c["steamId"] == p["steamId"])
        else {
            continue;
        };
        let k = p["kind"].as_str().unwrap();
        let steam = p["steamId"].as_str().unwrap();
        let list = ensure(&mut tx, &id, None, k).await?;
        let mut reason = string(&p["reason"], 200);
        if reason.is_empty() {
            reason = c["servers"]
                .as_array()
                .unwrap()
                .iter()
                .find_map(|s| s["reason"].as_str().filter(|s| !s.is_empty()))
                .unwrap_or("")
                .into()
        }
        let (_, added) = insert(
            &mut tx,
            &list,
            steam,
            &reason,
            None,
            Some(&actor.id),
            &actor.name,
        )
        .await?;
        if !added {
            continue;
        }
        for s in c["servers"].as_array().unwrap() {
            sqlx::query("INSERT INTO server_list_state(server_id,kind,steam_id,source_list_id,state,error,attempted_at,updated_at) VALUES($1,$2,$3,$4,'applied','',now(),now()) ON CONFLICT(server_id,kind,steam_id) DO UPDATE SET source_list_id=excluded.source_list_id,state='applied',error='',updated_at=now()").bind(s["serverId"].as_str()).bind(k).bind(steam).bind(&list).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE lists SET updated_at=now() WHERE id=$1")
            .bind(&list)
            .execute(&mut *tx)
            .await?;
        adopted.push(format!("{k}:{steam}"));
    }
    if !adopted.is_empty() {
        audit::event(
            &mut tx,
            &actor,
            Some(&id),
            &headers,
            "org",
            "list.import",
            &format!("{} entries", adopted.len()),
            json!({"orgId":id,"entries":adopted}),
        )
        .await?;
    }
    tx.commit().await?;
    let sync = if adopted.is_empty() {
        json!({"servers":[]})
    } else {
        list_sync::sync_org(&state, &id).await?
    };
    Ok(Json(
        json!({"ok":true,"imported":adopted.len(),"skipped":picks.len()-adopted.len(),"sync":sync}),
    ))
}
