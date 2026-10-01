use super::orgs;
use crate::{
    audit,
    auth::{self, Actor, ServerScope},
    config::AppState,
    crypto,
    error::{ApiError, Result},
    http::{ApiJson, integer, string, truthy},
    rcon,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
async fn manager(
    state: &AppState,
    headers: &HeaderMap,
    method: &Method,
    id: &str,
) -> Result<(Actor, ServerScope)> {
    let actor = auth::authenticate(state, headers, method).await?;
    let scope = auth::server_scope(state, &actor, id, "server.view").await?;
    if !scope.manager || actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    Ok((actor, scope))
}
pub async fn accessible(state: &AppState, actor: &Actor, org: Option<&str>) -> Result<Vec<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s)||jsonb_build_object('org_name',o.name,'allow_public_status',o.allow_public_status,'allow_public_leaderboards',o.allow_public_leaderboards) FROM servers s JOIN organizations o ON o.id=s.org_id WHERE ($1::text IS NULL OR s.org_id=$1) ORDER BY o.name,s.sort_order,s.name").bind(org).fetch_all(&state.db).await?;
    let mut result = vec![];
    for row in rows {
        let id = row["id"].as_str().unwrap_or("");
        let scope = match auth::server_scope(state, actor, id, "server.view").await {
            Ok(scope) => scope,
            Err(e) if [StatusCode::NOT_FOUND, StatusCode::FORBIDDEN].contains(&e.status) => {
                continue;
            }
            Err(e) => return Err(e),
        };
        let role = if actor.key.is_some() {
            "API key".into()
        } else if scope.manager {
            "owner".into()
        } else {
            sqlx::query_scalar::<_,String>("SELECT r.name FROM server_grants g JOIN org_roles r ON r.id=g.role_id WHERE g.server_id=$1 AND g.user_id=$2").bind(id).bind(&actor.id).fetch_optional(&state.db).await?.unwrap_or_default()
        };
        result.push(json!({"id":id,"orgId":scope.org_id,"orgName":row["org_name"],"name":row["name"],"host":if scope.manager{row["host"].clone()}else{json!("")},"port":if scope.manager{row["port"].clone()}else{json!(0)},"scheme":if scope.manager{row["scheme"].clone()}else{json!("http")},"notes":if scope.manager{row["notes"].clone()}else{json!("")},"roleName":role,"caps":scope.caps,"manager":scope.manager,"sortOrder":row["sort_order"],"demo":row["host"]=="demo"&&std::env::var("ALLOW_DEMO_SERVER").is_ok_and(|v|v!="false"&&v!="0"),"publicStatus":row["public_status"],"publicLeaderboards":row["public_leaderboards"],"publicKills":row["public_kills"],"allowPublicStatus":row["allow_public_status"],"allowPublicLeaderboards":row["allow_public_leaderboards"]}));
    }
    Ok(result)
}
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let actor = auth::authenticate(&state, &headers, &Method::GET).await?;
    Ok(Json(
        json!({"ok":true,"servers":accessible(&state,&actor,None).await?}),
    ))
}
fn switches(org: &Value, body: &Value, current: &mut Value) -> Result<()> {
    for (camel, snake, allow) in [
        ("publicStatus", "public_status", "allow_public_status"),
        (
            "publicLeaderboards",
            "public_leaderboards",
            "allow_public_leaderboards",
        ),
        ("publicKills", "public_kills", ""),
    ] {
        if let Some(v) = body.get(camel) {
            let on = truthy(v);
            if on && !allow.is_empty() && org[allow] != true {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "not_allowed",
                    "Public feature closed for this organisation by the site owner.",
                ));
            }
            current[snake] = json!(on)
        }
    }
    Ok(())
}
async fn target(actor: &Actor, body: &Value, current: Option<&Value>) -> Result<(Value, bool)> {
    let mut row=current.cloned().unwrap_or_else(||json!({"notes":"","sort_order":0,"public_status":false,"public_leaderboards":false,"public_kills":false}));
    if current.is_none() || body.get("name").is_some() {
        let n = string(&body["name"], 80);
        if n.is_empty() {
            return Err(ApiError::bad("name is required."));
        }
        row["name"] = json!(n)
    }
    if current.is_none() || body.get("host").is_some() {
        row["host"] = json!(
            rcon::normalise_host(&string(&body["host"], 253))
                .map_err(|_| ApiError::bad("Invalid host."))?
        )
    }
    if current.is_none() || body.get("port").is_some() {
        let port = integer(&body["port"], 0, 1, 65535);
        if port == 0 {
            return Err(ApiError::bad("port must be 1-65535."));
        }
        row["port"] = json!(port)
    }
    if current.is_none() || body.get("scheme").is_some() {
        row["scheme"] = json!(if body["scheme"] == "https" {
            "https"
        } else {
            "http"
        })
    }
    if let Some(v) = body.get("notes") {
        row["notes"] = json!(string(v, 2000))
    }
    if let Some(v) = body.get("sortOrder") {
        row["sort_order"] = json!(integer(v, 0, -1000, 1000))
    }
    let moved = current.is_none_or(|c| {
        ["host", "port", "scheme"]
            .iter()
            .any(|key| row[*key] != c[*key])
    });
    if moved {
        if current.is_some() && !body["password"].as_str().is_some_and(|s| !s.is_empty()) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "password_required",
                "Changing the host, port or scheme needs the RCON password again.",
            ));
        }
        let allowed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            rcon::resolve_target(
                row["host"].as_str().unwrap(),
                row["port"].as_u64().unwrap() as u16,
                row["scheme"].as_str().unwrap(),
                actor.owner,
            ),
        )
        .await;
        if !allowed.is_ok_and(|r| r.is_ok()) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "blocked_host",
                "The host could not be resolved to a permitted game server address.",
            ));
        }
        row["allow_private"] = json!(actor.owner);
    } else if actor.owner {
        row["allow_private"] = json!(true)
    }
    Ok((row, moved))
}
async fn trail(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    headers: &HeaderMap,
    id: &str,
    org: &str,
    name: &str,
    action: &str,
    target: &str,
    detail: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,org_id,server_id,server_name,category,action,target,detail,outcome,user_agent) VALUES($1,$2,$3,$4,$5,'server',$6,$7,$8,'ok',$9)").bind(&actor.id).bind(&actor.name).bind(org).bind(id).bind(name).bind(action).bind(target).bind(audit::redact(&detail,0)).bind(crate::feed::truncate(headers.get("user-agent").and_then(|v|v.to_str().ok()).unwrap_or(""),300)).execute(&mut **tx).await?;
    Ok(())
}
pub async fn observe(tx: &mut Transaction<'_, Postgres>, id: &str, identity: bool) -> Result<()> {
    sqlx::query("SELECT pg_notify('warcon_observe',$1)")
        .bind(json!({"serverId":id,"identity":identity}).to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let actor = auth::authenticate(&state, &headers, &Method::POST).await?;
    if actor.key.is_some() {
        return Err(ApiError::forbidden());
    }
    let org = string(&body["orgId"], 64);
    if org.is_empty() {
        return Err(ApiError::bad("orgId is required."));
    }
    let password = body["password"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad("The server's RCON password is required."))?;
    if password.len() > 65536 {
        return Err(ApiError::bad("RCON password is too long."));
    }
    let mut tx = state.db.begin().await?;
    let o = orgs::locked_org(&mut tx, &actor, &org).await?;
    if !actor.owner {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM servers WHERE org_id=$1")
            .bind(&org)
            .fetch_one(&mut *tx)
            .await?;
        if count
            >= o["server_limit"]
                .as_i64()
                .unwrap_or(state.config.organizations.max_servers)
        {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "limit",
                "Organization server limit reached.",
            ));
        }
    }
    let (mut row, _) = target(&actor, &body, None).await?;
    switches(&o, &body, &mut row)?;
    let id = uuid::Uuid::new_v4().to_string();
    let encrypted = crypto::encrypt_secret(&state.config.encryption_key, password)
        .map_err(|_| ApiError::bad("Could not encrypt RCON credential."))?;
    sqlx::query("INSERT INTO servers(id,org_id,name,host,port,scheme,password_enc,allow_private,notes,sort_order,public_status,public_leaderboards,public_kills,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)").bind(&id).bind(&org).bind(row["name"].as_str()).bind(row["host"].as_str()).bind(row["port"].as_i64().unwrap() as i32).bind(row["scheme"].as_str()).bind(encrypted).bind(row["allow_private"].as_bool()).bind(row["notes"].as_str()).bind(row["sort_order"].as_i64().unwrap() as i32).bind(row["public_status"].as_bool()).bind(row["public_leaderboards"].as_bool()).bind(row["public_kills"].as_bool()).bind(&actor.id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO server_lists(server_id,list_id) SELECT $1,id FROM lists WHERE org_id=$2 AND server_id IS NULL ON CONFLICT DO NOTHING").bind(&id).bind(&org).execute(&mut *tx).await?;
    for kind in ["ban", "reserve"] {
        let list = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO lists(id,org_id,server_id,kind,name) VALUES($1,$2,$3,$4,'Server')",
        )
        .bind(&list)
        .bind(&org)
        .bind(&id)
        .bind(kind)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO server_lists(server_id,list_id) VALUES($1,$2)")
            .bind(&id)
            .bind(list)
            .execute(&mut *tx)
            .await?;
    }
    trail(
        &mut tx,
        &actor,
        &headers,
        &id,
        &org,
        row["name"].as_str().unwrap(),
        "server.create",
        &format!("{}:{}", row["host"].as_str().unwrap(), row["port"]),
        json!({"scheme":row["scheme"],"orgId":org,"allowPrivate":row["allow_private"]}),
    )
    .await?;
    observe(&mut tx, &id, true).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"ok":true,"id":id}))))
}
async fn locked_server(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    id: &str,
) -> Result<(Value, Value)> {
    let org: Option<String> = sqlx::query_scalar("SELECT org_id FROM servers WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    let o = orgs::locked_org(tx, actor, &org.ok_or_else(ApiError::missing)?).await?;
    let row: Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM servers s WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(ApiError::missing)?;
    Ok((row, o))
}
pub async fn test(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (actor, scope) = manager(&state, &headers, &Method::POST, &id).await?;
    let started = std::time::Instant::now();
    let _lane = crate::dispatcher::acquire(&id, 0, std::time::Duration::from_secs(30)).await?;
    manager(&state, &headers, &Method::POST, &id).await?;
    let work = async {
        let client = crate::game::Client::for_server(&state, &id).await?;
        let status = crate::actions::run(&client, "status", &json!({"raw":true})).await?;
        let capabilities = crate::actions::run(&client, "capabilities", &json!({}))
            .await
            .unwrap_or(Value::Null);
        let server_id = if capabilities["features"]["serverId"] == true {
            crate::actions::run(&client, "serverId", &json!({}))
                .await
                .ok()
                .and_then(|v| v["serverId"].as_str().map(str::to_owned))
                .unwrap_or_default()
        } else {
            String::new()
        };
        Ok::<_, crate::game::Error>((status, capabilities, server_id))
    }
    .await;
    let duration = started.elapsed().as_millis() as i64;
    let (out, ok, status, message) = match work {
        Ok((status, capabilities, server_id)) => (
            json!({"ok":true,"status":status,"capabilities":capabilities,"serverId":server_id,"durationMs":duration}),
            true,
            200,
            String::new(),
        ),
        Err(crate::game::Error::Game(e)) => {
            let message = if e.status == 401 {
                format!(
                    "The game server rejected the stored RCON password: {}",
                    e.message
                )
            } else {
                e.message
            };
            (
                json!({"ok":false,"error":{"message":message,"status":e.status},"durationMs":duration}),
                false,
                e.status as i32,
                message,
            )
        }
        Err(crate::game::Error::Api(e)) => (
            json!({"ok":false,"error":{"message":e.message,"status":e.status.as_u16()},"durationMs":duration}),
            false,
            e.status.as_u16() as i32,
            e.message,
        ),
    };
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO audit_log(actor_id,actor_name,server_id,server_name,org_id,category,action,outcome,status,message,duration_ms) VALUES($1,$2,$3,$4,$5,'server','server.test',$6,$7,$8,$9)").bind(&actor.id).bind(&actor.name).bind(&id).bind(&scope.name).bind(&scope.org_id).bind(if ok{"ok"}else{"error"}).bind(status).bind(crate::feed::truncate(&message,1000)).bind(duration).execute(&mut *tx).await?;
    if ok {
        observe(&mut tx, &id, true).await?;
    }
    tx.commit().await?;
    Ok(Json(out))
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (actor, _) = manager(&state, &headers, &Method::PATCH, &id).await?;
    let mut tx = state.db.begin().await?;
    let (old, o) = locked_server(&mut tx, &actor, &id).await?;
    let (mut row, moved) = target(&actor, &body, Some(&old)).await?;
    switches(&o, &body, &mut row)?;
    let rotated = body["password"].as_str().is_some_and(|s| !s.is_empty());
    if rotated {
        row["password_enc"] = json!(
            crypto::encrypt_secret(
                &state.config.encryption_key,
                body["password"].as_str().unwrap()
            )
            .map_err(|_| ApiError::bad("Could not encrypt RCON credential."))?
        )
    }
    if row == old
        && ![
            "name",
            "host",
            "port",
            "scheme",
            "notes",
            "sortOrder",
            "publicStatus",
            "publicLeaderboards",
            "publicKills",
        ]
        .iter()
        .any(|k| body.get(*k).is_some())
    {
        return Err(ApiError::bad("Nothing to update."));
    }
    sqlx::query("UPDATE servers SET name=$2,host=$3,port=$4,scheme=$5,password_enc=$6,allow_private=$7,notes=$8,sort_order=$9,public_status=$10,public_leaderboards=$11,public_kills=$12,updated_at=now() WHERE id=$1").bind(&id).bind(row["name"].as_str()).bind(row["host"].as_str()).bind(row["port"].as_i64().unwrap() as i32).bind(row["scheme"].as_str()).bind(row["password_enc"].as_str()).bind(row["allow_private"].as_bool()).bind(row["notes"].as_str()).bind(row["sort_order"].as_i64().unwrap() as i32).bind(row["public_status"].as_bool()).bind(row["public_leaderboards"].as_bool()).bind(row["public_kills"].as_bool()).execute(&mut *tx).await?;
    trail(&mut tx,&actor,&headers,&id,old["org_id"].as_str().unwrap(),row["name"].as_str().unwrap(),"server.update","",json!({"credentialRotated":rotated,"targetChanged":moved,"name":row["name"],"host":row["host"],"port":row["port"],"scheme":row["scheme"],"allowPrivate":row["allow_private"],"publicStatus":row["public_status"],"publicLeaderboards":row["public_leaderboards"],"publicKills":row["public_kills"]})).await?;
    if moved || rotated {
        observe(&mut tx, &id, true).await?
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (actor, _) = manager(&state, &headers, &Method::DELETE, &id).await?;
    let mut tx = state.db.begin().await?;
    let (s, _) = locked_server(&mut tx, &actor, &id).await?;
    trail(
        &mut tx,
        &actor,
        &headers,
        &id,
        s["org_id"].as_str().unwrap(),
        s["name"].as_str().unwrap(),
        "server.delete",
        &format!("{}:{}", s["host"].as_str().unwrap(), s["port"]),
        Value::Null,
    )
    .await?;
    sqlx::query("DELETE FROM servers WHERE id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    observe(&mut tx, &id, true).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn grants(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    manager(&state, &headers, &Method::GET, &id).await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('userId',u.id,'username',u.username,'name',u.name,'roleId',r.id,'roleName',r.name,'createdAt',g.created_at) FROM server_grants g JOIN \"user\" u ON u.id=g.user_id JOIN org_roles r ON r.id=g.role_id WHERE g.server_id=$1 ORDER BY u.username").bind(id).fetch_all(&state.db).await?;
    Ok(Json(json!({"ok":true,"grants":rows})))
}
pub async fn set_grants(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (actor, _) = manager(&state, &headers, &Method::PUT, &id).await?;
    let mut tx = state.db.begin().await?;
    let (server, _) = locked_server(&mut tx, &actor, &id).await?;
    let org = server["org_id"].as_str().unwrap();
    let known:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('userId',m.user_id,'roleId',r.id,'roleName',r.name) FROM org_members m JOIN org_roles r ON r.org_id=m.org_id WHERE m.org_id=$1 FOR SHARE OF m,r").bind(org).fetch_all(&mut *tx).await?;
    let mut seen = std::collections::HashSet::new();
    let mut applied = vec![];
    for wanted in body["grants"].as_array().into_iter().flatten() {
        let uid = string(&wanted["userId"], 64);
        let role = string(&wanted["roleId"], 64);
        if seen.contains(&uid) {
            continue;
        }
        if let Some(g) = known
            .iter()
            .find(|g| g["userId"] == uid && g["roleId"] == role)
        {
            seen.insert(uid);
            applied.push(g.clone())
        }
    }
    sqlx::query("DELETE FROM server_grants WHERE server_id=$1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    for grant in &applied {
        sqlx::query(
            "INSERT INTO server_grants(server_id,user_id,role_id,granted_by) VALUES($1,$2,$3,$4)",
        )
        .bind(&id)
        .bind(grant["userId"].as_str())
        .bind(grant["roleId"].as_str())
        .bind(&actor.id)
        .execute(&mut *tx)
        .await?;
    }
    trail(
        &mut tx,
        &actor,
        &headers,
        &id,
        org,
        server["name"].as_str().unwrap(),
        "server.grants",
        "",
        json!({"grants":applied}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"grants":applied})))
}
